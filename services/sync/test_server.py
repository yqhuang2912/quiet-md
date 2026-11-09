import copy
import json
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

from server import Database, handler


def document(title):
    return {"kind": "doc", "meta": {"created": 1, "modified": 1},
            "body": f"<doc><title>{title}</title><p>text</p></doc>", "deleted": None, "root": None}


class SyncTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.db = Database(Path(self.tmp.name) / "sync.sqlite3")

    def tearDown(self):
        self.tmp.cleanup()

    def exchange(self, device, changes, **extra):
        return self.db.exchange({"protocol": 1, "library": "personal", "device": device, "changes": changes, **extra})

    def test_offline_edits_keep_both_and_retry_is_idempotent(self):
        a = document("initial")
        initial = self.exchange("a", {"doc/note": {"base": 0, "value": a}})
        rev = initial["versions"]["doc/note"]
        self.exchange("a", {"doc/note": {"base": rev, "value": document("Mac edit")}})
        change = {"doc/note": {"base": rev, "value": document("Windows edit")}}
        first = self.exchange("b", change)
        second = self.exchange("b", change)
        self.assertEqual(first["records"], second["records"])
        self.assertEqual(len(first["records"]), 2)
        self.assertIn("Mac edit", first["records"]["doc/note"]["body"])
        copy_key = first["conflicts"][0]
        self.assertIn("Windows edit (sync conflict)", first["records"][copy_key]["body"])

    def test_purge_tombstone_prevents_stale_resurrection(self):
        initial = self.exchange("a", {"doc/note": {"base": 0, "value": document("initial")}})
        rev = initial["versions"]["doc/note"]
        self.exchange("a", {"doc/note": {"base": rev, "value": None}})
        response = self.exchange("b", {"doc/note": {"base": rev, "value": document("offline")}})
        self.assertNotIn("doc/note", response["records"])
        self.assertIn("doc/note", response["versions"])
        self.assertEqual(len(response["records"]), 1)

    def test_delete_against_concurrent_edit_keeps_edited_note(self):
        initial = self.exchange("a", {"doc/note": {"base": 0, "value": document("initial")}})
        rev = initial["versions"]["doc/note"]
        self.exchange("a", {"doc/note": {"base": rev, "value": document("edited")}})
        response = self.exchange("b", {"doc/note": {"base": rev, "value": None}})
        self.assertIn("edited", response["records"]["doc/note"]["body"])
        self.assertEqual(response["conflicts"], ["doc/note"])

    def test_batch_rolls_back_on_asset_collision(self):
        asset = {"kind": "asset", "data": "YQ=="}
        self.exchange("a", {"asset/image.png": {"base": 0, "value": asset}})
        with self.assertRaises(ValueError):
            self.exchange("b", {"doc/new": {"base": 0, "value": document("new")},
                                "asset/image.png": {"base": 0, "value": {"kind": "asset", "data": "Yg=="}}})
        self.assertNotIn("doc/new", self.exchange("a", {})["records"])

    def test_invalid_paths_and_external_files(self):
        for key in ("asset/../secret", "asset/C:/secret", "asset/CON", "asset/a\\b", "doc/../secret"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.exchange("a", {key: {"base": 0, "value": {"kind": "asset", "data": ""}}})
        local = document("local")
        local["meta"]["path"] = "/Users/me/private.md"
        with self.assertRaises(ValueError):
            self.exchange("a", {"doc/local": {"base": 0, "value": local}})

    def test_identity_and_library_isolation(self):
        first = self.exchange("a", {"doc/note": {"base": 0, "value": document("private")}})
        with self.assertRaises(ValueError):
            self.exchange("a", {}, server_id="different-database")
        second = self.db.exchange({"protocol": 1, "library": "other", "device": "b", "changes": {}})
        self.assertEqual(second["records"], {})
        self.assertEqual(first["server_id"], second["server_id"])

    def test_concurrent_folder_moves_do_not_create_cycle(self):
        folder = document("folder")
        folder["meta"]["folder"] = "folder"
        folder["body"] = ""
        self.exchange("a", {"doc/a": {"base": 0, "value": folder}, "doc/b": {"base": 0, "value": folder}})
        initial = self.exchange("b", {})
        one, two = copy.deepcopy(folder), copy.deepcopy(folder)
        one["meta"]["parent"], two["meta"]["parent"] = "b", "a"
        self.exchange("a", {"doc/a": {"base": initial["versions"]["doc/a"], "value": one}})
        result = self.exchange("b", {"doc/b": {"base": initial["versions"]["doc/b"], "value": two}})
        self.assertTrue(any(result["records"][key]["meta"].get("parent") is None for key in ("doc/a", "doc/b")))

    def test_http_auth_and_roundtrip(self):
        token = "a" * 32
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler(self.db, token))
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/v1/sync"
            body = json.dumps({"protocol": 1, "library": "personal", "device": "a", "changes": {}}).encode()
            with self.assertRaises(urllib.error.HTTPError) as error:
                urllib.request.urlopen(urllib.request.Request(url, data=body))
            self.assertEqual(error.exception.code, 401)
            request = urllib.request.Request(url, data=body, headers={"Authorization": "Bearer " + token})
            with urllib.request.urlopen(request) as response:
                self.assertEqual(json.load(response)["protocol"], 1)
        finally:
            server.shutdown()
            server.server_close()
            worker.join()


if __name__ == "__main__":
    unittest.main()
