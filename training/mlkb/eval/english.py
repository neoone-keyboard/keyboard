"""How often correct English gets replaced: every word of every sentence is a check.

Each word is checked the moment before space is typed: the engine sees the
sentence so far, ending in the bare word. Rules:

- Punctuation (`PUNCT`) is stripped from both ends of the word, since the
  keyboard sees the word before the comma or full stop is typed. Earlier
  words keep theirs, as they would in the text field. A token that is only
  punctuation (`-`, `&`) is not a word and is skipped.
- A middle slot that is the bare word, or of kind `verbatim`, is fine.
- For the first word of a sentence, a middle slot that differs only in case
  is fine too (`Ok` -> `OK`): the keyboard capitalises sentence starts on its
  own, so case there says nothing about whether the word was understood.
  Anywhere else a case change counts (`iPhone` -> `IPhone` is wrong).
"""

PUNCT = ".,!?;:\"()…"


def wrong_replacements(engine, sentences: list[str]) -> tuple[int, int]:
    """Returns (middle slots that would replace a correct word, words checked)."""
    wrong = total = 0
    for sentence in sentences:
        _, wrong_here, total_here = check(engine, sentence)
        wrong += wrong_here
        total += total_here
    return wrong, total


def check(engine, sentence: str) -> tuple[list[tuple[str, str]], int, int]:
    """Checks one sentence; returns its replacements as (typed, middle) pairs, the count, and words checked."""
    words = sentence.split()
    replaced = []
    total = 0
    for i, word in enumerate(words):
        bare = word.strip(PUNCT)
        if not bare:
            continue
        context = " ".join([*words[:i], bare])
        bar = engine.suggest(context, "latin", True)
        total += 1
        if not bar:
            continue
        text, kind, _ = bar[0]
        if text == bare or kind == "verbatim":
            continue
        if i == 0 and text.casefold() == bare.casefold():
            continue
        replaced.append((bare, text))
    return replaced, len(replaced), total
