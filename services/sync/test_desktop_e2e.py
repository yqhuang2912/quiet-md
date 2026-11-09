"""Exercise two actual Rust stores against the HTTP server, without real user data."""
import json
import os
import subprocess
import tempfile
import threading
import unittest
from http.server import ThreadingHTTPServer
from pathlib import Path

from server import Database, handler

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "target/release/examples" / ("nas_sync.exe" if os.name == "nt" else "nas_sync")


@unittest.skipUnless(HELPER.is_file(), "Build cargo build --release -p fdl_core --example nas_sync first")
class DesktopSyncTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="mynote-e2e-")
        self.root = Path(self.tmp.name)
        self.db = Database(self.root / "nas.sqlite3")
        self.token = "e2eOnly" * 8
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler(self.db, self.token))
        self.worker = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.worker.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"
        self.a, self.b = self.root / "windows", self.root / "macos"
        for library in (self.a, self.b):
            (library / "docs").mkdir(parents=True)
            (library / ".nas-sync").mkdir()
            (library / "format.json").write_text('{"format":3}')
            (library / "index.json").write_text("{}")
            (library / ".nas-sync/config.json").write_text(json.dumps({"url": self.url, "library": "personal", "token": self.token}))

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.worker.join()
        self.tmp.cleanup()

    def sync(self, library, success=True):
        result = subprocess.run([str(HELPER)], env={**os.environ, "QUIET_MD_HOME": str(library)}, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        return result

    def write_note(self, library, title):
        (library / "docs/note.xml").write_text(f"<title>{title}</title>\n<p>Content</p>\n", encoding="utf-8")

    def test_two_desktops_offline_conflicts_attachments_trash_and_purge(self):
        self.write_note(self.a, "Initial")
        asset = "assets/files/uuid/附件.txt"
        (self.a / asset).parent.mkdir(parents=True)
        (self.a / asset).write_bytes(b"attachment bytes")
        self.sync(self.a)
        self.sync(self.b)
        self.assertIn("Initial", (self.b / "docs/note.xml").read_text())
        self.assertEqual((self.b / asset).read_bytes(), b"attachment bytes")

        self.write_note(self.a, "Windows offline")
        self.write_note(self.b, "Mac offline")
        self.sync(self.a)
        result = self.sync(self.b)
        self.assertIn("conflicts=1", result.stdout)
        self.sync(self.a)
        notes = list((self.a / "docs").glob("*.xml"))
        self.assertEqual(len(notes), 2)
        self.assertTrue(any("Mac offline (sync conflict)" in p.read_text() for p in notes))
        self.assertIn("Windows offline", (self.a / "docs/note.xml").read_text())
        # An unchanged retry must not create another copy.
        self.sync(self.b)
        self.assertEqual(len(list((self.b / "docs").glob("*.xml"))), 2)

        index = json.loads((self.a / "index.json").read_text())
        meta = index.pop("note")
        (self.a / "index.json").write_text(json.dumps(index))
        (self.a / "trash").mkdir(exist_ok=True)
        (self.a / "docs/note.xml").rename(self.a / "trash/note.xml")
        (self.a / "trash/index.json").write_text(json.dumps({"note": {"meta": meta, "deleted": 1794210000}}))
        self.sync(self.a)
        self.sync(self.b)
        self.assertFalse((self.b / "docs/note.xml").exists())
        self.assertIn("Windows offline", (self.b / "trash/note.xml").read_text())
        (self.a / "trash/note.xml").unlink()
        (self.a / "trash/index.json").write_text("{}")
        self.sync(self.a)
        self.sync(self.b)
        self.assertFalse((self.b / "trash/note.xml").exists())

    def test_typing_during_accepted_upload_does_not_conflict_with_same_device(self):
        exchange = self.db.exchange
        for uploaded, continued in (("New note", "Still typing a new note"), ("Next edit", "Still typing an existing note")):
            with self.subTest(uploaded=uploaded):
                self.write_note(self.a, uploaded)

                def edit_before_response(request):
                    response = exchange(request)
                    self.write_note(self.a, continued)
                    return response

                self.db.exchange = edit_before_response
                try:
                    self.sync(self.a)
                finally:
                    self.db.exchange = exchange
                self.assertIn(continued, (self.a / "docs/note.xml").read_text())
                result = self.sync(self.a)
                self.assertIn("conflicts=0", result.stdout)
                self.assertEqual(len(list((self.a / "docs").glob("*.xml"))), 1)
                self.sync(self.b)
                self.assertIn(continued, (self.b / "docs/note.xml").read_text())

    def test_wrong_token_does_not_change_files_or_expose_token(self):
        self.write_note(self.a, "private")
        config = self.a / ".nas-sync/config.json"
        wrong = "wrongToken" * 5
        config.write_text(json.dumps({"url": self.url, "library": "personal", "token": wrong}))
        result = self.sync(self.a, success=False)
        self.assertIn("401", result.stderr)
        self.assertNotIn(wrong, result.stderr)
        self.assertIn("private", (self.a / "docs/note.xml").read_text())
        state = json.loads((self.a / ".nas-sync/state.json").read_text())
        self.assertEqual(state["baseline"], {})
