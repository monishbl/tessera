"""Tessera as one signed bundle per version (tools/tessera-bundle.py,
docs/UPDATE-PLAN.md steps 3-6): packing a checkout, verifying a download
(minisign over the manifest, the archive's and every file's sha256, nothing
unlisted, no link out), unpacking it as a version, the atomic swap of
`current` with `previous` kept for undo, the move of a git install to its
first bundle, the package lock's rules (here and in the root helper), the
hold file, and Software Update's check, stage and finish for a copy that
runs from a bundle. No network: the "releases" are file:// folders,
signed with a throwaway key made in each test (python-cryptography writes
minisign's format), the programs are small scripts that answer --version,
and nothing touches the real data folder, /etc or pacman."""
import base64
import hashlib
import importlib.machinery
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
HAS_CRYPTO = importlib.util.find_spec("cryptography") is not None


def load(path, name):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(name, loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


B = load(ROOT / "tools/tessera-bundle.py", "tessera_bundle_t")
R = load(ROOT / "shell/assets/scripts/tessera-root.py", "tessera_root_bundles_t")


class Key:
    """A throwaway minisign key (the format, written with python-cryptography)"""

    def __init__(self):
        from cryptography.hazmat.primitives import serialization
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        self.priv = Ed25519PrivateKey.generate()
        self.keyid = os.urandom(8)
        pk = self.priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
        self.pub = ("untrusted comment: minisign public key\n" + base64.b64encode(b"Ed" + self.keyid + pk).decode() + "\n")

    def sign(self, data, trusted):
        sig = self.priv.sign(hashlib.blake2b(data, digest_size=64).digest())
        glob = self.priv.sign(sig + trusted.encode())
        return ("untrusted comment: signature from minisign secret key\n" + base64.b64encode(b"ED" + self.keyid + sig).decode() + "\n"
                + "trusted comment: " + trusted + "\n" + base64.b64encode(glob).decode() + "\n")


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@t", *args],
                          capture_output=True, text=True, check=True).stdout.strip()


