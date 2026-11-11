"""scripts/pack.py: the `lang` role (a script's rules as data, engine 4) and the layout check.

    python3 -I -m unittest discover -s scripts/tests
"""

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "pack.py"
spec = importlib.util.spec_from_file_location("pack", SCRIPT)
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)

LANG = {"id": "zz-test", "script": "gujr", "block": ["0A80", "0AFF"]}


def manifest(script="gujr", min_engine=4, lang=True):
    m = {"id": "zz-test", "script": script, "minEngine": min_engine, "files": {"lexicon": "zz.fst"}}
    if lang:
        m["files"]["lang"] = "lang.json"
    return m


class RomanisationRole(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.manifest = {"id": "zz-test", "script": "gujr", "files": {"romanisation": "romanisation.json"}}

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self, tables):
        (self.dir / "romanisation.json").write_text(json.dumps(tables), encoding="utf-8")
        return pack.romanisation_problems(self.dir, self.manifest)

    def test_the_tables_say_which_spellings_are_the_same(self):
        self.assertIn("no looseSpelling", self.problems({"format": 1})[0])
        self.assertEqual(self.problems({"format": 1, "looseSpelling": {}}), [])
        self.assertEqual(self.problems({"format": 1, "looseSpelling": {"collapse": "a"}}), [])

    def test_no_romanisation_is_not_a_problem(self):
        self.assertEqual(pack.romanisation_problems(self.dir, {"files": {}}), [])
        self.assertEqual(pack.romanisation_problems(self.dir, self.manifest), [])  # listed, not written yet

    def test_every_committed_pack_says(self):
        packs = sorted((pack.ROOT / "packs").glob("*.neopack"))
        self.assertTrue(packs)
        for folder in packs:
            manifest = json.loads((folder / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(pack.romanisation_problems(folder, manifest), [], folder.name)


class EmojiRoles(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name) / "zz.neopack"
        self.dir.mkdir()
        self.root = pack.ROOT
        pack.ROOT = Path(self.tmp.name) / "repo"
        self.source = pack.ROOT / "data/langs/zz-test"
        self.source.mkdir(parents=True)

    def tearDown(self):
        pack.ROOT = self.root
        self.tmp.cleanup()

    def test_a_language_without_emoji_words_has_no_roles(self):
        m = manifest(lang=False)
        pack.add_copied(self.dir, m)
        self.assertEqual(m, manifest(lang=False))
        self.assertEqual(pack.copied_problems(self.dir, m), [])

    def test_the_pack_carries_its_languages_emoji_words(self):
        (self.source / "emoji-words.txt").write_text("പൂച്ച\t🐈\n", encoding="utf-8")
        (self.source / "emoji-search.txt").write_text("പൂച്ച\t🐈\n", encoding="utf-8")
        m = manifest(lang=False)
        self.assertIn("the emojiWords role", pack.copied_problems(self.dir, m)[0])
        self.assertEqual(len(pack.copied_problems(self.dir, m)), 2)
        pack.add_copied(self.dir, m)
        self.assertEqual(m["files"]["emojiWords"], "emoji-words.txt")
        self.assertEqual(m["files"]["emojiSearch"], "emoji-search.txt")
        self.assertEqual(m["minEngine"], 4, "the roles don't raise it")
        self.assertEqual((self.dir / "emoji-words.txt").read_text(encoding="utf-8"), "പൂച്ച\t🐈\n")
        self.assertEqual(pack.copied_problems(self.dir, m), [])
        (self.dir / "emoji-search.txt").write_text("other\n", encoding="utf-8")
        self.assertIn("emoji-search.txt differs", pack.copied_problems(self.dir, m)[0])


class LangRole(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write_lang(self, lang):
        (self.dir / "lang.json").write_text(json.dumps(lang), encoding="utf-8")

    def test_an_embedded_script_needs_no_file(self):
        self.assertEqual(pack.lang_problems(self.dir, manifest("deva", 2, lang=False)), [])

    def test_a_pack_carries_its_languages_file_and_an_embedded_script_keeps_its_engine(self):
        # data/langs/<id>/lang.json exists: the pack must carry it, whatever its script.
        root = pack.ROOT
        self.addCleanup(setattr, pack, "ROOT", root)
        pack.ROOT = self.dir / "repo"
        source = pack.ROOT / "data/langs/zz-test/lang.json"
        source.parent.mkdir(parents=True)
        source.write_text(json.dumps({**LANG, "script": "deva", "block": ["0900", "097F"]}), encoding="utf-8")
        m = manifest("deva", 2, lang=False)
        self.assertIn("doesn't carry data/langs/zz-test/lang.json", pack.lang_problems(self.dir, m)[0])
        pack.add_lang(self.dir, m)
        self.assertEqual(m, manifest("deva", 2))  # the role, and minEngine as it was
        self.assertEqual((self.dir / "lang.json").read_bytes(), source.read_bytes())
        self.assertEqual(pack.lang_problems(self.dir, m), [])
        self.write_lang({**LANG, "script": "deva"})
        self.assertIn("differs from data/langs/zz-test/lang.json", pack.lang_problems(self.dir, m)[0])

    def test_a_fixture_pack_with_a_real_languages_id_stands_alone(self):
        # engine/core/tests/fixtures/gu.neopack has its own lang.json; data/langs/gu/lang.json
        # is the Gujarati pack's and must not be compared with it or copied over it.
        root = pack.ROOT
        self.addCleanup(setattr, pack, "ROOT", root)
        pack.ROOT = self.dir
        source = pack.ROOT / "data/langs/zz-test/lang.json"
        source.parent.mkdir(parents=True)
        source.write_text(json.dumps({**LANG, "notes": "the real language's"}), encoding="utf-8")
        real = pack.ROOT / "packs/zz-test.neopack"
        fixture = pack.ROOT / "engine/core/tests/fixtures/zz-test.neopack"
        for folder in (real, fixture):
            folder.mkdir(parents=True)
            (folder / "lang.json").write_text(json.dumps(LANG), encoding="utf-8")
        m = manifest("gujr", 4)
        self.assertFalse(pack.is_fixture(real))
        self.assertIn("differs from data/langs/zz-test/lang.json", pack.lang_problems(real, m)[0])
        self.assertTrue(pack.is_fixture(fixture))
        self.assertEqual(pack.lang_problems(fixture, m), [])
        pack.add_lang(fixture, m)
        self.assertEqual(json.loads((fixture / "lang.json").read_text(encoding="utf-8")), LANG)

    def test_the_newer_fields_need_engine_5_except_for_malayalam(self):
        joins = {**LANG, "finalPairs": [["0ABE", "0ACD"]]}
        self.write_lang(joins)
        problems = pack.lang_problems(self.dir, manifest())
        self.assertIn("minEngine must be at least 5", problems[0])
        self.assertIn("uses finalPairs", problems[0])
        self.assertEqual(pack.lang_problems(self.dir, manifest(min_engine=5)), [])
        m = manifest(min_engine=2)
        pack.add_lang(self.dir, m)
        self.assertEqual(m["minEngine"], 5)
        # Engines before 5 have Malayalam's rules in code and ignore the fields.
        self.write_lang({**joins, "script": "mlym", "block": ["0D00", "0D7F"]})
        self.assertEqual(pack.lang_problems(self.dir, manifest("mlym", 1)), [])
        # Devanagari's are in code there too, but not what the new fields say.
        self.write_lang({**joins, "script": "deva", "block": ["0900", "097F"]})
        self.assertIn("at least 5", pack.lang_problems(self.dir, manifest("deva", 2))[0])

    def test_the_joiner_fields_need_engine_6_except_for_malayalam(self):
        self.write_lang({**LANG, "virama": "0ACD", "joiners": "afterVirama"})
        problems = pack.lang_problems(self.dir, manifest(min_engine=5))
        self.assertIn("minEngine must be at least 6", problems[0])
        self.assertIn("uses joiners", problems[0])
        self.assertEqual(pack.lang_problems(self.dir, manifest(min_engine=6)), [])
        m = manifest(min_engine=4)
        pack.add_lang(self.dir, m)
        self.assertEqual(m["minEngine"], 6)
        self.write_lang({**LANG, "script": "mlym", "block": ["0D00", "0D7F"], "zwjForms": [["0D28", "0D7B"]]})
        self.assertEqual(pack.lang_problems(self.dir, manifest("mlym", 1)), [])

    def test_a_nasal_ending_of_several_code_points_needs_engine_6(self):
        self.write_lang({**LANG, "suffixJoins": {"nasal": ["0AAE", "0AAE"]}})
        self.assertEqual(pack.lang_problems(self.dir, manifest(min_engine=5)), [])
        self.write_lang({**LANG, "suffixJoins": {"nasal": ["0AAE 0ACD", "0AAE"]}})
        self.assertIn("at least 6", pack.lang_problems(self.dir, manifest(min_engine=5))[0])
        self.assertEqual(pack.lang_problems(self.dir, manifest(min_engine=6)), [])

    def test_another_script_needs_the_file_and_engine_4(self):
        problems = pack.lang_problems(self.dir, manifest(lang=False))
        self.assertIn("needs its lang.json", problems[0])
        self.write_lang(LANG)
        self.assertEqual(pack.lang_problems(self.dir, manifest()), [])
        problems = pack.lang_problems(self.dir, manifest(min_engine=3))
        self.assertIn("minEngine must be at least 4", problems[0])

    def test_the_file_must_define_the_manifest_script(self):
        self.write_lang({**LANG, "script": "taml"})
        self.assertIn("defines script 'taml'", pack.lang_problems(self.dir, manifest())[0])
        self.write_lang({"script": "gujr"})
        self.assertIn("no characters", pack.lang_problems(self.dir, manifest())[0])
        self.assertIn("not in the pack", pack.lang_problems(self.dir / "nowhere", manifest())[-1])

    def test_write_raises_min_engine_for_another_script(self):
        m = manifest(min_engine=2, lang=False)
        pack.add_lang(self.dir, m)
        self.assertEqual(m["minEngine"], 4)
        self.assertEqual(m["files"]["lang"], "lang.json")

    def test_write_leaves_an_embedded_script_without_a_file_alone(self):
        m = manifest("beng", 2, lang=False)
        pack.add_lang(self.dir, m)
        self.assertEqual(m, manifest("beng", 2, lang=False))


LAYOUT = {
    "format": 1,
    "spaceBar": "ગુજરાતી",
    "languageKey": "ગુ",
    "pages": [{"id": "letters", "label": "કખ", "rows": [["ક", "ખ", "ક્ષ"], ["।", ".", "?"]]}],
    "longPress": {"ક": ["ક્ષ"]},
    "vowelSignStrip": ["ા", "્"],
}


class Layout(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        (self.dir / "lang.json").write_text(json.dumps(LANG), encoding="utf-8")
        self.manifest = manifest()
        self.manifest["files"]["layout"] = "layout.json"

    def tearDown(self):
        self.tmp.cleanup()

    def problems(self, **changes):
        (self.dir / "layout.json").write_text(json.dumps({**LAYOUT, **changes}), encoding="utf-8")
        return pack.layout_problems(self.dir, self.manifest)

    def test_a_good_layout_passes_with_punctuation_and_conjunct_keys(self):
        self.assertEqual(self.problems(), [])

    def test_no_layout_role_or_file_is_not_a_layout_problem(self):
        self.assertEqual(pack.layout_problems(self.dir, self.manifest), [])  # not written yet
        self.assertEqual(pack.layout_problems(self.dir, manifest()), [])

    def test_what_the_app_refuses(self):
        self.assertIn("format is 2", self.problems(format=2)[0])
        self.assertIn("no pages", self.problems(pages=[])[0])
        page = {"id": "letters", "label": "કખ", "rows": [["ક", ""]]}
        self.assertIn("empty key", self.problems(pages=[page], longPress={})[0])
        self.assertIn("'ાિ' is not one character", self.problems(vowelSignStrip=["ાિ"])[0])

    def test_keys_options_and_signs_are_in_the_packs_script(self):
        page = {"id": "letters", "label": "કખ", "rows": [["ક", "क", "a"]]}
        problems = self.problems(pages=[page])
        self.assertEqual(len(problems), 2)
        self.assertIn("क (U+0915), not of script gujr", problems[0])
        self.assertIn("U+0061", problems[1])
        self.assertIn("not of script", self.problems(longPress={"ક": ["ক"]})[0])
        self.assertIn("not of script", self.problems(vowelSignStrip=["া"])[0])

    def test_an_embedded_script_uses_its_block(self):
        (self.dir / "lang.json").unlink()
        m = {**self.manifest, "script": "beng", "files": {"layout": "layout.json"}}
        (self.dir / "layout.json").write_text(json.dumps(LAYOUT), encoding="utf-8")
        self.assertTrue(all("not of script beng" in p for p in pack.layout_problems(self.dir, m)))
        self.assertTrue(pack.layout_problems(self.dir, m))

    def test_long_press_keys_are_on_a_page_and_names_are_there(self):
        self.assertIn("on no page", self.problems(longPress={"ગ": ["ઘ"]})[0])
        self.assertIn("spaceBar is missing", self.problems(spaceBar="")[0])
        page = {"id": "letters", "rows": [["ક"]]}
        self.assertIn("no label", self.problems(pages=[page], longPress={})[0])

    def test_every_committed_layout_passes(self):
        root = SCRIPT.parent.parent
        packs = sorted(root.glob("packs/*.neopack")) + [root / "engine/core/tests/fixtures/gu.neopack"]
        for folder in packs:
            m = json.loads((folder / "manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(pack.layout_problems(folder, m), [], folder.name)


if __name__ == "__main__":
    unittest.main()
