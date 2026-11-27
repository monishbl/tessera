"""Quick Share's identity is made once and kept for a day (17 Nov: the
phone listed the computer once per visibility toggle, a new endpoint id each
time; 27 Nov: renewed daily, so it can't be followed from place to place,
#1009)."""
import asyncio
import json
import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

ENGINE = Path(__file__).resolve().parent.parent / "shell/assets/scripts/quickshare"
sys.path.insert(0, str(ENGINE))

from nearby import endpoint, identity  # noqa: E402

try:
    from nearby.service import QuickShareService  # noqa: E402
except ImportError:  # (needs python-cryptography / protobuf)
    QuickShareService = None


class Identity(unittest.TestCase):
    def test_made_once_then_reused(self):
        with tempfile.TemporaryDirectory() as d:
            f = os.path.join(d, "tessera/quickshare/identity.json")
            a, how = identity.load(f)
            self.assertEqual(how, "new")
            b, how = identity.load(f)
            self.assertEqual(how, "kept")
            self.assertEqual(a, b)
            self.assertEqual(len(a["endpoint_id"]), 4)
            # only you can read it
            self.assertEqual(stat.S_IMODE(os.stat(f).st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(os.stat(os.path.dirname(f)).st_mode), 0o700)

    def test_a_damaged_file_is_made_again(self):
        with tempfile.TemporaryDirectory() as d:
            f = os.path.join(d, "identity.json")
            Path(f).write_text("{not json")
            a, how = identity.load(f)
            self.assertEqual(how, "new")
            Path(f).write_text(json.dumps({"endpoint_id": "ab", "salt": "00", "token": "0000"}))
            self.assertEqual(identity.load(f)[1], "new")
            self.assertEqual(identity.load(f)[1], "kept")

    def test_renewed_after_a_day(self):
        with tempfile.TemporaryDirectory() as d:
            f = os.path.join(d, "identity.json")
            a, how = identity.load(f, now=1000)
            self.assertEqual((how, a["made"]), ("new", 1000))
            b, how = identity.load(f, now=1000 + identity.MAX_AGE - 1)
            self.assertEqual((how, b), ("kept", a))
            c, how = identity.load(f, now=1000 + identity.MAX_AGE)
            self.assertEqual(how, "renewed")
            self.assertNotEqual(c["salt"], a["salt"])
            self.assertEqual(identity.load(f, now=1000 + identity.MAX_AGE + 5), (c, "kept"))
            self.assertEqual(stat.S_IMODE(os.stat(f).st_mode), 0o600)

    def test_an_undated_one_is_dated_and_kept(self):
        # (from before 27 Nov: phones that know it keep it a day more)
        with tempfile.TemporaryDirectory() as d:
            f = os.path.join(d, "identity.json")
            old = {"endpoint_id": "ABCD", "salt": "00" * 16, "token": "0000"}
            Path(f).write_text(json.dumps(old))
            a, how = identity.load(f, now=5000)
            self.assertEqual(how, "kept")
            self.assertEqual(a, dict(old, made=5000))
            self.assertEqual(json.loads(Path(f).read_text())["made"], 5000)

    def test_seconds_left(self):
        self.assertEqual(identity.seconds_left({"made": 100}, now=100), identity.MAX_AGE)
        self.assertEqual(identity.seconds_left({"made": 100}, now=100 + identity.MAX_AGE * 2), 0)
        self.assertEqual(identity.seconds_left({}, now=100), 0)
        self.assertEqual(identity.seconds_left({"made": 10_000}, now=100), 0)  # (a clock that was wrong)

    def test_the_state_folder_follows_xdg(self):
        with tempfile.TemporaryDirectory() as d:
            old = os.environ.get("XDG_STATE_HOME")
            os.environ["XDG_STATE_HOME"] = d
            try:
                self.assertEqual(identity.path(), os.path.join(d, "tessera/quickshare/identity.json"))
            finally:
                if old is None:
                    del os.environ["XDG_STATE_HOME"]
                else:
                    os.environ["XDG_STATE_HOME"] = old

    def test_the_endpoint_info_is_the_same_each_time(self):
        me = identity.new()
        salt = bytes.fromhex(me["salt"])
        one = endpoint.endpoint_info("Monish-Work", salt=salt)
        self.assertEqual(one, endpoint.endpoint_info("Monish-Work", salt=salt))
        self.assertEqual(one[1:17], salt)
        self.assertEqual(endpoint.parse_endpoint_info(one)["name"], "Monish-Work")
        # (without a salt: random, as before)
        self.assertNotEqual(endpoint.endpoint_info("x")[1:17], endpoint.endpoint_info("x")[1:17])


@unittest.skipIf(QuickShareService is None, "needs python-cryptography and python-protobuf")
class ServiceKeepsIt(unittest.TestCase):
    def test_two_starts_advertise_one_device(self):
        with tempfile.TemporaryDirectory() as d:
            f = os.path.join(d, "identity.json")
            first = QuickShareService(lambda ev: None, None, name="Monish-Work", identity=identity.load(f)[0])
            again = QuickShareService(lambda ev: None, None, name="Monish-Work", identity=identity.load(f)[0])
            self.assertEqual(first.endpoint_id, again.endpoint_id)
            self.assertEqual(first.device_token, again.device_token)
            self.assertEqual(first._endpoint_info(), again._endpoint_info())
            self.assertEqual(endpoint.service_name(first.endpoint_id), endpoint.service_name(again.endpoint_id))
            # a rename keeps the id (the name is only in the info)
            asyncio.run(again.configure(name="Desk"))
            self.assertEqual(again.endpoint_id, first.endpoint_id)
            self.assertEqual(again._endpoint_info()[1:17], first._endpoint_info()[1:17])

    def test_a_day_old_identity_is_renewed_when_shown(self):
        old = identity.new(now=0)
        fresh = identity.new()
        s = QuickShareService(lambda ev: None, None, name="Desk", identity=old, renew=lambda: (fresh, "renewed"))
        s.preferred_port = 0
        info = s._endpoint_info()

        async def go():
            await s.configure(visible=True)
            await s.configure(visible=False)
        asyncio.run(go())
        self.assertEqual(s.endpoint_id, fresh["endpoint_id"])
        self.assertEqual(s.salt, bytes.fromhex(fresh["salt"]))
        self.assertNotEqual(s._endpoint_info()[1:17], info[1:17])
        # a fresh one stays
        s2 = QuickShareService(lambda ev: None, None, name="Desk", identity=fresh, renew=lambda: (identity.new(), "renewed"))
        s2.preferred_port = 0

        async def go2():
            await s2.configure(visible=True)
            shown = s2.endpoint_id
            await s2.configure(visible=False)
            return shown
        self.assertEqual(asyncio.run(go2()), fresh["endpoint_id"])


if __name__ == "__main__":
    unittest.main()
