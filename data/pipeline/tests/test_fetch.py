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
