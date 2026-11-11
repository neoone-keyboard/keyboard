from drop_repeats import count, kept, main, runs


def test_runs_read_digits_as_zero_and_a_short_paragraph_is_one_run():
    assert runs("barren land: 259 hectares more", 4) == ["barren land: 0 hectares", "land: 0 hectares more"]
    assert runs("barren land: 12", 4) == ["barren land: 0"]
    assert runs("  ", 4) == []


def _text() -> list[str]:
    # A template repeated for 60 villages, with a name and a number that change, among prose.
    template = [f"village{i} has tap water all year and {i} hectares of barren land\n" for i in range(60)]
    short = [f"barren land: {i} hectares\n" for i in range(60)]
    prose = [
        "the river rises in the hills and runs east to the sea\n",
        "she wrote her first novel at the age of nineteen\n",
        "tap water all year is a phrase this paragraph borrows once before going its own way entirely\n",
    ]
    return template + prose + short


def test_template_paragraphs_go_and_prose_stays():
    lines = _text()
    table = count(lines, 4, bits=16)
    out = list(kept(lines, table, 4, min_count=50, share=0.5))
    assert out == lines[60:63], "the three prose paragraphs, one of which shares a phrase with the template"


def test_a_paragraph_repeated_a_few_times_stays():
    lines = ["one two three four five six\n"] * 10
    assert list(kept(lines, count(lines, 4, bits=16), 4, min_count=50, share=0.5)) == lines


def test_main_writes_the_kept_paragraphs(tmp_path, capsys):
    src, out = tmp_path / "in.txt", tmp_path / "out.txt"
    src.write_text("".join(_text()), encoding="utf-8")
    main([str(src), "--out", str(out)])
    assert out.read_text(encoding="utf-8").count("\n") == 3
    assert "dropped 120 of 123 paragraphs" in capsys.readouterr().err
