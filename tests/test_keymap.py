"""The shipped key map (hypr/tessera/keymap.json, from tools/gen-keymap.py)."""
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class Keymap(unittest.TestCase):
    def test_no_two_binds_on_one_combination_in_any_order(self):
        # (audit #63: "$MOD + CTRL + S" and "CTRL + $MOD + S" both ran on one press)
        binds = json.loads((ROOT / "hypr/tessera/keymap.json").read_text())
        seen = {}
        for b in binds:
            combo = tuple(sorted(p.strip().upper() for p in b["keys"].split("+")))
            self.assertNotIn(combo, seen, f"{b['id']} and {seen.get(combo)} share {b['keys']}")
            seen[combo] = b["id"]

    def test_the_docs_tables_are_the_keymaps(self):
        # (audit #59: FEATURES.md named keys that did something else)
        doc = (ROOT / "docs/FEATURES.md").read_text()
        start, end = "<!-- keymap: written by tools/gen-keymap.py -->\n", "<!-- keymap end -->\n"
        self.assertIn(start, doc)
        shown = doc.split(start, 1)[1].split(end, 1)[0]
        src = (ROOT / "tools/gen-keymap.py").read_text()
        # (doc_tables and its data, run on the shipped JSON, without writing anything)
        code = src[src.index("DOC_KEYS = ["):src.index("DOC = os.path.join")]
        ns = {}
        exec(code, ns)
        binds = json.loads((ROOT / "hypr/tessera/keymap.json").read_text())
        gestures = json.loads((ROOT / "hypr/tessera/gestures.json").read_text())
        self.assertEqual(shown, ns["doc_tables"](binds, gestures), "run tools/gen-keymap.py")


if __name__ == "__main__":
    unittest.main()
