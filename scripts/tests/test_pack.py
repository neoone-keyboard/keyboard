"""scripts/pack.py: the `lang` role (a script's rules as data, engine 4).

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


if __name__ == "__main__":
    unittest.main()