CONFIG_QML = """import QtQuick
Singleton {
    JsonAdapter {
        property JsonObject bar: JsonObject {
            property int height: %d
        }
    }
}
"""
# The tools a version needs for Software Update to run from it
TOOLS = ("tessera-update.py", "tessera-bundle.py", "native-install.py", "tessera_sources.py", "sources.txt", "config-sync.py",
         "tessera-ui.pub", "settings-migrations.json")


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="tessera-bundles-")
        self.d = Path(self.tmp.name)
        self.src = self.d / "src"
        self.make_checkout()
        self.share = self.d / "share"
        self.data = self.share / "tessera"
        self.config = self.d / "config"
        self.rel = self.d / "releases"
        self.key = Key() if HAS_CRYPTO else None
        (self.d / "key.pub").write_text(self.key.pub if self.key else "")
        self.env = mock.patch.dict(os.environ, {
            "XDG_DATA_HOME": str(self.share), "XDG_CONFIG_HOME": str(self.config), "XDG_STATE_HOME": str(self.d / "state"),
            "XDG_CACHE_HOME": str(self.d / "cache"), "XDG_RUNTIME_DIR": str(self.d / "run"), "HOME": str(self.d / "home"),
            "TESSERA_UI_VARIANT": "x86-64-v3", "TESSERA_UI_PUBKEY": str(self.d / "key.pub"),
            "TESSERA_BUNDLES": "file://" + str(self.rel) + "/{tag}", "TESSERA_HOLD_CONF": str(self.d / "hold.conf"),
            "TESSERA_NATIVE_DEST": str(self.d / "nbin"), "TESSERA_NO_INHIBIT": "1"})
        self.env.start()
        os.environ.pop("TESSERA_BUNDLE_HOME", None)
        os.environ.pop("TESSERA_UPDATE_MODE", None)
        (self.d / "run").mkdir()
        (self.d / "home").mkdir()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def make_checkout(self):
        s = self.src
        for rel, text in {"shell/shell.qml": "ShellRoot {}\n", "shell/common/Config.qml": CONFIG_QML % 32,
                          "shell/assets/scripts/run.sh": "#!/bin/sh\necho hi\n", "shell/glass.frag": "void main() {}\n",
                          "hypr/hyprland.lua": "-- hypr\n", "hypr/tessera/keymap.json": "[]\n", "tools/tessera": "#!/bin/sh\n",
                          "install.sh": "#!/bin/sh\n", "CHANGELOG.md": "# Tessera changelog\n\n## First · 2026-11-01\n\nHello.\n",
                          "native/tessera-ui/main.rs": "fn main() {}\n", "tests/test_x.py": "", "site/index.html": "",
                          "shell/assets/scripts/migrate-names.py": "OLD_KEYMAP = None\n"}.items():
            p = s / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)
        for t in TOOLS:
            if (ROOT / "tools" / t).exists():
                shutil.copy2(ROOT / "tools" / t, s / "tools" / t)
        (s / "shell/assets/scripts/run.sh").chmod(0o755)
        os.symlink("assets/scripts/run.sh", s / "shell/run-link")
        git(self.d, "init", "-q", "-b", "main", str(s))
        git(s, "add", "-A")
        git(s, "commit", "-q", "-m", "one")
        # what the builds leave in the checkout (not in git)
        (s / "shell/glass.frag.qsb").write_bytes(b"QSB1")
        (s / "shell/native/build/qml").mkdir(parents=True)
        (s / "shell/native/build/qml/libnative.so").write_bytes(b"\x7fELF native")

    def commit_change(self, height, entry=None):
        (self.src / "shell/common/Config.qml").write_text(CONFIG_QML % height)
        if entry:
            text = (self.src / "CHANGELOG.md").read_text()
            (self.src / "CHANGELOG.md").write_text(text.replace("# Tessera changelog\n\n", f"# Tessera changelog\n\n## {entry} · 2026-11-22\n\nNew.\n\n"))
        git(self.src, "commit", "-q", "-am", "change")
        return git(self.src, "rev-parse", "HEAD")

    def programs(self, said="0.1.0"):
        d = self.d / ("progs-" + said)
        d.mkdir(exist_ok=True)
        for n in B.PROGRAMS:
            (d / n).write_text(f"#!/bin/sh\n[ \"$1\" = --version ] && echo '{n} {said}' && exit 0\nexit 3\n")
            (d / n).chmod(0o755)
        return {n: str(d / n) for n in B.PROGRAMS}

    def plugin(self):
        d = self.d / "plugin"
        d.mkdir(exist_ok=True)
        (d / "tessera-glass.so").write_bytes(b"\x7fELF glass")
        (d / "tessera-glass.built-for").write_text("0.57.1\n")
        return str(d)

    def publish(self, commit=None, tag="tessera-ui-beta", lock=None, key=None, trusted=None, latest=True, edit=None):
        """Both variants packed, the manifest signed (and the pointer)"""
        commit = commit or git(self.src, "rev-parse", "HEAD")
        out = self.rel / tag
        out.mkdir(parents=True, exist_ok=True)
        entries = {v: B.pack_variant(str(self.src), commit, v, self.programs(), self.plugin(), str(out), 1700000000) for v in B.VARIANTS}
        m = B.make_manifest(str(self.src), commit, "beta", entries, lock or {"held": {}, "minimum": {}}, tree="t" * 40, tag=tag)
        if edit:
            edit(m)
        data = json.dumps(m, indent=1).encode()
        k = key or self.key
        name = B.manifest_name(commit)
        (out / name).write_bytes(data)
        (out / (name + ".minisig")).write_text(k.sign(data, trusted or f"{name} commit={commit} channel=beta"))
        if latest:
            (out / B.LATEST).write_bytes(data)
            (out / (B.LATEST + ".minisig")).write_text(k.sign(data, f"{B.LATEST} commit={commit} channel=beta"))
        return commit, m

    def bases(self):
        return ["file://" + str(self.rel) + "/{tag}"]


# ── Packing ───────────────────────────────────────────────────────────────

class Packing(Fixture):
    def test_a_bundle_carries_the_version_and_its_builds_not_its_sources(self):
        commit = git(self.src, "rev-parse", "HEAD")
        e = B.pack_variant(str(self.src), commit, "generic", self.programs(), self.plugin(), str(self.d), 1700000000)
        files = e["files"]
        for rel in ("shell/shell.qml", "shell/glass.frag.qsb", "shell/native/build/qml/libnative.so", "bin/tessera-ui",
                    "bin/tessera-events", "plugin/tessera-glass.so", "plugin/tessera-glass.built-for", "install.sh", "CHANGELOG.md"):
            self.assertIn(rel, files)
        for rel in files:
            self.assertFalse(rel.startswith(("native/", "tests/", "site/")), rel)
        self.assertEqual(files["bin/tessera-ui"]["mode"], 0o755)
        self.assertEqual(files["shell/assets/scripts/run.sh"]["mode"], 0o755)
        self.assertEqual(files["shell/shell.qml"]["mode"], 0o644)
        self.assertEqual(e["links"], {"shell/run-link": "assets/scripts/run.sh"})
        self.assertEqual(e["programs"]["tessera-ui"]["version"], "tessera-ui 0.1.0")
        self.assertEqual(e["sha256"], B.sha256_of(str(self.d / e["file"])))
        # Reproducible: the same checkout packs the same archive
        again = B.pack_variant(str(self.src), commit, "generic", self.programs(), self.plugin(), str(self.d / ".."), 1700000000)
        self.assertEqual(again["sha256"], e["sha256"])

    def test_the_manifest_carries_what_the_check_reads(self):
        commit = git(self.src, "rev-parse", "HEAD")
        e = B.pack_variant(str(self.src), commit, "generic", self.programs(), self.plugin(), str(self.d), 1)
        m = B.make_manifest(str(self.src), commit, "beta", {"generic": e}, {"held": {"hyprland": {"version": "0.57.1-1"}}, "minimum": {}})
        self.assertEqual(m["packages"]["held"], {"hyprland": {"version": "0.57.1-1", "arch": "x86_64"}})
        self.assertIn("property int height: 32", m["texts"]["shell/common/Config.qml"])
        self.assertIn("## First", m["changelog"])
        self.assertGreater(m["time"], 0)

    def test_a_link_out_of_the_checkout_is_never_packed(self):
        os.symlink("../../etc/passwd", self.src / "shell/evil")
        git(self.src, "add", "-A")
        git(self.src, "commit", "-q", "-m", "evil")
        with self.assertRaises(B.Failed):
            B.pack_variant(str(self.src), git(self.src, "rev-parse", "HEAD"), "generic", self.programs(), self.plugin(), str(self.d), 1)


