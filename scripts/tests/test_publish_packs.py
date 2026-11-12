"""scripts/publish-packs.py: the pack index's entries kept for older engines, and the packs
left out of the site.

    python3 -I -m unittest discover -s scripts/tests
"""

import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "publish-packs.py"
spec = importlib.util.spec_from_file_location("publish_packs", SCRIPT)
pp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pp)

DOWNLOADS = "https://packs.test/packs/"


def entry(pid="hi", version="2026.11.10", min_engine=3, **extra):
    e = {"id": pid, "name": pid.upper(), "nativeName": pid, "version": version, "script": "deva",
         "minEngine": min_engine, "size": 10, "sha256": "a" * 64,
         "url": f"{DOWNLOADS}{pid}-{version}.neopack"}
    e.update(extra)
    return e


def releases(*entries):
    return [{"entry": e} for e in entries]


def kept(*entries):
    return {"about": "…", "packs": list(entries)}


class Versions(unittest.TestCase):
    def test_as_pack_version_orders_them(self):
        self.assertEqual(pp.compare_versions("2026.11.6", "2026.11.06"), 0)
        self.assertEqual(pp.compare_versions("2026.11.06", "2026.11.06.1"), -1)
        self.assertEqual(pp.compare_versions("2026.11.10", "2026.11.9"), 1)
        self.assertEqual(pp.compare_versions("2026.11.b", "2026.11.a"), 1)


class Kept(unittest.TestCase):
    def test_an_older_entry_for_a_lower_engine_is_kept(self):
        old = entry(version="2026.11.06", min_engine=2, englishFingerprint="00ff")
        got = pp.kept_entries(kept(old), releases(entry()), DOWNLOADS)
        self.assertEqual(got, [old])

    def test_one_for_each_older_engine_is_kept(self):
        two = entry(version="2026.11.06", min_engine=2)
        one = entry(version="2026.11.01", min_engine=1)
        self.assertEqual(len(pp.kept_entries(kept(two, one), releases(entry()), DOWNLOADS)), 2)

    def test_refusals(self):
        current = releases(entry())
        cases = {
            "no pack": entry(pid="as", version="2026.11.06", min_engine=2),
            "same engine": entry(version="2026.11.06", min_engine=3),
            "newer": entry(version="2026.11.11", min_engine=2),
            "same version": entry(version="2026.11.10", min_engine=2),
            "another url": entry(version="2026.11.06", min_engine=2, url="https://elsewhere.test/hi.neopack"),
            "bad digest": entry(version="2026.11.06", min_engine=2, sha256="ABC"),
            "unknown field": entry(version="2026.11.06", min_engine=2, note="x"),
            "zero size": entry(version="2026.11.06", min_engine=2, size=0),
        }
        for why, bad in cases.items():
            with self.subTest(why), self.assertRaises(SystemExit):
                pp.kept_entries(kept(bad), current, DOWNLOADS)
        missing = entry(version="2026.11.06", min_engine=2)
        del missing["sha256"]
        with self.assertRaises(SystemExit):
            pp.kept_entries(kept(missing), current, DOWNLOADS)

    def test_one_no_app_picks_is_refused(self):
        # Engine 2 apps pick 2026.11.08; 2026.11.06 (engine 2) and 2026.11.07 (engine 2) are dead.
        newer = entry(version="2026.11.08", min_engine=2)
        for dead in (entry(version="2026.11.06", min_engine=2), entry(version="2026.11.07", min_engine=2)):
            with self.subTest(dead["version"]), self.assertRaises(SystemExit):
                pp.kept_entries(kept(newer, dead), releases(entry()), DOWNLOADS)
        # An engine 1 entry older than the engine 2 one is still the newest engine 1 reads.
        one = entry(version="2026.11.01", min_engine=1)
        self.assertEqual(len(pp.kept_entries(kept(newer, one), releases(entry()), DOWNLOADS)), 2)


