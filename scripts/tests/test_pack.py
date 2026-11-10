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


class LangRole(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write_lang(self, lang):
        (self.dir / "lang.json").write_text(json.dumps(lang), encoding="utf-8")

    def test_a_coded_script_needs_no_file(self):
        self.assertEqual(pack.lang_problems(self.dir, manifest("deva", 2, lang=False)), [])

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

    def test_write_leaves_a_coded_script_alone(self):
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

    def test_a_coded_script_uses_its_block(self):
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
