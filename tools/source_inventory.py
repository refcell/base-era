#!/usr/bin/env python3
"""Summarize the spike's Cargo unit graphs; never modify either source tree.

Generate the six history-copy-*-units.json inputs as documented in
docs/source-inventory.md. Run with Python 3.11+ and redirect stdout to JSON.
"""

import argparse
import hashlib
import json
import subprocess
import tomllib
from collections import Counter
from pathlib import Path
from urllib.parse import unquote, urlparse


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args])


def package_paths(units):
    return {
        Path(unquote(urlparse(unit["pkg_id"][5:]).path))
        for unit in units
        if unit["pkg_id"].startswith("path+")
    }


def dependencies(table):
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        yield from table.get(section, {}).items()
    for target in table.get("target", {}).values():
        yield from dependencies(target)


def declared_closure(root, selected):
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["dependencies"]
    seen, pending = set(), list(selected)
    while pending:
        directory = pending.pop()
        if directory in seen:
            continue
        seen.add(directory)
        manifest = tomllib.loads((directory / "Cargo.toml").read_text())
        for name, dependency in dependencies(manifest):
            anchor = directory
            if not isinstance(dependency, dict):
                continue
            if dependency.get("workspace"):
                dependency, anchor = workspace[name], root
            if isinstance(dependency, dict) and "path" in dependency:
                path = (anchor / dependency["path"]).resolve()
                if path.is_relative_to(root):
                    pending.append(path)
    return seen


def relative(paths, root):
    return sorted(str(path.relative_to(root)) for path in paths)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", type=Path, help="Existing Base spike checkout")
    args = parser.parse_args()
    root = args.base.resolve()
    frozen = root / "etc/history-worker/generated/base"
    reference = root / "target/history-reference-source"
    reth = root / "target/history-reth"
    graph_names = ("host", "proof", "worker", "host-tests", "worker-tests", "reference")
    selected = {"host": set(), "worker": set(), "reference": set()}
    graphs = {}
    for name in graph_names:
        graph_file = root / f"target/history-copy-{name}-units.json"
        data = graph_file.read_bytes()
        graph = json.loads(data)
        units = graph["units"]
        summaries = []
        for index in graph["roots"]:
            reached, pending = set(), [index]
            while pending:
                current = pending.pop()
                if current in reached:
                    continue
                reached.add(current)
                pending.extend(d["index"] for d in units[current]["dependencies"])
            paths = package_paths(units[i] for i in reached)
            host_paths = {
                p for p in paths if p.is_relative_to(root)
                and not p.is_relative_to(root / "target")
                and not p.is_relative_to(root / "etc/history-worker")
            }
            worker_paths = {p for p in paths if p.is_relative_to(frozen)}
            reference_paths = {p for p in paths if p.is_relative_to(reference)}
            selected["host"].update(host_paths)
            selected["worker"].update(worker_paths)
            selected["reference"].update(reference_paths)
            summaries.append({
                "target": units[index]["target"]["name"],
                "mode": units[index]["mode"],
                "total_packages": len({units[i]["pkg_id"] for i in reached}),
                "host_base_packages": len(host_paths),
                "host_reth_packages": sum(p.is_relative_to(reth) for p in paths),
                "historical_base_packages": len(worker_paths),
                "reference_base_packages": len(reference_paths),
            })
        graphs[name] = {"sha256": hashlib.sha256(data).hexdigest(), "roots": summaries}

    sections = {}
    for name, source in (("host", root), ("worker", frozen), ("reference", reference)):
        compiled = selected[name]
        declared = declared_closure(source, compiled)
        # The shared protocol lives in the independent workspace, not Base's source snapshot.
        if name == "host":
            declared = {p for p in declared if not p.is_relative_to(root / "etc/history-worker")}
        sections[name] = {
            "compiled_packages": relative(compiled, source),
            "additional_manifest_packages": relative(declared - compiled, source),
            "copy_packages": relative(declared, source),
        }

    metadata = json.loads((root / "target/history-metadata-final.json").read_bytes())
    member_ids = set(metadata["workspace_members"])
    members = {
        Path(p["manifest_path"]).parent for p in metadata["packages"]
        if p["id"] in member_ids
    }
    sections["host"]["omitted_workspace_packages"] = relative(members - selected["host"], root)
    names = git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard").decode().split("\0")
    files = sorted({n for n in names if n and (root / n).is_file()
                    and any((root / n).is_relative_to(p) for p in selected["host"])})
    sections["host"]["copy_files"] = files
    sections["host"]["copy_bytes"] = sum((root / n).stat().st_size for n in files)
    sections["host"]["rust_physical_lines"] = sum(
        (root / n).read_bytes().count(b"\n") for n in files if n.endswith(".rs")
    )
    sections["host"]["family_counts"] = dict(sorted(Counter(
        "/".join(p.relative_to(root).parts[:2]) for p in selected["host"]
    ).items()))
    print(json.dumps({
        "schema": 1,
        "scope": "Selected Linux build/test plans; not a standalone-copy build validation",
        "base_revision": git(root, "rev-parse", "HEAD").decode().strip(),
        "base_tracked_diff_sha256": hashlib.sha256(git(root, "diff", "HEAD", "--binary")).hexdigest(),
        "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(),
        "worker_lock_sha256": hashlib.sha256((root / "etc/history-worker/Cargo.lock").read_bytes()).hexdigest(),
        "graphs": graphs,
        "sources": sections,
    }, indent=2))


if __name__ == "__main__":
    main()