# ── Verifying and unpacking ───────────────────────────────────────────────

@unittest.skipUnless(HAS_CRYPTO, "needs python-cryptography")
class Verify(Fixture):
    def test_a_signed_bundle_is_fetched_checked_and_unpacked(self):
        commit, m = self.publish()
        got = B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub)
        vdir = self.data / "versions" / got["id"]
        self.assertEqual(got["id"], commit[:12])
        self.assertEqual(got["variant"], "x86-64-v3")
        self.assertEqual((vdir / "shell/shell.qml").read_text(), "ShellRoot {}\n")
        self.assertEqual(os.readlink(vdir / "shell/run-link"), "assets/scripts/run.sh")
        self.assertTrue(os.access(vdir / "bin/tessera-ui", os.X_OK))
        self.assertEqual(B.verify_tree(str(vdir)), [])
        rec = json.loads((vdir / "bin/tessera-ui.json").read_text())
        self.assertEqual((rec["source"], rec["commit"]), ("bundle", commit))
        # (the login screen's check: the program is the one its signed manifest names)
        data = (vdir / "bin/tessera-ui").read_bytes()
        self.assertEqual(R.login_ui_vouched(data, rec, key=self.key.pub), "signed")
        with self.assertRaises(R.Refused):
            R.login_ui_vouched(data + b"x", rec, key=self.key.pub)
        # Fetching it again: nothing downloaded, it's here whole
        self.assertTrue(B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub).get("kept"))

    def test_another_key_or_another_versions_signature_is_refused(self):
        commit, _ = self.publish(key=Key())
        with self.assertRaises(B.Failed) as e:
            B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub)
        self.assertIn("another key", e.exception.why)
        commit, _ = self.publish(trusted="tessera-0000.bundle.json commit=" + "0" * 40)
        with self.assertRaises(B.Failed) as e:
            B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub)
        self.assertIn("another file or version", e.exception.why)
        self.assertFalse((self.data / "versions" / commit[:12]).exists())

    def test_a_damaged_archive_is_refused_and_nothing_is_installed(self):
        commit, m = self.publish()
        f = self.rel / "tessera-ui-beta" / m["variants"]["x86-64-v3"]["file"]
        data = bytearray(f.read_bytes())
        data[len(data) // 2] ^= 0xFF
        f.write_bytes(bytes(data))
        with self.assertRaises(B.Failed) as e:
            B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub)
        self.assertIn("signed checksum", e.exception.why)
        self.assertFalse((self.data / "versions" / commit[:12]).exists())

    def test_no_bundle_yet_is_missing_not_refused(self):
        p = B.plan("a" * 40, "beta", base_list=self.bases(), pubkey=self.key.pub)
        self.assertEqual((p["available"], p["kind"]), (False, "missing"))

    def test_the_plan_says_the_size_and_the_held_packages_it_moves(self):
        lock = {"held": {"hyprland": {"version": "0.57.2-1"}, "quickshell": {"version": "0.3.2-1"}}, "minimum": {"qt6-base": "6.10.1-1"}}
        commit, m = self.publish(lock=lock)
        p = B.plan(commit, "beta", base_list=self.bases(), pubkey=self.key.pub, installed={"hyprland": "0.57.1-3", "qt6-base": "6.10.1-2"},
                   repo={"hyprland": "0.58.0-1"})
        self.assertTrue(p["available"])
        self.assertEqual(p["size"], m["variants"]["x86-64-v3"]["size"])
        self.assertEqual([x["name"] for x in p["moves"]["held"]], ["hyprland"])
        self.assertEqual(p["moves"]["held"][0]["url"], "https://archive.archlinux.org/packages/h/hyprland/hyprland-0.57.2-1-x86_64.pkg.tar.zst")
        self.assertEqual(p["moves"]["hold"], ["hyprland"])  # (quickshell isn't installed: left alone)
        self.assertFalse(p["moves"]["system"])
        self.assertEqual(p["movesText"], "hyprland 0.57.1 → 0.57.2")


