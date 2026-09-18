"""Regression tests for source and reference approval, not simulated execution."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import artifacts


class ArtifactApprovalTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.addCleanup(patch.stopall)
        patch.object(artifacts, "ROOT", self.root).start()
        source = self.root / "reference"
        # Byte sorting puts a-b/file before a/file; Path component sorting does not.
        contents = {"Cargo.lock": b"lock", "a-b/file": b"second", "a/file": b"first"}
        for name, body in contents.items():
            file = source / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(body)
        (source / "link").symlink_to("a/file")
        sums = "".join(f"{hashlib.sha256(body).hexdigest()}  ./{name}\n"
                       for name, body in contents.items())
        manifest = {"reference_base": {
            "path": "reference", "content_sha256": hashlib.sha256(sums.encode()).hexdigest(),
            "lockfile_sha256": hashlib.sha256(b"lock").hexdigest(),
        }}
        (self.root / "sources").mkdir()
        self.approval = self.root / "sources/frozen-sources.json"
        self.approval.write_text(json.dumps(manifest))
        content = b"approved reference executable"
        checksum = hashlib.sha256(content).hexdigest()
        self.artifact = self.root / "target/sha256" / checksum / "reference"
        self.artifact.parent.mkdir(parents=True)
        self.artifact.write_bytes(content)
        self.binary = self.root / "target/reference"
        self.binary.write_bytes(content)
        self.metadata = self.root / "target/history-reference-build.json"
        self.metadata.write_text(json.dumps({
            "source_manifest_sha256": hashlib.sha256(self.approval.read_bytes()).hexdigest(),
            "artifact": str(self.artifact), "sha256": checksum,
        }))

    def test_byte_sorted_sources_and_approved_reference(self):
        artifacts.verify_source("reference_base")
        self.assertEqual(artifacts.verify_reference(self.binary), self.artifact)

    def test_modified_source_is_rejected(self):
        (self.root / "reference/a/file").write_bytes(b"different implementation")
        with self.assertRaisesRegex(RuntimeError, "source integrity mismatch"):
            artifacts.verify_reference(self.binary)

    def test_stale_convenience_binary_is_rejected_and_recovers(self):
        self.binary.write_bytes(b"stale reference executable")
        with self.assertRaisesRegex(RuntimeError, "executable integrity mismatch"):
            artifacts.verify_reference(self.binary)
        self.binary.write_bytes(self.artifact.read_bytes())
        self.assertEqual(artifacts.verify_reference(self.binary), self.artifact)

    def test_modified_content_addressed_binary_is_rejected(self):
        self.artifact.write_bytes(b"modified immutable artifact")
        with self.assertRaisesRegex(RuntimeError, "executable integrity mismatch"):
            artifacts.verify_reference(self.binary)

    def test_changed_approval_requires_new_build_record(self):
        manifest = json.loads(self.approval.read_text())
        manifest["approval_revision"] = 2
        self.approval.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(RuntimeError, "different source approval"):
            artifacts.verify_reference(self.binary)


if __name__ == "__main__":
    unittest.main()
