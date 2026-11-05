from emoji_data import FLAG_NAMES, catalog_lines, merge_words, parse_annotations, parse_emoji_test, search_lines, search_words, word_lines

EMOJI_TEST = """\
# group: Smileys & Emotion
# subgroup: face-smiling
1F600                                                  ; fully-qualified     # 😀 E1.0 grinning face
263A FE0F                                              ; fully-qualified     # ☺️ E0.6 smiling face
263A                                                   ; unqualified         # ☺ E0.6 smiling face
# subgroup: heart
2764 FE0F                                              ; fully-qualified     # ❤️ E0.6 red heart
2764                                                   ; unqualified         # ❤ E0.6 red heart

# group: People & Body
1F44B                                                  ; fully-qualified     # 👋 E0.6 waving hand
1F44B 1F3FB                                            ; fully-qualified     # 👋🏻 E1.0 waving hand: light skin tone
1F44B 1F3FC                                            ; fully-qualified     # 👋🏼 E1.0 waving hand: medium-light skin tone
1F575 FE0F                                             ; fully-qualified     # 🕵️ E0.7 detective
1F575 1F3FB                                            ; fully-qualified     # 🕵🏻 E2.0 detective: light skin tone
1F9D1 200D 1F91D 200D 1F9D1                            ; fully-qualified     # 🧑‍🤝‍🧑 E12.0 people holding hands
1F9D1 1F3FB 200D 1F91D 200D 1F9D1 1F3FB                ; fully-qualified     # 🧑🏻‍🤝‍🧑🏻 E12.0 people holding hands: light skin tone
1F9D1 1F3FB 200D 1F91D 200D 1F9D1 1F3FC                ; fully-qualified     # 🧑🏻‍🤝‍🧑🏼 E12.1 people holding hands: light skin tone, medium-light skin tone

# group: Component
1F3FB                                                  ; component           # 🏻 E1.0 light skin tone

# group: Food & Drink
1F355                                                  ; fully-qualified     # 🍕 E0.6 pizza
"""

ANNOTATIONS = """<?xml version="1.0" encoding="UTF-8" ?>
<ldml><annotations>
  <annotation cp="😀">face | grin | happy</annotation>
  <annotation cp="😀" type="tts">grinning face</annotation>
  <annotation cp="☺">face | happy | smile</annotation>
  <annotation cp="☺" type="tts">smiling face</annotation>
  <annotation cp="❤">heart | love | up</annotation>
  <annotation cp="❤" type="tts">red heart</annotation>
  <annotation cp="🍕">cheese | Pizza | slice | 3</annotation>
  <annotation cp="🍕" type="tts">pizza</annotation>
  <annotation cp="🏻">light | skin | tone</annotation>
  <annotation cp="👋">hand | wave | heart-eyes | a</annotation>
</annotations></ldml>
"""

ML_ANNOTATIONS = """<?xml version="1.0" encoding="UTF-8" ?>
<ldml><annotations>
  <annotation cp="❤">ചുവന്ന ഹൃദയം | സ്നേഹം</annotation>
</annotations></ldml>
"""


def test_keeps_fully_qualified_emoji_by_group_without_components():
    groups = parse_emoji_test(EMOJI_TEST)
    assert [name for name, _ in groups] == ["Smileys & Emotion", "People & Body", "Food & Drink"]
    assert [e.emoji for e in groups[0][1]] == ["😀", "☺️", "❤️"]
    assert groups[0][1][0].version == "1.0"


def test_attaches_single_tone_variants_to_their_base():
    people = parse_emoji_test(EMOJI_TEST)[1][1]
    assert [e.emoji for e in people] == ["👋", "🕵️", "🧑‍🤝‍🧑"]
    wave, detective, holding = people
    assert wave.tones == ["👋🏻", "👋🏼"]
    assert wave.tone_version == "1.0"
    # The toned form drops the base's FE0F.
    assert detective.tones == ["🕵🏻"]
    assert detective.tone_version == "2.0"
    # Mixed tones (two people, two tones) are left out.
    assert holding.tones == ["🧑🏻‍🤝‍🧑🏻"]


def test_catalog_lines():
    lines = catalog_lines(parse_emoji_test(EMOJI_TEST))
    body = [line for line in lines if not line.startswith("#")]
    assert body == [
        "=Smileys & Emotion", "😀 1.0", "☺️ 0.6", "❤️ 0.6",
        "=People & Body", "👋 0.6 1.0 👋🏻 👋🏼", "🕵️ 0.7 2.0 🕵🏻", "🧑‍🤝‍🧑 12.0 12.0 🧑🏻‍🤝‍🧑🏻",
        "=Food & Drink", "🍕 0.6",
    ]


