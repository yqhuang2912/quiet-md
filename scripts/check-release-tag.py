#!/usr/bin/env python3
"""Validate that a personal release tag matches both Cargo package versions."""
import os
from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
def version_of(path):
    return re.search(r'^version = "([^"]+)"$', path.read_text(encoding="utf-8"), re.M).group(1)

version = version_of(root / "Cargo.toml")
core = version_of(root / "crates/core/Cargo.toml")
if version != core or not re.fullmatch(r"\d+\.\d+\.\d+", version):
    raise SystemExit("Root and core must share a numeric major.minor.patch version")
tag = f"mynote-v{version}"
if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ.get("GITHUB_REF_NAME") != tag:
    raise SystemExit(f"Tag must match the source version: {tag}")
print(tag)
