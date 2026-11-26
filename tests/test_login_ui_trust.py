"""The login screen's tessera-ui (20 Nov audit #71, #24): root copies yours
for greetd only when it's Tessera's signed release build (its record keeps
the signed manifest, checked against root's own copy of the release key)
or a build of yours its record vouches for (the sha256 recorded right after
it was built). Anything else is refused and the login screen falls back to
the QML one. An update's root step refreshes it whenever yours is another
program now (sysupdate.py login_ui_stale), with the one password it asks
anyway. Throwaway keys, homes and system folders only."""
import base64
import hashlib
import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "shell/assets/scripts"
HAS_CRYPTO = importlib.util.find_spec("cryptography") is not None


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


R = load(SCRIPTS / "tessera-root.py", "tessera_root_trust")


class Key:
    """A throwaway minisign key (as tests/test_native_install.py's)"""

    def __init__(self):
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        self.priv = Ed25519PrivateKey.generate()
        self.keyid = os.urandom(8)
        pk = self.priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        self.pub = ("untrusted comment: minisign public key " + self.keyid[::-1].hex().upper() + "\n"
                    + base64.b64encode(b"Ed" + self.keyid + pk).decode() + "\n")

    def sign(self, data, trusted, prehash=True):
        msg = hashlib.blake2b(data, digest_size=64).digest() if prehash else data
        sig = self.priv.sign(msg)
        glob = self.priv.sign(sig + trusted.encode())
        return ("untrusted comment: signature from minisign secret key\n"
                + base64.b64encode((b"ED" if prehash else b"Ed") + self.keyid + sig).decode() + "\n"
                + "trusted comment: " + trusted + "\n" + base64.b64encode(glob).decode() + "\n")


PROGRAM = b"\x7fELF the login screen"
TREE = "a" * 40


def sha(data):
    return hashlib.sha256(data).hexdigest()


def ready_made(key, data=PROGRAM, tree=TREE, edit=None, trusted=None):
    """A ready-made install's record, as native-install.py writes it"""
    manifest = {"format": 1, "tree": tree, "commit": "b" * 40,
                "variants": {"x86-64-v3": {"programs": {"tessera-ui": {"sha256": sha(data)}, "tessera-events": {"sha256": "c" * 64}}},
                             "generic": {"programs": {"tessera-ui": {"sha256": "d" * 64}}}}}
    if edit:
        edit(manifest)
    text = json.dumps(manifest)
    return {"source": "ready-made", "variant": "x86-64-v3", "tree": tree, "sha256": sha(data), "manifest": text,
            "minisig": key.sign(text.encode(), trusted or f"tessera-ui-{tree}.manifest.json tree={tree} commit={'b' * 40}")}


