#!/usr/bin/env python3
"""Copy Git-visible source without staging, commits, docs or cached build output."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new, nonexistent directory")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    destination = args.destination.resolve()
    files = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root, capture_output=True, check=True,
    ).stdout.split(b"\0")
    manifest = []
    destination.mkdir(parents=True, exist_ok=False)
    for raw in sorted(set(files)):
        if not raw:
            continue
        name = raw.decode("utf-8")
        source = root / name
        if source.is_symlink() or not source.is_file():
            raise ValueError(f"snapshot requires regular source files: {name}")
        output = destination / name
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, output)
        manifest.append({"path": name, "sha256": hashlib.sha256(output.read_bytes()).hexdigest()})
    encoded = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    report = {"sourceSha256": hashlib.sha256(encoded).hexdigest(), "files": manifest}
    (destination / "_source-manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"destination": str(destination), "files": len(manifest),
                      "sourceSha256": report["sourceSha256"]}))


if __name__ == "__main__":
    main()
