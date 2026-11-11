"""scripts/coverage-table.py: the table `make coverage-engine` prints.

    python3 -I -m unittest discover -s scripts/tests
"""

import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "coverage-table.py"
spec = importlib.util.spec_from_file_location("coverage_table", SCRIPT)
coverage_table = importlib.util.module_from_spec(spec)
spec.loader.exec_module(coverage_table)


def export(*files):
    return {
        "data": [
            {
                "files": [
                    {"filename": name, "summary": {"lines": {"count": count, "covered": covered}}}
                    for name, count, covered in files
                ]
            }
        ]
    }


class Table(unittest.TestCase):
    def test_percentages_are_cut_not_rounded(self):
        self.assertEqual(coverage_table.cut(9999, 10000), "99.9")
        self.assertEqual(coverage_table.cut(1, 3), "33.3")
        self.assertEqual(coverage_table.cut(2, 3), "66.6")
        self.assertEqual(coverage_table.cut(5, 5), "100.0")
        self.assertEqual(coverage_table.cut(0, 0), "-")

    def test_least_covered_file_first_and_names_relative(self):
        rows = coverage_table.rows(
            export(("/e/core/src/a.rs", 100, 90), ("/e/core/src/b.rs", 10, 2), ("/x/c.rs", 50, 45)),
            Path("/e"),
        )
        self.assertEqual(
            rows, [("core/src/b.rs", 10, 8), ("core/src/a.rs", 100, 10), ("/x/c.rs", 50, 5)]
        )

    def test_total_counts_every_file(self):
        text = coverage_table.table(export(("/e/a.rs", 100, 90), ("/e/b.rs", 100, 50)), Path("/e"))
        last = text.splitlines()[-1].split()
        self.assertEqual(last, ["total", "200", "60", "70.0%"])


if __name__ == "__main__":
    unittest.main()
