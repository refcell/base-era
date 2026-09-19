#!/usr/bin/env python3
"""Measure a real Base deletion diff, separating Rust test syntax from production.

Requires tree-sitter==0.25.2 and tree-sitter-rust==0.24.0. Counts physical
lines (including comments/blanks), not statements. Does not infer removability.
"""

import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
import subprocess

from tree_sitter import Language, Parser
import tree_sitter_rust


args = argparse.ArgumentParser(description=__doc__)
args.add_argument("repo", type=Path)
args.add_argument("--base", required=True)
args.add_argument("--output", type=Path, required=True)
args.add_argument("--patch", type=Path, required=True)
args = args.parse_args()
parser = Parser(Language(tree_sitter_rust.language()))


def git(*argv):
    return subprocess.check_output(["git", "-C", str(args.repo), *argv])


def classify(path, source):
    lines = source.splitlines()
    if not path.endswith(".rs"):
        kind = "embedded_artifacts" if path.endswith(".hex") else "docs_and_build_metadata"
        return [kind] * len(lines)
    if any(part in ("tests", "benches", "test_utils") for part in Path(path).parts) or path.endswith("/test_utils.rs"):
        return ["rust_tests_and_benches"] * len(lines)
    result = ["rust_production"] * len(lines)
    tree = parser.parse(source)
    if tree.root_node.has_error:
        raise ValueError(f"Rust parse failed: {path}")

    def walk(node):
        test_start = None
        for child in node.children:
            text = source[child.start_byte:child.end_byte]
            if child.type == "attribute_item":
                if re.fullmatch(rb"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]", text):
                    test_start = child.start_point.row
                continue
            if child.type in ("line_comment", "block_comment"):
                continue
            if test_start is not None:
                for row in range(test_start, child.end_point.row + 1):
                    if row < len(result):
                        result[row] = "rust_tests_and_benches"
                test_start = None
            else:
                walk(child)

    walk(tree.root_node)
    return result


patch = git("diff", "--no-ext-diff", "--no-renames", "--binary", "--src-prefix=a/", "--dst-prefix=b/", args.base)
files = []
totals = defaultdict(lambda: {"added": 0, "removed": 0})
for path in git("diff", "--no-renames", "--name-only", args.base).decode().splitlines():
    before = git("show", f"{args.base}:{path}")
    after = (args.repo / path).read_bytes() if (args.repo / path).exists() else b""
    old_kinds, new_kinds = classify(path, before), classify(path, after)
    counts = defaultdict(lambda: {"added": 0, "removed": 0})
    old = new = None
    diff = git("diff", "--no-ext-diff", "--no-renames", "--unified=0", args.base, "--", path).decode()
    for line in diff.splitlines():
        if line.startswith("@@"):
            match = re.match(r"@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@", line)
            old, new = map(int, match.groups())
        elif old is not None:
            if line.startswith("-"):
                counts[old_kinds[old - 1]]["removed"] += 1
                old += 1
            elif line.startswith("+"):
                counts[new_kinds[new - 1]]["added"] += 1
                new += 1
            elif line.startswith(" "):
                old += 1
                new += 1
    for kind, values in counts.items():
        for key, value in values.items():
            totals[kind][key] += value
    files.append({"path": path, "counts": dict(counts), "bytes_removed_net": len(before) - len(after)})

for values in totals.values():
    values["net_removed"] = values["removed"] - values["added"]
data = {
    "base": git("rev-parse", args.base).decode().strip(),
    "candidate": git("rev-parse", "HEAD").decode().strip(),
    "candidate_tree": git("write-tree").decode().strip(),
    "patch_sha256": hashlib.sha256(patch).hexdigest(),
    "counting": "Physical diff lines, including comments/blanks. Rust syntax under cfg(test), test_utils, tests and benches is separate. Not integration-adjusted savings; no historical worker added by this patch.",
    "totals": dict(totals),
    "files": files,
}
args.output.write_text(json.dumps(data, indent=2) + "\n")
args.patch.write_bytes(patch)
print(json.dumps(data["totals"], indent=2))
