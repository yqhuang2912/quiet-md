#!/usr/bin/env python3
"""Bind reused build artifacts to the release tag and its embedded public key."""
import base64
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
REPO = "yqhuang2912/quiet-md"


def api(path):
    return json.loads(subprocess.check_output(["gh", "api", f"repos/{REPO}/{path}"], text=True, encoding="utf-8"))


def main():
    run_id = sys.argv[1]
    if not re.fullmatch(r"[0-9]+", run_id):
        raise SystemExit("Build run ID must be numeric")
    run = api(f"actions/runs/{run_id}")
    sha = run["head_sha"]
    jobs = api(f"actions/runs/{run_id}/jobs?per_page=100&filter=all")["jobs"]
    for name in ("windows", "macos"):
        if not any(j["name"] == name and j["conclusion"] == "success" for j in jobs):
            raise SystemExit(f"The {name} build did not succeed in run {run_id}")
    tag = subprocess.check_output([sys.executable, str(ROOT / "scripts/check-release-tag.py")], text=True, encoding="utf-8").strip()
    if run_id != os.environ.get("GITHUB_RUN_ID"):
        # Recovery builds must belong to the already-created release tag;
        # never attach binaries compiled from a different source revision.
        if api(f"commits/{tag}")["sha"] != sha:
            raise SystemExit("Build artifacts do not belong to this release tag")
    source_key = api(f"contents/packaging/mynote-update-key.pub?ref={sha}")
    embedded = base64.b64decode(source_key["content"]).decode().strip().replace("\r\n", "\n")
    current = (ROOT / "packaging/mynote-update-key.pub").read_text(encoding="utf-8").strip()
    if embedded != current:
        raise SystemExit("Build artifacts embed a different update public key")
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a", encoding="utf-8") as output:
            output.write(f"sha={sha}\n")
    print(f"Verified package source: {sha} ({tag})")


if __name__ == "__main__":
    main()
