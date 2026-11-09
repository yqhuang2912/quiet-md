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