class Unpack(Fixture):
    def tarball(self, members):
        """A zstd tar of (name, bytes or ("link", target))"""
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w") as tar:
            for name, content in members:
                ti = tarfile.TarInfo(name)
                if isinstance(content, tuple):
                    ti.type, ti.linkname = tarfile.SYMTYPE, content[1]
                    tar.addfile(ti)
                else:
                    ti.size = len(content)
                    tar.addfile(ti, io.BytesIO(content))
        plain = self.d / "t.tar"
        plain.write_bytes(buf.getvalue())
        out = self.d / "t.tar.zst"
        subprocess.run(["zstd", "-q", "-f", "-o", str(out), str(plain)], check=True)
        return str(out)

    def entry(self, files, links=None):
        return {"files": {n: {"sha256": hashlib.sha256(c).hexdigest(), "size": len(c), "mode": 0o644} for n, c in files.items()},
                "links": links or {}}

    def into(self):
        d = self.d / ("into-" + os.urandom(3).hex())
        d.mkdir()
        return str(d)

    def test_exactly_what_the_manifest_lists(self):
        good = {"shell/a.qml": b"a", "bin/x": b"x"}
        B.unpack(self.tarball(list(good.items())), self.into(), self.entry(good))
        # one more file than listed
        with self.assertRaises(B.Failed) as e:
            B.unpack(self.tarball(list(good.items()) + [("shell/extra", b"!")]), self.into(), self.entry(good))
        self.assertIn("doesn't list", e.exception.why)
        # one missing
        with self.assertRaises(B.Failed) as e:
            B.unpack(self.tarball([("shell/a.qml", b"a")]), self.into(), self.entry(good))
        self.assertIn("missing bin/x", e.exception.why)
        # a file changed (same size)
        with self.assertRaises(B.Failed) as e:
            B.unpack(self.tarball([("shell/a.qml", b"b"), ("bin/x", b"x")]), self.into(), self.entry(good))
        self.assertIn("signed checksum", e.exception.why)

    def test_no_path_or_link_leads_out(self):
        for bad in ("../escape", "/etc/passwd", "shell/../../x", ".git/config"):
            with self.assertRaises(B.Failed):
                B.unpack(self.tarball([(bad, b"x")]), self.into(), self.entry({"shell/a": b"x"}))
        # an unlisted link, or a listed one out of the bundle (the manifest's check refuses it first)
        with self.assertRaises(B.Failed):
            B.unpack(self.tarball([("shell/a", b"x"), ("shell/l", ("link", "../../../etc"))]), self.into(), self.entry({"shell/a": b"x"}))
        self.assertFalse(B.link_ok("shell/l", "../../etc"))
        self.assertFalse(B.link_ok("shell/l", "/etc"))
        self.assertTrue(B.link_ok("shell/l", "../tools/x"))

    def test_a_link_then_a_file_through_it_cant_write_outside(self):
        # (links are made last, so a file can't be written through one)
        outside = self.d / "outside"
        outside.mkdir()
        files = {"shell/dir/f": b"x"}
        with self.assertRaises(B.Failed):
            B.unpack(self.tarball([("shell/dir", ("link", str(outside))), ("shell/dir/f", b"x")]), self.into(),
                     self.entry(files, {"shell/dir": str(outside)}))
        self.assertEqual(list(outside.iterdir()), [])


# ── Current, previous, undo, migration ───────────────────────────────────

