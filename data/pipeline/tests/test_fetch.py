import hashlib

import pytest

from fetch import digest, expected, verified


def test_digest_and_verify(tmp_path):
    path = tmp_path / "f.txt"
    path.write_bytes(b"hello")
    sha = hashlib.sha256(b"hello").hexdigest()
    assert digest(path, "sha256") == sha
    assert verified(path, {"url": "u", "sha256": sha})
    assert not verified(path, {"url": "u", "sha256": "0" * 64})
    assert not verified(tmp_path / "missing", {"url": "u", "sha256": sha})


def test_sha1_is_accepted_and_one_checksum_is_required():
    assert expected({"url": "u", "sha1": "ABC"}) == ("sha1", "abc")
    with pytest.raises(ValueError):
        expected({"url": "u"})


# ---- downloads, against a local HTTP server ----

import http.server
import threading

from fetch import fetch

BODY = bytes(range(256)) * 400  # 102,400 bytes
SHA = hashlib.sha256(BODY).hexdigest()


class Handler(http.server.BaseHTTPRequestHandler):
    routes: dict = {}
    seen: list = []

    def log_message(self, *args):
        pass

    def do_GET(self):
        route = self.routes.get(self.path)
        self.seen.append((self.path, self.headers.get("Range")))
        if route is None:
            return self.send_error(404)
        if route.get("fail", 0) > 0:
            route["fail"] -= 1
            return self.send_error(503)
        body, start = route["body"], 0
        if self.headers.get("Range") and route.get("ranges", True):
            start = int(self.headers["Range"].split("=")[1].rstrip("-"))
            self.send_response(206)
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(len(body) - start))
        self.end_headers()
        self.wfile.write(body[start:])


@pytest.fixture
def server():
    Handler.routes, Handler.seen = {}, []
    httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{httpd.server_port}", Handler
    httpd.shutdown()


def manifest(base, sha=SHA, mirrors=()):
    return {
        "id": "src",
        "mirrors": [base + m for m in mirrors],
        "files": [{"url": base + "/dumps/f.bin", "sha256": sha, "path": "f.bin"}],
    }


def test_downloads_verifies_and_skips_when_present(tmp_path, server):
    base, h = server
    h.routes["/dumps/f.bin"] = {"body": BODY}
    [path] = fetch(manifest(base), tmp_path, backoff=0)
    assert path.read_bytes() == BODY and not path.with_name("f.bin.part").exists()
    fetch(manifest(base), tmp_path, backoff=0)
    assert len(h.seen) == 1


def test_retries_server_errors_with_backoff(tmp_path, server):
    base, h = server
    h.routes["/dumps/f.bin"] = {"body": BODY, "fail": 2}
    [path] = fetch(manifest(base), tmp_path, backoff=0)
    assert path.read_bytes() == BODY and len(h.seen) == 3


def test_resumes_a_partial_download_with_a_range_request(tmp_path, server):
    base, h = server
    h.routes["/dumps/f.bin"] = {"body": BODY}
    part = tmp_path / "src" / "f.bin.part"
    part.parent.mkdir()
    part.write_bytes(BODY[:1000])
    [path] = fetch(manifest(base), tmp_path, backoff=0)
    assert path.read_bytes() == BODY
    assert h.seen == [("/dumps/f.bin", "bytes=1000-")]


def test_restarts_when_the_server_ignores_range(tmp_path, server):
    base, h = server
    h.routes["/dumps/f.bin"] = {"body": BODY, "ranges": False}
    part = tmp_path / "src" / "f.bin.part"
    part.parent.mkdir()
    part.write_bytes(BODY[:1000])
    [path] = fetch(manifest(base), tmp_path, backoff=0)
    assert path.read_bytes() == BODY


def test_checksum_mismatch_deletes_the_download_and_stops(tmp_path, server):
    base, h = server
    h.routes["/dumps/f.bin"] = {"body": BODY}
    with pytest.raises(SystemExit, match="checksum mismatch"):
        fetch(manifest(base, sha="0" * 64), tmp_path, backoff=0)
    assert not list((tmp_path / "src").glob("f.bin*"))


def test_http_errors_give_a_clean_message_and_are_not_retried(tmp_path, server):
    base, h = server
    with pytest.raises(SystemExit) as e:
        fetch(manifest(base), tmp_path, backoff=0)
    assert "could not download f.bin" in str(e.value) and "HTTP 404" in str(e.value)
    assert len(h.seen) == 1


def test_falls_back_to_mirrors_in_order(tmp_path, server):
    base, h = server
    h.routes["/m2/dumps/f.bin"] = {"body": BODY}
    [path] = fetch(manifest(base, mirrors=("/m1", "/m2")), tmp_path, backoff=0)
    assert path.read_bytes() == BODY
    assert [p for p, _ in h.seen] == ["/dumps/f.bin", "/m1/dumps/f.bin", "/m2/dumps/f.bin"]