class Unlisted(unittest.TestCase):
    def test_ids_of_packs_in_packs(self):
        self.assertEqual(pp.unlisted_ids({"about": "…", "ids": ["mr", "bn"]}, ["bn", "hi", "mr"]), ["mr", "bn"])
        self.assertEqual(pp.unlisted_ids({}, ["hi"]), [])

    def test_refusals(self):
        cases = {"no pack": ["mr"], "twice": ["hi", "hi"], "not ids": [{"id": "hi"}], "not a list": "hi"}
        for why, ids in cases.items():
            with self.subTest(why), self.assertRaises(SystemExit):
                pp.unlisted_ids({"ids": ids}, ["hi"])

    def test_an_unlisted_pack_is_checked_but_not_built_or_listed(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            manifests = []
            for pid in ("hi", "mr"):
                pack = root / f"{pid}.neopack"
                pack.mkdir()
                (pack / "layout.json").write_text("{}")
                manifest = {"id": pid, "name": pid.upper(), "version": "1", "script": "deva", "minEngine": 3,
                            "files": {"layout": "layout.json"}}
                manifest["sha256"] = pp.load_pack_module().listed(pack, manifest)
                (pack / "manifest.json").write_text(json.dumps(manifest))
                manifests.append((pack, manifest))
            out = root / "out"
            out.mkdir()
            module = pp.load_pack_module()
            got = pp.build_packs(module, out, DOWNLOADS, manifests, ["mr"])
            self.assertEqual([r["entry"]["id"] for r in got], ["hi"])
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["hi-1.neopack"])
            self.assertEqual([e["id"] for e in json.loads(pp.index_json(got, []))["packs"]], ["hi"])
            # Unlisted, its checksums are still checked.
            (root / "mr.neopack" / "layout.json").write_text("{ }")
            with self.assertRaises(SystemExit):
                pp.build_packs(module, out, DOWNLOADS, manifests, ["mr"])
            # And a kept entry of an unlisted id has no pack to be older than.
            with self.assertRaises(SystemExit):
                pp.kept_entries(kept(entry(pid="mr", version="0", min_engine=2)), got, DOWNLOADS)


class Index(unittest.TestCase):
    def test_by_id_and_newest_first(self):
        old = entry(version="2026.11.06", min_engine=2)
        got = pp.index_entries(releases(entry(pid="ml", version="1", min_engine=1), entry(),
                                        entry(pid="as", version="2", min_engine=2)), [old])
        self.assertEqual([(e["id"], e["version"]) for e in got],
                         [("as", "2"), ("hi", "2026.11.10"), ("hi", "2026.11.06"), ("ml", "1")])

    def test_a_kept_entry_published_with_other_bytes_is_refused(self):
        import json
        import tempfile
        with tempfile.TemporaryDirectory() as folder:
            index = Path(folder) / "index.json"
            index.write_text(json.dumps({"format": 1, "packs": [entry(version="2026.11.06", min_engine=2)]}))
            same = entry(version="2026.11.06", min_engine=2)
            other = entry(version="2026.11.06", min_engine=2, sha256="b" * 64)
            self.assertEqual(pp.republished(index, [same]), [])
            self.assertEqual(pp.republished(index, [other]), ["hi 2026.11.06"])