@unittest.skipUnless(HAS_CRYPTO, "needs python-cryptography")
class Swap(Fixture):
    def install(self, height=None, entry=None):
        commit = self.commit_change(height, entry) if height else git(self.src, "rev-parse", "HEAD")
        self.publish(commit)
        return B.fetch(commit, "beta", base_list=self.bases(), pubkey=self.key.pub)["id"]

    def test_activate_swaps_current_and_keeps_previous_for_undo(self):
        one = self.install()
        B.activate(one)
        self.assertEqual(os.readlink(self.data / "current"), "versions/" + one)
        self.assertFalse((self.data / "previous").exists())
        # Everything outside versions/ goes through current
        self.assertEqual(os.readlink(self.data / "bin/tessera-ui"), "../current/bin/tessera-ui")
        self.assertEqual(subprocess.run([str(self.data / "bin/tessera-ui"), "--version"], capture_output=True, text=True).stdout.strip(),
                         "tessera-ui 0.1.0")
        self.assertEqual(os.readlink(self.data / "tessera-glass.so"), "current/plugin/tessera-glass.so")
        self.assertEqual((self.data / "tessera-glass.so").read_bytes(), b"\x7fELF glass")
        self.assertEqual(os.readlink(self.config / "quickshell/tessera"), str(self.data / "current" / "shell"))
        self.assertEqual((self.data / "source").read_text().strip(), str(self.data / "current"))
        two = self.install(40)
        B.activate(two)
        self.assertEqual((B.read_link(str(self.data / "current")), B.read_link(str(self.data / "previous"))), (two, one))
        self.assertIn("height: 40", (self.config / "quickshell/tessera/common/Config.qml").read_text())
        # Undo, and undo again (forward)
        self.assertEqual(B.undo()["current"], one)
        self.assertIn("height: 32", (self.config / "quickshell/tessera/common/Config.qml").read_text())
        self.assertEqual(B.undo()["current"], two)
        # Nothing else in the data folder holds a version: prune keeps these two
        three = self.install(48)
        self.assertEqual(B.prune(), [three])
        self.assertTrue((self.data / "versions" / one).is_dir() and (self.data / "versions" / two).is_dir())

    def test_undo_without_a_previous_version_changes_nothing(self):
        one = self.install()
        B.activate(one)
        with self.assertRaises(B.Failed):
            B.undo()
        self.assertEqual(B.read_link(str(self.data / "current")), one)

    def test_rollback_marks_the_version_bad(self):
        one = self.install()
        B.activate(one)
        two = self.install(40)
        B.activate(two)
        r = B.rollback()
        self.assertEqual(r["current"], one)
        self.assertTrue(B.is_bad(r["bad"]))
        note = json.loads((self.d / "state/tessera/rolled-back.json").read_text())
        self.assertEqual((note["from"], note["to"]), (two, one))

    def test_a_git_install_moves_to_its_first_bundle_keeping_what_was_there(self):
        # A normal install from before bundles: the shell copied, the programs and plugin files
        qs = self.config / "quickshell/tessera"
        qs.mkdir(parents=True)
        (qs / "shell.qml").write_text("old copy")
        (self.data / "bin").mkdir(parents=True)
        (self.data / "bin/tessera-ui").write_text("old program")
        (self.data / "tessera-glass.so").write_text("old plugin")
        (self.data / "source").write_text(str(self.data / "repo") + "\n")
        settings = self.config / "tessera/config.json"
        settings.parent.mkdir(parents=True)
        settings.write_text('{"bar": {"height": 50}}')
        one = self.install()
        B.activate(one)
        self.assertTrue(qs.is_symlink())
        kept = self.data / "backups/before-bundles"
        self.assertEqual((kept / "tessera/shell.qml").read_text(), "old copy")
        self.assertEqual((kept / "tessera-ui").read_text(), "old program")
        self.assertEqual((kept / "tessera-glass.so").read_text(), "old plugin")
        self.assertEqual((self.data / "source").read_text().strip(), str(self.data / "current"))
        # (your settings are never touched)
        self.assertEqual(settings.read_text(), '{"bar": {"height": 50}}')


class Mode(Fixture):
    def test_how_each_copy_updates(self):
        self.assertEqual(B.mode(str(self.src), str(self.data), str(self.config)), "dev")  # (any other folder)
        v = self.data / "versions" / ("a" * 12)
        (v / "tools").mkdir(parents=True)
        (v / "bundle.json").write_text("{}")
        self.assertEqual(B.mode(str(v), str(self.data), str(self.config)), "bundle")
        # setup.sh's checkout, its shell copied, following origin/main, nothing of its own
        repo = self.data / "repo"
        subprocess.run(["git", "clone", "-q", str(self.src), str(repo)], check=True, capture_output=True)
        (self.config / "quickshell/tessera").mkdir(parents=True)
        self.assertEqual(B.mode(str(repo), str(self.data), str(self.config)), "git")
        (repo / "install.sh").write_text("edited\n")
        self.assertEqual(B.mode(str(repo), str(self.data), str(self.config)), "dev")  # (edits of its own)
        git(repo, "checkout", "-q", "--", "install.sh")
        self.assertEqual(B.mode(str(repo), str(self.data), str(self.config), cfg={"update": {"mode": "dev"}}), "dev")
        shutil.rmtree(self.config / "quickshell/tessera")
        os.symlink(str(repo / "shell"), self.config / "quickshell/tessera")  # (install.sh --dev)
        self.assertEqual(B.mode(str(repo), str(self.data), str(self.config)), "dev")
        with mock.patch.dict(os.environ, {"TESSERA_UPDATE_MODE": "bundle"}):
            self.assertEqual(B.mode(str(repo), str(self.data), str(self.config)), "bundle")


# ── The package lock ──────────────────────────────────────────────────────

