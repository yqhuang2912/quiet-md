#!/usr/bin/env python3
"""Atomically publish both update manifests to the dedicated updates branch."""
import argparse
import base64
import json
from pathlib import Path
import re
import subprocess

REPO = "yqhuang2912/quiet-md"
BRANCH = "updates"
PLATFORMS = ("windows-x64", "macos-arm64")


def api(endpoint, method="GET", payload=None, missing_ok=False):
    args = ["gh", "api", f"repos/{REPO}/{endpoint}", "--method", method]
    if payload is not None:
        args += ["--input", "-"]
    result = subprocess.run(args, input=json.dumps(payload) if payload is not None else None,
                            capture_output=True, text=True, encoding="utf-8")
    if result.returncode:
        if missing_ok and "(HTTP 404)" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip())
    return json.loads(result.stdout)


def version(value):
    if not isinstance(value, str) or not re.fullmatch(r"\d+\.\d+\.\d+", value):
        raise ValueError("Invalid manifest version")
    return tuple(map(int, value.split(".")))


def publish(directory, request=api):
    manifests = {}
    for platform in PLATFORMS:
        name = f"update-{platform}.json"
        text = (Path(directory) / name).read_text(encoding="utf-8")
        manifest = json.loads(text)
        version(manifest["version"])
        suffix = "-setup.exe" if platform == "windows-x64" else ".zip"
        package = f'quiet-md-{manifest["version"]}-{platform}{suffix}'
        expected = f'https://github.com/{REPO}/releases/download/mynote-v{manifest["version"]}/{package}'
        if manifest["url"] != expected or not 0 < manifest["size"] <= 1 << 30 or f"trusted comment: {package}\n" not in manifest["signature"]:
            raise ValueError("Invalid manifest package, size or signed filename")
        manifests[name] = (manifest, text)
    versions = {m[0]["version"] for m in manifests.values()}
    if len(versions) != 1:
        raise ValueError("Both platforms must publish the same version")
    current = versions.pop()
    release = request(f"releases/tags/mynote-v{current}")
    if release["draft"] or release["prerelease"]:
        raise ValueError("Update channel requires a public stable release")
    assets = {a["name"]: a for a in release["assets"]}
    for manifest, _ in manifests.values():
        asset = assets.get(manifest["url"].rsplit("/", 1)[1], {})
        if asset.get("size") != manifest["size"] or asset.get("browser_download_url") != manifest["url"]:
            raise ValueError("Public release is missing a complete matching installer")
    ref = request(f"git/ref/heads/{BRANCH}", missing_ok=True)
    head = ref["object"]["sha"] if ref else None
    if head:
        for name in manifests:
            old = request(f"contents/{name}?ref={head}")
            old = json.loads(base64.b64decode(old["content"]).decode("utf-8"))
            if version(old["version"]) > version(current):
                raise ValueError("Refusing to replace a newer update channel")
    tree = request("git/trees", "POST", {"tree": [
        {"path": name, "mode": "100644", "type": "blob", "content": text}
        for name, (_, text) in manifests.items()
    ]})
    commit = request("git/commits", "POST", {"message": f"Publish MyNote {current} update manifests", "tree": tree["sha"], "parents": [head] if head else []})
    # No forced updates: another release winning this race must be rechecked.
    if head:
        request(f"git/refs/heads/{BRANCH}", "PATCH", {"sha": commit["sha"], "force": False})
    else:
        request("git/refs", "POST", {"ref": f"refs/heads/{BRANCH}", "sha": commit["sha"]})
    return commit["sha"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    print(f"Published update manifests: {publish(args.directory)}")