class Beta(unittest.TestCase):
    """Beta packs are listed in the index's `beta` section, which builds before it never read
    (they decode only `format` and `packs`). Ids in `alsoInPacks` stay in `packs` too, for the
    builds that listed them before there was a beta section."""

    def test_ids_of_listed_packs(self):
        raw = {"about": "…", "ids": ["ta", "kn"], "alsoInPacks": ["ta"]}
        self.assertEqual(pp.beta_ids(raw, ["hi", "kn", "ta"], []), (["ta", "kn"], ["ta"]))
        self.assertEqual(pp.beta_ids({}, ["hi"], []), ([], []))

    def test_refusals(self):
        cases = {
            "no pack": ({"ids": ["kn"]}, []),
            "twice": ({"ids": ["ta", "ta"]}, []),
            "not a list": ({"ids": "ta"}, []),
            "unlisted too": ({"ids": ["ta"]}, ["ta"]),
            "also not beta": ({"ids": ["ta"], "alsoInPacks": ["hi"]}, []),
        }
        for why, (raw, unlisted) in cases.items():
            with self.subTest(why), self.assertRaises(SystemExit):
                pp.beta_ids(raw, ["hi", "ta"], unlisted)

    def test_index_with_beta_entries(self):
        import json
        full = entry(pid="ml", version="1", min_engine=1)
        old_hi = entry(version="2026.11.06", min_engine=2)
        ta, kn = entry(pid="ta", min_engine=6), entry(pid="kn", min_engine=6)
        got = json.loads(pp.index_json(releases(full, entry(), ta, kn), [old_hi], beta=["kn", "ta"], also=["ta"]))
        self.assertEqual(list(got), ["format", "packs", "beta"])
        self.assertEqual(got["format"], 1)
        self.assertEqual([(e["id"], e["version"]) for e in got["packs"]],
                         [("hi", "2026.11.10"), ("hi", "2026.11.06"), ("ml", "1"), ("ta", "2026.11.10")])
        self.assertEqual([e["id"] for e in got["beta"]], ["kn", "ta"])
        # The same entry in both sections, byte for byte.
        self.assertEqual(got["packs"][-1], got["beta"][-1])

    def test_index_without_beta_entries_is_the_old_format(self):
        import json
        got = json.loads(pp.index_json(releases(entry(pid="ml", version="1", min_engine=1), entry()), []))
        self.assertEqual(list(got), ["format", "packs"])

    def test_old_builds_see_what_they_saw_before(self):
        """What an old build reads (`format` and `packs`) is the index without the beta section,
        with the beta packs it listed before still in it."""
        import json
        rel = releases(entry(pid="ml", version="1", min_engine=1), entry(pid="as", min_engine=6),
                       entry(pid="kn", min_engine=6))
        before = json.loads(pp.index_json(rel[:2], []))
        after = json.loads(pp.index_json(rel, [], beta=["as", "kn"], also=["as"]))
        self.assertEqual({k: after[k] for k in ("format", "packs")}, before)

    def test_credits_mark_beta_packs(self):
        rel = [{"entry": entry(pid="ml", version="1", min_engine=1), "manifest": {"name": "ML", "id": "ml"}},
               {"entry": entry(pid="kn", min_engine=6), "manifest": {"name": "KN", "id": "kn"}}]
        english = {"url": DOWNLOADS + "english-base.zip", "size": 10}
        orig = pp.english_credits
        pp.english_credits = lambda: []
        try:
            page = pp.credits_page(rel, [], english, beta=["kn"])
        finally:
            pp.english_credits = orig
        self.assertIn("KN pack 2026.11.10</a>, beta", page)
        self.assertNotIn("ML pack 1</a>, beta", page)


class Listing(unittest.TestCase):
    """The user's decision of which packs are full languages, beta or unlisted."""

    def test_the_decision(self):
        self.assertEqual(pp.LISTING["full"], ["hi", "ml"])
        self.assertEqual(pp.LISTING["beta"], ["as", "gu", "kn", "ne", "pa", "ta", "te"])
        self.assertEqual(pp.LISTING["unlisted"], ["bho", "bn", "ja", "mai", "mr", "or", "si", "ur"])
        self.assertEqual(pp.LISTING["alsoInPacks"], ["as", "ta"])

    def test_the_site_src_files_match_it(self):
        ids = [p.name.removesuffix(".neopack") for p in pp.PACKS.glob("*.neopack")]
        unlisted = pp.unlisted_ids(pp.json.loads(pp.UNLISTED.read_text()), ids)
        beta, also = pp.beta_ids(pp.json.loads(pp.BETA.read_text()), ids, unlisted)
        self.assertEqual(pp.listing_problems(ids, unlisted, beta, also), [])

    def test_a_different_listing_is_named(self):
        ids = ["as", "hi", "kn", "ml", "mr"]
        self.assertEqual(pp.listing_problems(ids, ["mr"], ["as", "kn"], ["as"], decision={
            "full": ["hi", "ml"], "beta": ["as", "kn"], "unlisted": ["mr"], "alsoInPacks": ["as"]}), [])
        got = pp.listing_problems(ids, [], ["as", "kn", "mr"], ["as"], decision={
            "full": ["hi", "ml"], "beta": ["as", "kn"], "unlisted": ["mr"], "alsoInPacks": ["as"]})
        self.assertTrue(any("mr" in p for p in got), got)
        got = pp.listing_problems(ids, ["mr"], ["kn"], [], decision={
            "full": ["hi", "ml"], "beta": ["as", "kn"], "unlisted": ["mr"], "alsoInPacks": ["as"]})
        self.assertTrue(any("as" in p for p in got), got)


if __name__ == "__main__":
    unittest.main()