class Lock(unittest.TestCase):
    GOOD = {"held": {"hyprland": {"version": "0.57.1-1", "arch": "x86_64"}, "aquamarine": {"version": "1:0.9.5-2.1"}},
            "minimum": {"qt6-base": "6.10.1-1", "python-cryptography": "45.0.0-1"}}

    def test_a_good_lock_both_here_and_in_the_root_helper(self):
        self.assertEqual(B.check_lock(self.GOOD)["held"]["aquamarine"], {"version": "1:0.9.5-2.1", "arch": "x86_64"})
        self.assertEqual(R.check_lock(self.GOOD), B.check_lock(self.GOOD))
        # (the root helper takes a whole bundle manifest too)
        self.assertEqual(R.check_lock({"name": "tessera-bundle", "packages": self.GOOD}), B.check_lock(self.GOOD))

    def test_names_versions_and_what_may_be_held_are_checked(self):
        bad = [
            {"held": {"glibc": {"version": "2.40-1"}}},  # (not one Tessera holds)
            {"held": {"hyprland": {"version": "0.57.1"}}},  # (no release)
            {"held": {"hyprland": {"version": "0.57.1-1; rm -rf /"}}},
            {"held": {"hyprland": {"version": "0.57.1-1", "arch": "../../x"}}},
            {"minimum": {"-rf": "1-1"}}, {"minimum": {"Qt6": "1-1"}}, {"minimum": {"a b": "1-1"}}, {"minimum": {".x": "1-1"}},
            {"minimum": {"qt6-base": ""}}, {"minimum": {"qt6-base": "6.10 1-1"}}, {"minimum": {"qt6-base": 6}},
            {"held": {"hyprland": {"version": "0.57.1-1"}}, "minimum": {"hyprland": "0.57.1-1"}},
            {"held": [], "minimum": {}}, {"url": "https://evil.example/x.pkg.tar.zst"}, "nope",
        ]
        for lock in bad:
            with self.assertRaises(B.Failed, msg=lock):
                B.check_lock(lock)
            if not (isinstance(lock, dict) and set(lock) - {"held", "minimum"}) and not (
                    isinstance(lock, dict) and "hyprland" in (lock.get("minimum") or {})):
                with self.assertRaises(R.Refused, msg=lock):
                    R.check_lock(lock)

    def test_archive_addresses_are_made_from_checked_parts_only(self):
        url = B.archive_url("hyprland", "0.57.1-1", "x86_64")
        self.assertEqual(url, "https://archive.archlinux.org/packages/h/hyprland/hyprland-0.57.1-1-x86_64.pkg.tar.zst")
        self.assertEqual(R.lock_archive_url("hyprland", "0.57.1-1", "x86_64"), url)
        # (a CachyOS rebuild's pkgrel: Arch's file)
        self.assertEqual(R.lock_archive_url("hyprland", "0.57.1-1.1", "x86_64"), url)
        self.assertTrue(R.ARCHIVE_URL.match(B.archive_url("aquamarine", "1:0.9.5-2", "x86_64")))
        for args in (("hyprland", "0.57.1-1", "arm64"), ("../h", "1-1", "x86_64"), ("hyprland", "1-1/../../x", "x86_64")):
            with self.assertRaises(B.Failed):
                B.archive_url(*args)
            with self.assertRaises(R.Refused):
                R.lock_archive_url(*args)

    @unittest.skipUnless(shutil.which("vercmp"), "pacman's vercmp isn't here")
    def test_vercmp_is_pacmans(self):
        pairs = [("1.0", "1.0"), ("1.0", "1.1"), ("1.0.1", "1.0"), ("1:1.0", "2.0"), ("1.0-1", "1.0-2"), ("1.0a", "1.0"),
                 ("1.0", "1.0rc1"), ("1.0.r12.gabc", "1.0"), ("0.57.1-1", "0.57.1-1.1"), ("2.0-1", "2.0"), ("1.0..1", "1.0.1"),
                 ("1.0_1", "1.0.1"), ("6.10.1-1", "6.9.3-2"), ("1.0~rc1", "1.0"), ("1.001", "1.1"), ("1.0+1", "1.0.1")]
        for a, b in pairs:
            want = int(subprocess.run(["vercmp", a, b], capture_output=True, text=True).stdout.strip())
            self.assertEqual(B.vercmp(a, b), want, (a, b))
            self.assertEqual(B.vercmp(b, a), -want, (b, a))

    def test_what_the_lock_moves(self):
        lock = {"held": {"hyprland": {"version": "0.57.2-1"}, "hyprutils": {"version": "0.10.0-1"}, "hyprlang": {"version": "0.6.4-1"},
                         "quickshell": {"version": "0.3.2-1"}, "aquamarine": {"version": "0.9.5-1"}},
                "minimum": {"qt6-base": "6.10.1-1", "mesa": "1:25.2.0-1", "gtk5": "1-1"}}
        installed = {"hyprland": "0.57.1-2", "hyprutils": "0.10.0-3", "qt6-base": "6.10.0-1", "mesa": "1:25.2.0-1",
                     "aquamarine": "0.9.5-1"}
        m = B.package_moves(lock, installed, repo={"hyprlang": "0.6.4-1"})
        by = {x["name"]: x for x in m["held"]}
        self.assertEqual(sorted(by), ["hyprland", "hyprlang"])  # (hyprutils: the same pkgver, another rebuild)
        self.assertEqual((by["hyprland"]["how"], by["hyprlang"]["how"]), ("archive", "repo"))
        self.assertEqual(by["hyprlang"]["from"], "")  # (not installed: installed)
        self.assertEqual(sorted(m["hold"]), ["aquamarine", "hyprland", "hyprlang", "hyprutils"])  # (quickshell isn't installed)
        self.assertEqual([x["name"] for x in m["minimum"]], ["qt6-base"])  # (gtk5 isn't installed: not this computer's)
        self.assertTrue(m["system"])


