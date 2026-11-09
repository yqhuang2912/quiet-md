#!/usr/bin/env python3
"""Stage this fork's release packages, Minisign signatures and update manifests."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REPOSITORY = "yqhuang2912/quiet-md"
PLATFORMS = ("windows-x64", "macos-arm64")


def package_name(version, platform):
    suffix = "-setup.exe" if platform == "windows-x64" else ".zip"
    return f"quiet-md-{version}-{platform}{suffix}"


def prepare(packages, output, key, public_key, minisign="minisign", platforms=PLATFORMS):
    version = re.search(r'^version = "([^"]+)"$', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M).group(1)
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Personal stable releases need a numeric major.minor.patch version")
    tag = f"mynote-v{version}"
    packages, output = Path(packages), Path(output)
    output.mkdir(parents=True, exist_ok=True)
    checked = []
    for platform in platforms:
        name = package_name(version, platform)
        found = [p for p in packages.rglob(name) if p.is_file()]
        if len(found) != 1:
            raise ValueError(f"Expected exactly one {name}, found {len(found)}")
        source = found[0]
        # Packaging scripts write a checksum alongside each artifact.
        checksum = source.with_name(name + ".sha256")
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        declared = checksum.read_text(encoding="utf-8").split() if checksum.is_file() else []
        if not declared or declared[0].lower() != digest:
            raise ValueError(f"Missing or mismatched packaging checksum: {name}")
        if not 0 < source.stat().st_size <= 1 << 30:
            raise ValueError(f"Package size is outside updater limits: {name}")
        checked.append((platform, name, source, digest))
    for platform, name, source, digest in checked:
        target = output / name
        if source.resolve() != target.resolve():
            shutil.copyfile(source, target)
        signature = output / (name + ".minisig")
        subprocess.run([str(minisign), "-S", "-W", "-s", str(key), "-m", str(target), "-x", str(signature), "-t", name, "-q"], check=True, capture_output=True)
        subprocess.run([str(minisign), "-V", "-q", "-p", str(public_key), "-m", str(target), "-x", str(signature)], check=True, capture_output=True)
        (output / (name + ".sha256")).write_text(f"{digest}  {name}\n", encoding="utf-8")
        manifest = {
            "version": version,
            "url": f"https://github.com/{REPOSITORY}/releases/download/{tag}/{name}",
            "size": target.stat().st_size,
            "signature": signature.read_text(encoding="utf-8"),
        }
        (output / f"update-{platform}.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return tag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "dist/github-release")
    parser.add_argument("--key", type=Path, required=True)
    parser.add_argument("--public-key", type=Path, default=ROOT / "packaging/mynote-update-key.pub")
    parser.add_argument("--minisign", default="minisign")
    parser.add_argument("--platform", action="append", choices=PLATFORMS)
    args = parser.parse_args()
    tag = prepare(args.packages, args.output, args.key, args.public_key, args.minisign, args.platform or PLATFORMS)
    print(f"Prepared signed assets for {REPOSITORY} / {tag} in {args.output}")


if __name__ == "__main__":
    main()
