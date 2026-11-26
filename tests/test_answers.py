"""answers.py: Spotlight's plain-language answers (sums in words and
percentages, dates, time zones, units, currency), at a fixed moment and with
fixed rates so nothing depends on today or the network."""
import datetime as dt
import importlib.util
import unittest
from pathlib import Path
from zoneinfo import ZoneInfo

SCRIPT = Path(__file__).resolve().parent.parent / "shell/assets/scripts/answers.py"
spec = importlib.util.spec_from_file_location("answers", SCRIPT)
an = importlib.util.module_from_spec(spec)
spec.loader.exec_module(an)

NOW = dt.datetime(2026, 11, 1, 15, 0, tzinfo=ZoneInfo("Asia/Kolkata"))
RATES = lambda: {"EUR": 1.0, "USD": 1.08, "INR": 90.0, "GBP": 0.86}  # noqa: E731


def ask(q):
    return an.answer(q, NOW, RATES)


class Maths(unittest.TestCase):
    def test_percentages(self):
        self.assertEqual(ask("18% of 2450")["title"], "441")
        self.assertEqual(ask("2450 plus 18%")["title"], "2,891")
        self.assertEqual(ask("200 minus 10 percent")["title"], "180")
        self.assertEqual(ask("120 is what percent of 480")["title"], "25%")

    def test_words(self):
        self.assertEqual(ask("twelve times four")["title"], "48")
        self.assertEqual(ask("twelve times four")["subtitle"], "12 × 4")
        self.assertEqual(ask("twenty-five plus seventeen")["title"], "42")
        self.assertEqual(ask("half of 300")["title"], "150")
        self.assertEqual(ask("3 dozen")["title"], "36")
        self.assertEqual(ask("square root of 144")["title"], "12")
        self.assertEqual(ask("2 to the power of 10")["copy"], "1024")

    def test_not_a_sum(self):
        for q in ("terminal", "hello world", "calc", "firefox", "set a timer"):
            self.assertEqual(ask(q), {}, q)


class Dates(unittest.TestCase):
    def test_until(self):
        r = ask("days until christmas")
        self.assertEqual(r["title"], "54 days")
        self.assertIn("25 December 2026", r["subtitle"])
        self.assertEqual(ask("days until 1 jan")["title"], "61 days")

    def test_from_today_and_ago(self):
        self.assertEqual(ask("3 weeks from today")["title"], "Sunday 22 November 2026")
        self.assertEqual(ask("10 days ago")["title"], "Thursday 22 October 2026")
        self.assertEqual(ask("in 2 months")["title"], "Friday 1 January 2027")

    def test_weekday_and_age(self):
        self.assertEqual(ask("what day is 14 feb")["title"], "Sunday")
        self.assertEqual(ask("how old is someone born 1998")["title"], "27 or 28")


class Zones(unittest.TestCase):
    def test_time_in(self):
        r = ask("time in tokyo")
        self.assertEqual(r["title"], "6:30 PM")
        self.assertIn("3 h 30 min ahead", r["subtitle"])

    def test_between_cities(self):
        self.assertEqual(ask("5pm london in delhi")["title"], "10:30 PM")
        self.assertEqual(ask("11pm delhi in new york")["title"], "12:30 PM")  # (US clocks went back that morning)


class Units(unittest.TestCase):
    def test_units(self):
        self.assertEqual(ask("5 km in miles")["title"], "3.1069 miles")
        self.assertEqual(ask("100 f in c")["title"], "37.78 °C")
        self.assertEqual(ask("2 cups in ml")["title"], "480 ml")
        self.assertEqual(ask("6 feet in cm")["title"], "182.88 cm")

    def test_mismatched_kinds(self):
        self.assertEqual(ask("5 km in kg"), {})


class Currency(unittest.TestCase):
    def test_convert(self):
        r = ask("100 usd in inr")
        self.assertEqual(r["kind"], "currency")
        self.assertEqual(r["title"], "8,333.33 INR")
        self.assertEqual(ask("$50 in euros")["title"], "46.3 EUR")

    def test_offline_without_rates(self):
        self.assertEqual(an.answer("100 usd in inr", NOW, lambda: None), {})


class Limits(unittest.TestCase):
    def test_huge_powers_are_refused_quickly(self):
        # (audit #324: a tower of powers built a number with billions of digits)
        self.assertEqual(ask("((((9^100)^100)^100)^100)^100"), {})
        self.assertEqual(ask("1000000000 ^ 50"), {})
        self.assertEqual(ask("2^10")["title"], "1,024")

    def test_roots_of_negatives_answer_nothing(self):
        self.assertEqual(ask("square root of -4"), {})
        self.assertEqual(ask("square root of 16")["title"], "4")

    def test_serve_always_answers_with_the_id(self):
        import io
        import json
        import sys
        old_in, old_out = sys.stdin, sys.stdout
        sys.stdin, sys.stdout = io.StringIO('{"id": 7, "q": "square root of -4"}\nnot json\n'), io.StringIO()
        try:
            an.main(["--serve"])
            lines = [json.loads(x) for x in sys.stdout.getvalue().splitlines()]
        finally:
            sys.stdin, sys.stdout = old_in, old_out
        self.assertEqual(lines, [{"id": 7}, {"id": None}])

    def test_a_failed_fetch_is_not_tried_on_every_keystroke(self):
        import tempfile
        from unittest import mock
        calls = []

        def offline(*a, **k):
            calls.append(1)
            raise OSError("offline")
        with tempfile.TemporaryDirectory() as d, mock.patch.object(an, "RATES_FILE", Path(d) / "rates.json"), \
                mock.patch.object(an, "_failed_at", [0.0]), mock.patch.object(an.urllib.request, "urlopen", offline):
            self.assertIsNone(an.rates())
            self.assertIsNone(an.rates())
            self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