class RootHold(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="tessera-hold-")
        self.root = Path(self.tmp.name)
        (self.root / "etc/pacman.d").mkdir(parents=True)
        self.conf = self.root / "etc/pacman.conf"
        self.conf.write_text("# pacman\n[options]\nHoldPkg = pacman glibc\nArchitecture = auto\n\n[core]\nInclude = /etc/pacman.d/mirrorlist\n")

    def tearDown(self):
        self.tmp.cleanup()

    def test_held_packages_are_written_and_included_once(self):
        R.write_hold(["hyprland", "aquamarine"], prefix=str(self.root))
        hold = (self.root / "etc/pacman.d/tessera-hold.conf").read_text()
        self.assertIn("IgnorePkg = aquamarine hyprland\n", hold)
        text = self.conf.read_text()
        self.assertEqual(text.count(R.HOLD_INCLUDE), 1)
        # (in [options], right after its header, so pacman reads it as an option)
        self.assertLess(text.index("[options]"), text.index(R.HOLD_INCLUDE))
        self.assertLess(text.index(R.HOLD_INCLUDE), text.index("[core]"))
        self.assertTrue((self.root / "etc/pacman.conf.pre-tessera").exists())
        before = self.conf.read_text()
        R.write_hold(["hyprland"], prefix=str(self.root))
        self.assertEqual(self.conf.read_text(), before)
        self.assertIn("IgnorePkg = hyprland\n", (self.root / "etc/pacman.d/tessera-hold.conf").read_text())
        R.write_hold([], prefix=str(self.root))
        self.assertNotIn("IgnorePkg", (self.root / "etc/pacman.d/tessera-hold.conf").read_text())
        with self.assertRaises(R.Refused):
            R.write_hold(["glibc"], prefix=str(self.root))
        with self.assertRaises(R.Refused):
            R.with_hold_include("[core]\n")

    def test_update_takes_a_lock_file_by_its_full_path(self):
        self.assertEqual(R.parse_update(["--lock", "/home/u/.local/share/tessera/versions/abc/bundle.json"])["lock"],
                         "/home/u/.local/share/tessera/versions/abc/bundle.json")
        with self.assertRaises(R.Refused):
            R.parse_update(["--lock", "relative.json"])
        with self.assertRaises(R.Refused):
            R.parse_update(["--lock"])

    def test_the_moves_the_root_helper_makes(self):
        def run(cmd, **kw):
            out = ""
            if cmd[1] == "-Q":
                out = "hyprland 0.57.1-2\nhyprutils 0.10.0-3\n"
            elif cmd[1] == "-Si":
                out = "Name            : hyprland\nVersion         : 0.58.0-1\n\nName            : hyprutils\nVersion         : 0.10.0-1\n"
            elif cmd[0] == R.VERCMP:
                out = str(B.vercmp(cmd[1], cmd[2]))
            return subprocess.CompletedProcess(cmd, 0, out, "")
        lock = R.check_lock({"held": {"hyprland": {"version": "0.57.2-1"}, "hyprutils": {"version": "0.10.0-1"},
                                      "quickshell": {"version": "0.3.2-1"}}})
        m = R.lock_moves(lock, run)
        self.assertEqual(m["archive"], ["https://archive.archlinux.org/packages/h/hyprland/hyprland-0.57.2-1-x86_64.pkg.tar.zst"])
        self.assertEqual(m["repo"], [])
        self.assertEqual(sorted(m["hold"]), ["hyprland", "hyprutils"])
        calls = []

        def run2(cmd, **kw):
            calls.append(cmd)
            return run(cmd, **kw) if cmd[1] in ("-Q", "-Si") or cmd[0] == R.VERCMP else subprocess.CompletedProcess(cmd, 0, "", "")
        said = []
        self.assertTrue(R.apply_lock(lock, said.append, None, run2))
        self.assertIn([R.PACMAN, "-U", "--needed", "--noconfirm", "--", m["archive"][0]], calls)
        self.assertTrue(any("hyprland 0.57.1-2 -> 0.57.2-1" in s for s in said))


