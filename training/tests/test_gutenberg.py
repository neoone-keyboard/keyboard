from mlkb.lm.gutenberg import authors_dead_before, dialogue_lines, strip_boilerplate


def test_every_author_must_have_died_before_the_cutoff():
    assert authors_dead_before("Austen, Jane, 1775-1817", 1956)
    assert not authors_dead_before("Orwell, George, 1903-1950; Huxley, Aldous, 1894-1963", 1956)
    assert not authors_dead_before("Anonymous", 1956)  # no dates: status unknown, skip it
    assert not authors_dead_before("Doyle, Arthur Conan, 1859-", 1956)  # no death year


def test_boilerplate_is_stripped_and_copyrighted_books_are_refused():
    text = ("Header\n*** START OF THE PROJECT GUTENBERG EBOOK X ***\nBody one.\n\nBody two.\n"
            "*** END OF THE PROJECT GUTENBERG EBOOK X ***\nLicence")
    assert strip_boilerplate(text) == "Body one.\n\nBody two."
    assert strip_boilerplate("This eBook is copyrighted.\n*** START OF THE PROJECT GUTENBERG EBOOK X ***\nBody") is None


def test_dialogue_lines_are_the_quoted_speech():
    assert dialogue_lines('He said, "Come here now." She left. "Fine," he said.') == ["Come here now.", "Fine,"]
