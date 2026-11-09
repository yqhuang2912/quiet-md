"""Personal NAS sync service. Standard library only; SQLite owns all revisions."""
import base64
import copy
from contextlib import contextmanager
import hmac
import json
import os
import re
import sqlite3
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MAX_BODY = 128 * 1024 * 1024
MAX_FILE = 32 * 1024 * 1024
ID = re.compile(r"[A-Za-z0-9_-]{1,128}\Z")
RESERVED = re.compile(r"(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?\Z", re.I)


def dumps(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def validate(key, value):
    if not isinstance(key, str) or "/" not in key:
        raise ValueError("Invalid record key")
    kind, name = key.split("/", 1)
    if kind == "doc":
        if not ID.fullmatch(name):
            raise ValueError("Invalid document ID")
    elif kind == "asset":
        if len(name) > 1024 or not name or any(
            not p or p in (".", "..") or p.endswith((".", " ")) or RESERVED.fullmatch(p)
            or any(c in p for c in '<>:"|?*\\') or any(ord(c) < 32 for c in p)
            for p in name.split("/")
        ):
            raise ValueError("Invalid portable asset path")
    else:
        raise ValueError("Unknown record kind")
    if value is None:
        return
    if not isinstance(value, dict) or value.get("kind") != kind:
        raise ValueError("Invalid record")
    if kind == "asset":
        data = base64.b64decode(value.get("data", ""), validate=True)
        if len(data) > MAX_FILE:
            raise ValueError("Attachment exceeds 32 MiB")
    else:
        meta, body = value.get("meta"), value.get("body")
        if not isinstance(meta, dict) or not isinstance(body, str) or meta.get("path") is not None:
            raise ValueError("External files are not synchronizable")
        if len(body.encode()) > MAX_FILE:
            raise ValueError("Document exceeds 32 MiB")
        for field in ("parent",):
            if meta.get(field) is not None and not ID.fullmatch(str(meta[field])):
                raise ValueError("Invalid parent")
        if value.get("deleted") is not None and (type(value["deleted"]) is not int or value["deleted"] < 0):
            raise ValueError("Invalid deletion time")


def conflict_copy(value):
    value = copy.deepcopy(value)
    value["deleted"], value["root"] = None, None
    meta = value["meta"]
    meta["parent"] = None
    suffix = " (sync conflict)"
    if meta.get("folder") is not None:
        meta["folder"] += suffix
    elif meta.get("markdown"):
        meta["name"] = (meta.get("name") or "Untitled") + suffix
    else:
        # Preserve every original XML byte except the title suffix.
        value["body"] = value["body"].replace("</title>", suffix + "</title>", 1)
    return value


class Database:
    def __init__(self, path):
        self.path = str(path)
        with self.connect() as db:
            db.executescript("""
                PRAGMA journal_mode=WAL;
                CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS records (
                    library TEXT NOT NULL, key TEXT NOT NULL, revision INTEGER NOT NULL,
                    value TEXT, PRIMARY KEY(library, key));
                CREATE TABLE IF NOT EXISTS history (
                    revision INTEGER PRIMARY KEY AUTOINCREMENT, library TEXT NOT NULL,
                    key TEXT NOT NULL, value TEXT, saved TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
            """)
            db.execute("INSERT OR IGNORE INTO settings VALUES ('server_id', ?)", (str(uuid.uuid4()),))

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.path, timeout=30)
        try:
            db.execute("PRAGMA synchronous=FULL")
            with db:
                yield db
        finally:
            db.close()

    def exchange(self, request):
        if request.get("protocol") != 1:
            raise ValueError("Unsupported sync protocol")
        library, device = request.get("library"), request.get("device")
        if not isinstance(library, str) or not ID.fullmatch(library):
            raise ValueError("Invalid library name")
        if not isinstance(device, str) or not ID.fullmatch(device):
            raise ValueError("Invalid device ID")
        changes = request.get("changes")
        if not isinstance(changes, dict) or len(changes) > 10000:
            raise ValueError("Invalid change set")
        for key, change in changes.items():
            if not isinstance(change, dict) or type(change.get("base")) is not int or change["base"] < 0:
                raise ValueError("Invalid base revision")
            validate(key, change.get("value"))
        with self.connect() as db:
            db.execute("BEGIN IMMEDIATE")
            server_id = db.execute("SELECT value FROM settings WHERE key='server_id'").fetchone()[0]
            if request.get("server_id") not in (None, server_id):
                raise ValueError("NAS database identity changed; restore its backup or configure a new library")
            rows = {key: (rev, json.loads(value) if value is not None else None)
                    for key, rev, value in db.execute("SELECT key, revision, value FROM records WHERE library=?", (library,))}

            def put(key, value):
                if key in rows and rows[key][1] == value:
                    return
                encoded = dumps(value) if value is not None else None
                rev = db.execute("INSERT INTO history(library,key,value) VALUES (?,?,?)", (library, key, encoded)).lastrowid
                db.execute("INSERT INTO records VALUES (?,?,?,?) ON CONFLICT(library,key) DO UPDATE SET revision=excluded.revision,value=excluded.value",
                           (library, key, rev, encoded))
                rows[key] = (rev, value)

            conflicts = []
            for key, change in changes.items():
                revision, remote = rows.get(key, (0, None))
                incoming = change.get("value")
                if remote == incoming:
                    continue
                if key.startswith("asset/") and remote is not None:
                    raise ValueError("Attachment name collision; existing bytes were preserved")
                if revision == change["base"]:
                    put(key, incoming)
                elif key.startswith("doc/"):
                    if incoming is not None:
                        # Deterministic: retrying after a lost response cannot multiply copies.
                        ident = uuid.uuid5(uuid.NAMESPACE_URL, device + key + dumps(incoming)).hex
                        conflict_key = "doc/" + ident
                        put(conflict_key, conflict_copy(incoming))
                        conflicts.append(conflict_key)
                    else:
                        conflicts.append(key)  # concurrent edit wins over stale purge
                else:
                    put(key, incoming)

            # Concurrent folder moves can form cycles. Detach an affected item
            # rather than making documents disappear from the tree.
            for key, (_, value) in list(rows.items()):
                if not key.startswith("doc/") or value is None or value.get("deleted") is not None:
                    continue
                parent, seen = value["meta"].get("parent"), {key[4:]}
                while parent:
                    target = rows.get("doc/" + parent, (0, None))[1]
                    if parent in seen or target is None or target.get("deleted") is not None or target["meta"].get("folder") is None:
                        value = copy.deepcopy(value)
                        value["meta"]["parent"] = None
                        put(key, value)
                        break
                    seen.add(parent)
                    parent = target["meta"].get("parent")
            response = {"protocol": 1, "server_id": server_id,
                        "versions": {k: r for k, (r, _) in rows.items()},
                        "records": {k: v for k, (_, v) in rows.items() if v is not None},
                        "conflicts": conflicts}
            if len(dumps(response).encode()) > MAX_BODY:
                raise ValueError("Library exceeds the 128 MiB prototype transfer limit")
            return response


