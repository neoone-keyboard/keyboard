"""The iOS privacy manifests (apps/ios/*/PrivacyInfo.xcprivacy): each target's own, each one a
plist declaring no tracking, no collected data and exactly the required-reason APIs that
target's binary uses, with the reasons in its comments (Apple's "Describing use of required
reason API").

    python3 -I -m unittest discover -s scripts/tests
"""

import plistlib
import unittest
from pathlib import Path

IOS = Path(__file__).resolve().parent.parent.parent / "apps" / "ios"

EXPECTED = {
    "KeyboardApp": {
        "NSPrivacyAccessedAPICategoryUserDefaults": ["CA92.1", "1C8F.1"],
        "NSPrivacyAccessedAPICategoryFileTimestamp": ["C617.1"],
    },
    "KeyboardExtension": {
        "NSPrivacyAccessedAPICategoryUserDefaults": ["CA92.1", "1C8F.1"],
        "NSPrivacyAccessedAPICategoryFileTimestamp": ["C617.1"],
        "NSPrivacyAccessedAPICategorySystemBootTime": ["35F9.1"],
    },
}


def load(target):
    with open(IOS / target / "PrivacyInfo.xcprivacy", "rb") as f:
        return plistlib.load(f)


class PrivacyManifests(unittest.TestCase):
    def test_no_tracking_and_no_collected_data(self):
        for target in EXPECTED:
            with self.subTest(target=target):
                manifest = load(target)
                self.assertIs(manifest["NSPrivacyTracking"], False)
                self.assertEqual(manifest["NSPrivacyTrackingDomains"], [])
                self.assertEqual(manifest["NSPrivacyCollectedDataTypes"], [])

    def test_declares_exactly_the_target_s_api_categories_and_reasons(self):
        for target, expected in EXPECTED.items():
            with self.subTest(target=target):
                declared = {
                    entry["NSPrivacyAccessedAPIType"]: entry["NSPrivacyAccessedAPITypeReasons"]
                    for entry in load(target)["NSPrivacyAccessedAPITypes"]
                }
                self.assertEqual(declared, expected)

    def test_each_target_includes_its_own_manifest(self):
        # project.yml takes each target's whole folder as its sources, so the manifest in that
        # folder is copied into that target's bundle and no other.
        project = (IOS / "project.yml").read_text()
        for target in EXPECTED:
            with self.subTest(target=target):
                self.assertIn(f"      - path: {target}\n", project)


if __name__ == "__main__":
    unittest.main()