def test_words_map_to_fully_qualified_emoji_best_first():
    groups = parse_emoji_test(EMOJI_TEST)
    words = parse_annotations(ANNOTATIONS, groups)
    # One-word keywords and names, lowercased; numbers, one letter and hyphenated
    # keywords are left out, as are emoji not in the panel (the skin tone).
    assert words["pizza"] == ["🍕"]
    assert words["love"] == ["❤️"]
    assert "3" not in words and "a" not in words and "heart-eyes" not in words
    assert "light" not in words
    assert "up" not in words, "a grammar word"
    # In the panel's order.
    assert words["happy"] == ["😀", "☺️"]
    assert words["face"] == ["😀", "☺️"]


def test_an_emoji_named_by_the_word_comes_first():
    groups = parse_emoji_test(EMOJI_TEST)
    words = parse_annotations(ANNOTATIONS.replace('cp="☺" type="tts">smiling face', 'cp="☺" type="tts">happy'), groups)
    assert words["happy"] == ["☺️", "😀"]


def test_malayalam_keywords_keep_their_vowel_signs():
    groups = parse_emoji_test(EMOJI_TEST)
    words = parse_annotations(ML_ANNOTATIONS, groups)
    assert words == {"സ്നേഹം": ["❤️"]}


def test_word_lines_are_sorted_by_utf8_bytes():
    lines = word_lines({"zoo": ["🐘"], "love": ["❤️", "😍"], "സ്നേഹം": ["❤️"], "lov": ["x"]})
    body = [line for line in lines if not line.startswith("#")]
    assert body == ["lov\tx", "love\t❤️ 😍", "zoo\t🐘", "സ്നേഹം\t❤️"]
    assert body == sorted(body, key=lambda line: line.encode())


def test_search_words_take_each_word_of_names_and_keywords_without_a_cap():
    groups = parse_emoji_test(EMOJI_TEST)
    words = search_words(ANNOTATIONS, groups)
    # Named first: "red heart" names ❤️, whose keywords also have "heart"; "heart-eyes" is
    # only a keyword of 👋.
    assert words["red"] == (["❤️"], [])
    assert words["heart"] == (["❤️"], ["👋"])
    assert words["eyes"] == ([], ["👋"])
    assert words["face"] == (["😀", "☺️"], [])
    assert words["happy"] == ([], ["😀", "☺️"])
    assert "up" not in words and "a" not in words and "3" not in words
    assert "light" not in words, "not in the panel"


def test_search_words_keep_malayalam_phrases_as_words():
    words = search_words(ML_ANNOTATIONS, parse_emoji_test(EMOJI_TEST))
    assert words == {"ചുവന്ന": ([], ["❤️"]), "ഹൃദയം": ([], ["❤️"]), "സ്നേഹം": ([], ["❤️"])}


def test_search_lines_are_sorted_by_utf8_bytes():
    lines = search_lines({"zoo": (["🐘"], []), "heart": (["❤️"], ["👋"]), "eyes": ([], ["👋"])})
    body = [line for line in lines if not line.startswith("#")]
    assert body == ["eyes\t| 👋", "heart\t❤️ | 👋", "zoo\t🐘"]


FLAGS_TEST = """\
# group: Flags
# subgroup: country-flag
1F1EE 1F1F3                                            ; fully-qualified     # 🇮🇳 E2.0 flag: India
1F1FA 1F1F8                                            ; fully-qualified     # 🇺🇸 E2.0 flag: United States
"""

DERIVED_FLAGS = """<?xml version="1.0" encoding="UTF-8" ?>
<ldml><annotations>
<annotation cp="🇮🇳">flag</annotation>
<annotation cp="🇮🇳" type="tts">flag: India</annotation>
<annotation cp="🇺🇸">flag</annotation>
<annotation cp="🇺🇸" type="tts">flag: United States</annotation>
</annotations></ldml>
"""


def test_a_country_suggests_its_flag():
    words = parse_annotations(DERIVED_FLAGS, parse_emoji_test(FLAGS_TEST))
    assert words["india"] == ["🇮🇳"]
    # A name of two words is no one word the keyboard has typed.
    assert "united states" not in words and "states" not in words


def test_an_emoji_the_word_names_comes_first_across_files():
    merged = merge_words([{"australia": [(False, "🐨"), (False, "🌏")]}, {"australia": [(True, "🇦🇺")]}])
    assert merged["australia"] == ["🇦🇺", "🐨", "🌏"]


def test_everyday_flag_names_are_words_and_flags():
    from emoji_data import _is_flag, _is_word
    assert all(_is_word(word) and _is_flag(flag) for word, flag in FLAG_NAMES.items())
    assert "us" not in FLAG_NAMES
