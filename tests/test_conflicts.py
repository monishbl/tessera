"""shell/assets/scripts/conflicts.py's decisions over fake processes (no real
/proc read): a second clipboard recorder is named whether or not it has a
--type (audit #706)."""
import importlib.util
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("tessera_conflicts", ROOT / "shell/assets/scripts/conflicts.py")
C = importlib.util.module_from_spec(spec)
spec.loader.exec_module(C)


def found_for(procs):
    """duplicates() for {pid: argv} wl-paste processes, all in this session"""
    out = []
    with mock.patch.object(C, "processes", lambda: list(procs)), mock.patch.object(C, "comm", lambda p: "wl-paste"), \
            mock.patch.object(C, "cmdline", lambda p: procs[p]), mock.patch.object(C, "ours", lambda p: True):
        C.duplicates(out)
    return [f["id"] for f in out]


TESSERAS = ["wl-paste", "--watch", "cliphist", "store"]


class Clipboard(unittest.TestCase):
    def test_tesseras_own_recorder_alone_is_fine(self):
        self.assertEqual(found_for({1: TESSERAS}), [])

    def test_typed_ones_one_of_each_kind_are_fine(self):
        self.assertEqual(found_for({1: ["wl-paste", "--type", "text", "--watch", "cliphist", "store"],
                                    2: ["wl-paste", "--type", "image", "--watch", "cliphist", "store"]}), [])

    def test_an_untyped_one_next_to_typed_ones_is_named(self):
        self.assertEqual(found_for({1: TESSERAS, 2: ["wl-paste", "--type", "text", "--watch", "cliphist", "store"]}), ["cliphist"])

    def test_two_untyped_ones_are_named(self):
        self.assertEqual(found_for({1: TESSERAS, 2: TESSERAS}), ["cliphist"])


if __name__ == "__main__":
    unittest.main()