@unittest.skipUnless(HAS_CRYPTO, "python-cryptography isn't installed")
class Vouched(unittest.TestCase):
    def setUp(self):
        self.key = Key()

    def test_signed_and_named_by_its_manifest(self):
        self.assertEqual(R.login_ui_vouched(PROGRAM, ready_made(self.key), key=self.key.pub), "signed")
        # (the generic build's checksum counts too: any variant the manifest signs)
        rec = ready_made(self.key, edit=lambda m: m["variants"]["generic"]["programs"]["tessera-ui"].update(sha256=sha(b"\x7fELF g")))
        self.assertEqual(R.login_ui_vouched(b"\x7fELF g", rec, key=self.key.pub), "signed")

    def test_a_bad_signature_is_refused(self):
        rec = ready_made(Key())  # (signed by another key)
        with self.assertRaisesRegex(R.Refused, "another key"):
            R.login_ui_vouched(PROGRAM, rec, key=self.key.pub)
        rec = ready_made(self.key)
        rec["manifest"] = rec["manifest"].replace('"format": 1', '"format": 2')  # (changed after it was signed)
        with self.assertRaisesRegex(R.Refused, "signature doesn't match"):
            R.login_ui_vouched(PROGRAM, rec, key=self.key.pub)
        rec = ready_made(self.key)
        rec["minisig"] = "not a signature"
        with self.assertRaises(R.Refused):
            R.login_ui_vouched(PROGRAM, rec, key=self.key.pub)
        rec = ready_made(self.key, trusted="tessera-ui tree=" + "e" * 40)  # (signed for another version)
        with self.assertRaisesRegex(R.Refused, "another version"):
            R.login_ui_vouched(PROGRAM, rec, key=self.key.pub)

    def test_a_checksum_the_manifest_doesnt_name_is_refused(self):
        with self.assertRaisesRegex(R.Refused, "checksum differs"):
            R.login_ui_vouched(PROGRAM + b"!", ready_made(self.key), key=self.key.pub)

    def test_an_older_record_without_the_manifest_is_refused(self):
        rec = ready_made(self.key)
        del rec["manifest"], rec["minisig"]
        with self.assertRaisesRegex(R.Refused, "wasn't kept"):
            R.login_ui_vouched(PROGRAM, rec, key=self.key.pub)

    def test_the_real_release_key_is_roots_own_copy(self):
        self.assertEqual(R.RELEASE_KEY.strip(), (ROOT / "tools/tessera-ui.pub").read_text().strip())
        with self.assertRaises(R.Refused):
            R.login_ui_vouched(PROGRAM, ready_made(self.key))  # (a throwaway key isn't Tessera's)


class BuiltHere(unittest.TestCase):
    def test_a_build_that_matches_its_record(self):
        for source in ("built", "built-tuned"):
            self.assertEqual(R.login_ui_vouched(PROGRAM, {"source": source, "sha256": sha(PROGRAM)}), "built")

    def test_a_build_changed_since_is_refused(self):
        with self.assertRaisesRegex(R.Refused, "changed after it was built"):
            R.login_ui_vouched(PROGRAM + b"tampered", {"source": "built", "sha256": sha(PROGRAM)})

    def test_no_record_is_refused(self):
        for rec in (None, {}, {"source": "built"}, {"source": "somewhere"}, ["built"]):
            with self.assertRaises(R.Refused):
                R.login_ui_vouched(PROGRAM, rec)

    def test_it_says_built_on_this_computer(self):
        self.assertEqual(R.LOGIN_UI_HOW["built"], "built on this computer")


class FromYourHome(unittest.TestCase):
    """read_login_ui: the file checks (ELF, owner, modes, no link) and the record"""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.home = Path(self.tmp.name) / "home"
        self.bin = self.home / ".local/share/tessera/bin"
        self.bin.mkdir(parents=True)
        self.bin.chmod(0o755)

    def tearDown(self):
        self.tmp.cleanup()

    def put(self, data=PROGRAM, record=None):
        (self.bin / "tessera-ui").write_bytes(data)
        (self.bin / "tessera-ui").chmod(0o755)
        if record is not False:
            (self.bin / "tessera-ui.json").write_text(json.dumps(record or {"source": "built", "sha256": sha(data)}))
            (self.bin / "tessera-ui.json").chmod(0o644)

    def test_a_build_of_yours(self):
        self.put()
        self.assertEqual(R.read_login_ui(os.getuid(), str(self.home)), (PROGRAM, "built"))

    @unittest.skipUnless(HAS_CRYPTO, "python-cryptography isn't installed")
    def test_a_signed_build(self):
        key = Key()
        self.put(record=ready_made(key))
        self.assertEqual(R.read_login_ui(os.getuid(), str(self.home), key=key.pub), (PROGRAM, "signed"))

    def test_without_its_record(self):
        self.put(record=False)
        with self.assertRaisesRegex(R.Refused, "no record"):
            R.read_login_ui(os.getuid(), str(self.home))

    def test_a_record_others_could_write(self):
        self.put()
        (self.bin / "tessera-ui.json").chmod(0o666)
        with self.assertRaisesRegex(R.Refused, "another account"):
            R.read_login_ui(os.getuid(), str(self.home))

    def test_tampered(self):
        self.put()
        (self.bin / "tessera-ui").write_bytes(PROGRAM + b"!")
        with self.assertRaisesRegex(R.Refused, "changed after it was built"):
            R.read_login_ui(os.getuid(), str(self.home))