def handler(database, token):
    class Handler(BaseHTTPRequestHandler):
        def setup(self):
            super().setup()
            self.connection.settimeout(60)

        def reply(self, status, value):
            body = dumps(value).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            self.reply(200 if self.path == "/health" else 404, {"service": "mynote-sync", "protocol": 1})

        def do_POST(self):
            if self.path != "/v1/sync":
                return self.reply(404, {"error": "Not found"})
            supplied = self.headers.get("Authorization", "").encode()
            if not hmac.compare_digest(supplied, ("Bearer " + token).encode()):
                return self.reply(401, {"error": "Invalid access token"})
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 0 < size <= MAX_BODY:
                    return self.reply(413, {"error": "Invalid request size"})
                body = self.rfile.read(size)
                if len(body) != size:
                    raise ValueError("Incomplete request")
                request = json.loads(body)
                if not isinstance(request, dict):
                    raise ValueError("Invalid request")
                result = database.exchange(request)
            except (ValueError, TypeError, KeyError, OverflowError) as error:
                return self.reply(400, {"error": str(error)})
            except sqlite3.Error:
                return self.reply(503, {"error": "NAS storage unavailable; retry later"})
            self.reply(200, result)

    return Handler


if __name__ == "__main__":
    token = os.environ.get("SYNC_TOKEN", "")
    if len(token) < 32 or not token.isascii() or not token.isalnum():
        raise SystemExit("Set SYNC_TOKEN to at least 32 ASCII letters/digits")
    data = Path(os.environ.get("SYNC_DATA", "/data"))
    data.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer((os.environ.get("SYNC_HOST", "0.0.0.0"), int(os.environ.get("SYNC_PORT", "8787"))), handler(Database(data / "sync.sqlite3"), token))
    print("mynote-sync listening", flush=True)
    server.serve_forever()
