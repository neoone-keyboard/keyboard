import io
import tarfile

from scowl_words import words


def add(tar, name, text):
    data = text.encode("latin-1")
    info = tarfile.TarInfo(name)
    info.size = len(data)
    tar.addfile(info, io.BytesIO(data))


def test_reads_sizes_up_to_70_and_skips_australian(tmp_path):
    path = tmp_path / "scowl.tar.gz"
    with tarfile.open(path, "w:gz") as tar:
        add(tar, "scowl/final/english-words.10", "the\ncat\n")
        add(tar, "scowl/final/american-words.70", "color\ncaf\xe9\n")
        add(tar, "scowl/final/english-words.80", "zyzzyva\n")
        add(tar, "scowl/final/australian-words.10", "arvo\n")
        add(tar, "scowl/final/english-contractions.35", "don't\n")
    assert words(str(path)) == {"the", "cat", "color", "don't"}