class RootsRecord(unittest.TestCase):
    """Audit #95: a build of yours reaches the login screen only when root's
    own record (/var/lib/tessera/login-ui-trust.json, written by root:
    login-ui-trust, or once by the first root step of this version) lists
    its sha256; a refused one gives way to the one root last put there, else
    the QML login screen (bin/tessera-ui missing: greeter-start.sh starts
    the QML one). Never no login screen."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.prefix, self.home = d / "root", d / "home"
        self.bin = self.home / ".local/share/tessera/bin"
        self.bin.mkdir(parents=True)
        self.bin.chmod(0o755)
        self.trust = self.prefix / R.LOGIN_UI_TRUST.lstrip("/")
        self.share = self.prefix / R.LOGIN_SHARE.lstrip("/") / "bin"
        self.home_patch = mock.patch.object(R, "caller_home", lambda uid: str(self.home))
        self.home_patch.start()

    def tearDown(self):
        self.home_patch.stop()
        self.tmp.cleanup()

    def put(self, data=PROGRAM, source="built"):
        (self.bin / "tessera-ui").write_bytes(data)
        (self.bin / "tessera-ui").chmod(0o755)
        (self.bin / "tessera-ui.json").write_text(json.dumps({"source": source, "sha256": sha(data)}))
        (self.bin / "tessera-ui.json").chmod(0o644)

    def read(self):
        return R.read_login_ui(os.getuid(), str(self.home), prefix=str(self.prefix))

    def record(self, built=(), installed=None):
        self.trust.parent.mkdir(parents=True, exist_ok=True)
        self.trust.write_text(json.dumps({"built": list(built), "installed": installed}))
        self.trust.chmod(0o644)
        self.trust.parent.chmod(0o755)

    def test_no_record_yet_keeps_the_old_rule(self):
        # (an install from before: nobody is locked out of the Rust login
        # screen until a root step of this version writes the record)
        self.put()
        self.assertEqual(self.read(), (PROGRAM, "built"))

    def test_a_listed_build_is_taken(self):
        self.put()
        self.record([sha(PROGRAM)])
        self.assertEqual(self.read(), (PROGRAM, "built"))

    def test_an_unlisted_build_is_refused_even_with_a_matching_record_at_home(self):
        self.put(b"\x7fELF planted")  # (its record in your home rewritten to match)
        self.record([sha(PROGRAM)])
        with self.assertRaisesRegex(R.Refused, "login-ui-trust"):
            self.read()
        self.record([])
        with self.assertRaisesRegex(R.Refused, "login-ui-trust"):
            self.read()

    def test_a_record_root_didnt_write_alone_trusts_nothing(self):
        self.put()
        self.record([sha(PROGRAM)])
        self.trust.chmod(0o666)
        with self.assertRaisesRegex(R.Refused, "root's alone"):
            self.read()
        self.trust.chmod(0o644)
        self.trust.parent.chmod(0o777)
        with self.assertRaisesRegex(R.Refused, "root's alone"):
            self.read()
        self.trust.parent.chmod(0o755)
        self.trust.write_text("not json")
        with self.assertRaisesRegex(R.Refused, "can't be read"):
            self.read()
        self.trust.unlink()
        self.trust.symlink_to(self.prefix / "elsewhere.json")
        (self.prefix / "elsewhere.json").write_text(json.dumps({"built": [sha(PROGRAM)]}))
        with self.assertRaisesRegex(R.Refused, "root.s alone|isn.t a file"):
            self.read()
        with mock.patch.object(R.os, "geteuid", lambda: os.getuid() + 1):  # (owned by someone else than root)
            self.trust.unlink()
            self.record([sha(PROGRAM)])
            with self.assertRaisesRegex(R.Refused, "root's alone"):
                self.read()

    def test_its_entries_are_checked(self):
        self.record(["nothex", sha(PROGRAM), 5], {"sha256": "x"})
        self.assertEqual(R.read_login_trust(str(self.prefix)), {"built": [sha(PROGRAM)], "installed": None})

    def test_signed_builds_need_no_record(self):
        self.record([])
        self.assertIsNone(R.login_ui_trusted_here(PROGRAM, "signed", str(self.prefix)))

    def test_login_ui_trust_records_and_installs(self):
        self.put()
        self.share.mkdir(parents=True)
        self.record([])
        said = R.login_ui_trust(os.getuid(), prefix=str(self.prefix))
        self.assertIn("now uses", said)
        self.assertEqual((self.share / "tessera-ui").read_bytes(), PROGRAM)
        rec = R.read_login_trust(str(self.prefix))
        self.assertEqual(rec, {"built": [sha(PROGRAM)], "installed": {"sha256": sha(PROGRAM), "how": "built"}})
        self.assertEqual(self.read(), (PROGRAM, "built"))
        self.assertEqual(self.trust.stat().st_mode & 0o777, 0o644)
        self.assertEqual(self.trust.parent.stat().st_mode & 0o777, 0o755)

    def test_login_ui_trust_keeps_the_newest_few(self):
        for i in range(5):
            self.put(b"\x7fELF build %d" % i)
            R.login_ui_trust(os.getuid(), prefix=str(self.prefix))
        built = R.read_login_trust(str(self.prefix))["built"]
        self.assertEqual(built, [sha(b"\x7fELF build %d" % i) for i in (2, 3, 4)])

    def test_login_ui_trust_refuses_what_root_would_refuse(self):
        self.put()
        (self.bin / "tessera-ui").write_bytes(PROGRAM + b"tampered")  # (not what its record says)
        with self.assertRaisesRegex(R.Refused, "changed after it was built"):
            R.login_ui_trust(os.getuid(), prefix=str(self.prefix))
        self.assertFalse(self.trust.exists())
        (self.bin / "tessera-ui").unlink()
        with self.assertRaisesRegex(R.Refused, "no tessera-ui"):
            R.login_ui_trust(os.getuid(), prefix=str(self.prefix))

    def test_login_ui_trust_without_tesseras_login_screen_only_records(self):
        self.put()
        self.assertIn("is trusted", R.login_ui_trust(os.getuid(), prefix=str(self.prefix)))
        self.assertFalse(self.share.exists())
        self.assertEqual(R.read_login_trust(str(self.prefix))["installed"], None)

    def test_the_last_one_root_put_there_is_kept(self):
        self.share.mkdir(parents=True)
        ui = self.share / "tessera-ui"
        ui.write_bytes(PROGRAM)
        self.assertFalse(R.login_ui_keep_last(str(ui), str(self.prefix)))  # (no record: nothing vouches for it)
        self.record([], {"sha256": sha(PROGRAM), "how": "signed"})
        self.assertTrue(R.login_ui_keep_last(str(ui), str(self.prefix)))
        ui.write_bytes(PROGRAM + b"!")
        self.assertFalse(R.login_ui_keep_last(str(ui), str(self.prefix)))
        ui.unlink()
        self.assertFalse(R.login_ui_keep_last(str(ui), str(self.prefix)))

    def test_the_command_and_its_polkit_action(self):
        text = (SCRIPTS / "tessera-root.py").read_text()
        self.assertIn('if cmd == "login-ui-trust" and not args:', text)
        policy = (ROOT / "shell/assets/polkit/org.tessera.root.policy").read_text()
        self.assertIn('<annotate key="org.freedesktop.policykit.exec.argv1">login-ui-trust</annotate>', policy)
        rules = (ROOT / "shell/assets/polkit/49-tessera.rules").read_text()
        self.assertNotIn("login-ui-trust", rules)  # (always the password)
        self.assertIn("login-ui-trust", (ROOT / "install.sh").read_text())


class Stale(unittest.TestCase):
    """sysupdate.py login_ui_stale: the root step refreshes the login
    screen's tessera-ui only when yours is another program root would take"""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.system, self.home = d / "root", d / "home"
        self.env = mock.patch.dict(os.environ, {"TESSERA_SYSTEM_ROOT": str(self.system), "XDG_RUNTIME_DIR": str(d / "run")})
        self.env.start()
        self.su = load(SCRIPTS / "sysupdate.py", "sysupdate_stale_" + os.urandom(3).hex())
        self.share = self.system / "usr/share/tessera/bin"
        self.mine = self.home / ".local/share/tessera/bin/tessera-ui"
        self.mine.parent.mkdir(parents=True)

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def put(self, data, record):
        self.mine.write_bytes(data)
        self.mine.with_name("tessera-ui.json").write_text(json.dumps(record))

    def stale(self):
        return self.su.login_ui_stale(str(self.home))

    def test_no_login_screen_of_tesseras(self):
        self.put(PROGRAM, {"source": "built", "sha256": sha(PROGRAM)})
        self.assertFalse(self.stale())

    def test_the_same_program_needs_nothing(self):
        self.share.mkdir(parents=True)
        (self.share / "tessera-ui").write_bytes(PROGRAM)
        self.put(PROGRAM, {"source": "built", "sha256": sha(PROGRAM)})
        self.assertFalse(self.stale())

    def test_another_program_is_refreshed(self):
        self.share.mkdir(parents=True)
        (self.share / "tessera-ui").write_bytes(b"\x7fELF older")
        self.put(PROGRAM, {"source": "built", "sha256": sha(PROGRAM)})
        self.assertTrue(self.stale())
        # (fallen back to the QML one: its bin/ is still there)
        (self.share / "tessera-ui").unlink()
        self.assertTrue(self.stale())

    def test_a_build_roots_record_doesnt_list_asks_for_nothing(self):
        self.share.mkdir(parents=True)
        (self.share / "tessera-ui").write_bytes(b"\x7fELF older")
        self.put(PROGRAM, {"source": "built", "sha256": sha(PROGRAM)})
        trust = self.system / "var/lib/tessera/login-ui-trust.json"
        trust.parent.mkdir(parents=True)
        trust.write_text(json.dumps({"built": [sha(b"other")]}))
        self.assertFalse(self.stale())
        trust.write_text(json.dumps({"built": [sha(PROGRAM)]}))
        self.assertTrue(self.stale())

    def test_one_root_would_refuse_asks_for_nothing(self):
        self.share.mkdir(parents=True)
        (self.share / "tessera-ui").write_bytes(b"\x7fELF older")
        self.put(PROGRAM, {"source": "built", "sha256": sha(b"another")})
        self.assertFalse(self.stale())
        self.put(PROGRAM, {"source": "ready-made"})  # (an older record: no manifest kept)
        self.assertFalse(self.stale())
        manifest = json.dumps({"variants": {"x86-64-v3": {"programs": {"tessera-ui": {"sha256": sha(PROGRAM)}}}}})
        self.put(PROGRAM, {"source": "ready-made", "manifest": manifest, "minisig": "untrusted comment: x"})
        self.assertTrue(self.stale())  # (the signature is root's to check)

    def test_the_plan_takes_the_setup_step_for_it(self):
        text = (SCRIPTS / "sysupdate.py").read_text()
        self.assertIn('p["setup"] = not fake and (not helper_current(p["setup_dir"]) or login_ui_stale())', text)
        self.assertIn('"login": login_ui_stale()', text)
        rs = (ROOT / "native/tessera-ui/host/system/src/sysinfo/software_update.rs").read_text()
        self.assertIn('if (p["setup"] == true || self.login_stale) && !incoming.is_empty()', rs)


if __name__ == "__main__":
    unittest.main()