# ── Software Update from a bundle: check, stage, finish ──────────────────

@unittest.skipUnless(HAS_CRYPTO, "needs python-cryptography")
class Updater(Fixture):
    def updater(self, vid):
        """tools/tessera-update.py as the installed version runs it"""
        return load(self.data / "versions" / vid / "tools/tessera-update.py", "tessera_update_" + vid)

    def test_a_bundle_copy_checks_stages_and_finishes_without_git(self):
        one = self.publish()[0]
        vid = B.fetch(one, "beta", base_list=self.bases(), pubkey=self.key.pub)["id"]
        B.activate(vid)
        cfgdir = self.config / "tessera"
        cfgdir.mkdir(parents=True)
        (cfgdir / "config.json").write_text(json.dumps({"update": {"channel": "beta"}}))
        U = self.updater(vid)
        self.assertTrue(U.is_bundle())
        self.assertEqual(U.update_mode(), "bundle")
        self.assertEqual(U.check(str(cfgdir))["kind"], "none")  # (the pointer is this version)
        # A newer version is published: its download, and the default it changes
        two = self.commit_change(40, entry="Second")
        self.publish(two)
        r = U.check(str(cfgdir))
        self.assertEqual((r["available"], r["target"], r["how"]), (True, two, "bundle"))
        self.assertGreater(r["bundle"]["size"], 0)
        self.assertEqual([e["title"] for e in r["changelog"]], ["Second"])
        self.assertEqual([c["id"] for c in r["conflicts"]], ["setting:bar.height"])
        # Stage: downloaded, checked, beside the current one; nothing swapped yet
        out = U.stage(str(cfgdir), {"setting:bar.height": "new"})
        self.assertEqual(out["state"], "ready")
        self.assertEqual(B.read_link(str(self.data / "current")), vid)
        self.assertTrue((self.data / "versions" / two[:12]).is_dir())
        self.assertTrue(U.staged_info()["valid"])
        # Finish (the restart): your choice written, the version swapped in, the old one kept
        res = U.finish(str(cfgdir))
        self.assertTrue(res["applied"], res)
        self.assertEqual((B.read_link(str(self.data / "current")), B.read_link(str(self.data / "previous"))), (two[:12], vid))
        self.assertEqual(json.loads((cfgdir / "config.json").read_text())["bar"]["height"], 40)
        self.assertFalse(U.read_staged())

    def test_no_download_yet_asks_and_waiting_changes_nothing(self):
        one = self.publish()[0]
        vid = B.fetch(one, "beta", base_list=self.bases(), pubkey=self.key.pub)["id"]
        B.activate(vid)
        cfgdir = self.config / "tessera"
        cfgdir.mkdir(parents=True)
        U = self.updater(vid)
        with mock.patch.dict(os.environ, {"TESSERA_REMOTE_HEAD": "b" * 40}):
            r = U.check(str(cfgdir), channel="beta")
            self.assertEqual((r["available"], r["target"], r["how"]), (True, "b" * 40, "ask"))
            self.assertIn("noBundle", r)
            out = U.stage(str(cfgdir), {}, channel="beta")
            self.assertEqual(out["state"], "waiting")
            self.assertIn("still being published", out["message"])
            self.assertEqual(B.read_link(str(self.data / "current")), vid)
            self.assertFalse(U.read_staged())
        # A version that didn't start here isn't offered again
        bad = self.d / "state/tessera/bad-bundles.json"
        bad.parent.mkdir(parents=True, exist_ok=True)
        bad.write_text(json.dumps({"commits": ["b" * 40]}))
        with mock.patch.dict(os.environ, {"TESSERA_REMOTE_HEAD": "b" * 40}):
            self.assertFalse(U.check(str(cfgdir), channel="beta")["available"])

    def test_the_cli_waits_when_nobody_is_there(self):
        one = self.publish()[0]
        vid = B.fetch(one, "beta", base_list=self.bases(), pubkey=self.key.pub)["id"]
        B.activate(vid)
        cfgdir = self.config / "tessera"
        cfgdir.mkdir(parents=True)
        U = self.updater(vid)
        with mock.patch.dict(os.environ, {"TESSERA_REMOTE_HEAD": "c" * 40}), mock.patch("sys.stdin.isatty", return_value=False):
            self.assertEqual(U.cli_update(str(cfgdir), "beta"), 0)
        self.assertEqual(B.read_link(str(self.data / "current")), vid)


if __name__ == "__main__":
    unittest.main()
