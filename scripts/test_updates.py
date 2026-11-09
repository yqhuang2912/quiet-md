"""Exercise release signing with throwaway keys; never use the release secret."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("prepare_updates", Path(__file__).with_name("prepare-updates.py"))
UPDATES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UPDATES)
MINISIGN = os.environ.get("MYNOTE_MINISIGN") or shutil.which("minisign")
PUBLISH_SPEC = importlib.util.spec_from_file_location("publish_updates", Path(__file__).with_name("publish-update-manifests.py"))
PUBLISH = importlib.util.module_from_spec(PUBLISH_SPEC)
PUBLISH_SPEC.loader.exec_module(PUBLISH)


class ManifestPublication(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = []
        self.calls = []
        for platform in UPDATES.PLATFORMS:
            package = UPDATES.package_name("0.2.1", platform)
            url = f"https://github.com/yqhuang2912/quiet-md/releases/download/mynote-v0.2.1/{package}"
            m = {"version": "0.2.1", "url": url, "size": 42, "signature": f"trusted comment: {package}\n"}
            (self.root / f"update-{platform}.json").write_text(json.dumps(m), encoding="utf-8")
            self.assets.append({"name": package, "size": 42, "browser_download_url": url})
        self.head = None
        self.previous_version = "0.2.0"

    def request(self, endpoint, method="GET", payload=None, missing_ok=False):
        self.calls.append((endpoint, method, payload))
        if endpoint.startswith("releases/"):
            return {"draft": False, "prerelease": False, "assets": self.assets}
        if endpoint == "git/ref/heads/updates":
            return {"object": {"sha": self.head}} if self.head else None
        if endpoint.startswith("contents/"):
            import base64
            return {"content": base64.b64encode(json.dumps({"version": self.previous_version}).encode()).decode()}
        return {"sha": "newcommit" if endpoint == "git/commits" else "tree"}

    def test_initial_branch_contains_only_both_manifests_and_is_orphaned(self):
        PUBLISH.publish(self.root, self.request)
        tree = next(payload for endpoint, _, payload in self.calls if endpoint == "git/trees")
        self.assertEqual({e["path"] for e in tree["tree"]}, {f"update-{p}.json" for p in UPDATES.PLATFORMS})
        commit = next(payload for endpoint, _, payload in self.calls if endpoint == "git/commits")
        self.assertEqual(commit["parents"], [])
        self.assertEqual(self.calls[-1], ("git/refs", "POST", {"ref": "refs/heads/updates", "sha": "newcommit"}))

    def test_existing_branch_advances_without_force_and_rejects_downgrade(self):
        self.head = "oldcommit"
        PUBLISH.publish(self.root, self.request)
        self.assertEqual(self.calls[-1], ("git/refs/heads/updates", "PATCH", {"sha": "newcommit", "force": False}))
        self.calls.clear()
        self.previous_version = "0.3.0"
        with self.assertRaisesRegex(ValueError, "newer"):
            PUBLISH.publish(self.root, self.request)
        self.assertTrue(all(method == "GET" for _, method, _ in self.calls))

    def test_incomplete_release_and_mismatched_platforms_never_change_channel(self):
        self.assets.pop()
        with self.assertRaisesRegex(ValueError, "missing"):
            PUBLISH.publish(self.root, self.request)
        self.assertTrue(all(method == "GET" for _, method, _ in self.calls))
        path = self.root / "update-macos-arm64.json"
        m = json.loads(path.read_text())
        m["version"] = "0.2.2"
        path.write_text(json.dumps(m), encoding="utf-8")
        with self.assertRaises(ValueError):
            PUBLISH.publish(self.root, self.request)


@unittest.skipUnless(MINISIGN, "minisign is required for release-signing tests")
class SignedUpdates(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.key, self.pub = self.root / "test.key", self.root / "test.pub"
        subprocess.run([MINISIGN, "-G", "-W", "-s", str(self.key), "-p", str(self.pub)], check=True, capture_output=True)
        self.packages = self.root / "packages"
        self.packages.mkdir()
        self.output = self.root / "release"
        self.version = subprocess.check_output([os.sys.executable, str(Path(__file__).with_name("check-release-tag.py"))], text=True).strip().removeprefix("mynote-v")
        for platform in UPDATES.PLATFORMS:
            package = self.packages / UPDATES.package_name(self.version, platform)
            package.write_bytes(b"throwaway package for " + platform.encode())
            package.with_name(package.name + ".sha256").write_text(hashlib.sha256(package.read_bytes()).hexdigest() + "  " + package.name, encoding="utf-8")

    def prepare(self, public_key=None):
        return UPDATES.prepare(self.packages, self.output, self.key, public_key or self.pub, MINISIGN)

    def test_all_platforms_have_valid_signatures_and_pinned_release_urls(self):
        tag = self.prepare()
        for platform in UPDATES.PLATFORMS:
            m = json.loads((self.output / f"update-{platform}.json").read_text(encoding="utf-8"))
            name = UPDATES.package_name(self.version, platform)
            self.assertEqual(m["url"], f"https://github.com/yqhuang2912/quiet-md/releases/download/{tag}/{name}")
            package = self.output / name
            self.assertEqual(m["size"], package.stat().st_size)
            self.assertIn(f"trusted comment: {name}\n", m["signature"])
            package.write_bytes(b"tampered")
            result = subprocess.run([MINISIGN, "-V", "-q", "-p", str(self.pub), "-m", str(package), "-x", str(package) + ".minisig"], capture_output=True)
            self.assertNotEqual(result.returncode, 0)

    def test_wrong_key_cannot_publish_manifests(self):
        other = self.root / "other.pub"
        subprocess.run([MINISIGN, "-G", "-W", "-s", str(self.root / "other.key"), "-p", str(other)], check=True, capture_output=True)
        with self.assertRaises(subprocess.CalledProcessError):
            self.prepare(other)
        self.assertFalse(list(self.output.glob("update-*.json")))

    def test_changed_or_missing_packages_are_rejected(self):
        package = self.packages / UPDATES.package_name(self.version, "windows-x64")
        package.write_bytes(b"changed after packaging")
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.prepare()
        package.unlink()
        with self.assertRaisesRegex(ValueError, "exactly one"):
            self.prepare()


if __name__ == "__main__":
    unittest.main()
