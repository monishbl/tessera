"""The login screen's keys and the admin dialogs, read from their markup
(audit #778: what the Rust unit tests can't reach without a display). The
decisions themselves are tested in Rust: surfaces/greeter/src/view.rs
(field_key), model.rs (pick_session, pick_user, the power choice),
surfaces/session/src/lib.rs (key_do, hovered), surfaces/polkit/src/btagent.rs
(notice_do). Here: each greeter style's password field hands its keys to
them, keeps Enter and Escape, is read-only while the password is checked or
a power choice waits, and the lists give the keyboard back to the field.
Nothing runs; no session is touched."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "native/tessera-ui"
GREETER = (UI / "surfaces/greeter/ui/greeter.slint").read_text()
GREETER_LIB = (UI / "surfaces/greeter/src/lib.rs").read_text()
POLKIT = (UI / "surfaces/polkit/ui/polkit.slint").read_text()
POLKIT_LIB = (UI / "surfaces/polkit/src/lib.rs").read_text()


def component(src, name):
    """A component's text, up to the next top-level component"""
    start = re.search(r"^(export )?component %s\b" % re.escape(name), src, re.M)
    assert start, name
    end = re.search(r"^(export )?component \w+", src[start.end():], re.M)
    return src[start.start(): start.end() + (end.start() if end else len(src))]


def block(text, opener):
    """The braces' content after `opener` (its first match)"""
    i = text.index(opener)
    i = text.index("{", i + len(opener) - 1)
    depth = 0
    for j in range(i, len(text)):
        depth += {"{": 1, "}": -1}.get(text[j], 0)
        if depth == 0:
            return text[i + 1: j]
    raise AssertionError("unclosed " + opener)


STYLES = ("GlassStyle", "PlainStyle", "MaterialStyle")


class PasswordFields(unittest.TestCase):
    def test_each_style_hands_its_own_keys_first(self):
        for style in STYLES:
            keys = block(component(GREETER, style), "key(e) =>")
            with self.subTest(style=style):
                # F1–F3, Ctrl+U, Shift+Tab (#745): before anything else
                first = keys.index("Greet.field-key(e.text, e.modifiers.shift, e.modifiers.control, e.modifiers.alt)")
                self.assertLess(first, keys.index("Key.Return"))
                # Enter logs in with what's typed; Escape cancels; Tab steps the session
                self.assertRegex(keys, r"Key\.Return \{\s*Greet\.submit\(self\.text\);")
                self.assertRegex(keys, r"Key\.Escape \{\s*Greet\.cancel\(\);")
                self.assertRegex(keys, r"Key\.Tab \{\s*Greet\.next-session\(1\);")

    def test_the_field_is_read_only_while_checked_or_a_power_choice_waits(self):
        for style in STYLES:
            with self.subTest(style=style):
                self.assertIn('ro: Greet.busy || Greet.power-choice != "";', component(GREETER, style))

    def test_the_text_style_keeps_its_function_keys(self):
        text = component(GREETER, "TextStyle")
        for key, action in (("F1", "poweroff"), ("F2", "reboot"), ("F3", "suspend")):
            self.assertRegex(text, r"Key\.%s \{\s*Greet\.power\(\"%s\"\);" % (key, action))

    def test_the_rust_side_answers_the_field_keys(self):
        self.assertIn("g.on_field_key(", GREETER_LIB)
        self.assertIn("view::field_key(&t, shift, ctrl, alt)", GREETER_LIB)
        for k in ("FieldKey::Power(a) => self.model.choose_power(a)", "FieldKey::OtherUser => self.model.edit_name()",
                  "FieldKey::SessionBack => self.model.next_session(-1)"):
            self.assertIn(k, GREETER_LIB)


class Lists(unittest.TestCase):
    def test_each_style_opens_and_closes_its_list(self):
        for style in STYLES:
            c = component(GREETER, style)
            with self.subTest(style=style):
                self.assertIn('root.picking = "session";', c)
                self.assertIn('root.picking = "person";', c)
                lst = block(c, 'root.picking != "": PickList')
                # a pick goes to Rust; closed or picked, the field has the keyboard again
                self.assertIn("Greet.pick-session(i);", lst)
                self.assertIn("Greet.pick-user(i);", lst)
                self.assertIn("pw.focus-it();", block(lst, "closed =>"))
                self.assertIn('root.picking = "";', block(lst, "closed =>"))

    def test_the_list_keeps_the_keyboard_while_open(self):
        p = component(GREETER, "PickList")
        self.assertIn("root.focus();", block(p, "init =>"))
        keys = block(p, "key-pressed(e) =>")
        self.assertRegex(keys, r"Key\.Escape \{\s*root\.closed\(\);")
        # (nothing typed reaches the password behind it)
        self.assertTrue(keys.rstrip().endswith("accept"), keys[-80:])


class Spinner(unittest.TestCase):
    def test_glass_and_plain_show_one_while_checked(self):
        for style in ("GlassStyle", "PlainStyle"):
            with self.subTest(style=style):
                self.assertIn("if Greet.busy: Spin {", component(GREETER, style))
        # Glass's send arrow goes while checked (it looked clickable)
        self.assertIn('visible: !Greet.busy && (Greet.finger-on || pw.text != "");', component(GREETER, "GlassStyle"))


class AdminDialog(unittest.TestCase):
    def test_enter_pairs_only_a_dialog_you_asked_for(self):
        # Enter on a pairing's code: only the dialog's own (a request you
        # started, or one you chose Pair for in its notification; #774)
        self.assertIn("if e.text == Key.Return && pair && !field-shown && ok-shown", POLKIT)
        ask = block(POLKIT_LIB, "BtRequest::Ask { id, path, name, kind, reply, ours } =>")
        self.assertIn("} else if ours {", ask)
        self.assertIn("self.bt_notice(st, id, &kind, &name);", ask)

    def test_details_are_folded_and_never_for_a_pairing(self):
        self.assertIn("if !pair && details.length > 0: Rectangle", POLKIT)
        self.assertIn("if !pair && details-open && details.length > 0: Rectangle", POLKIT)
        self.assertIn("ui.set_details_open(false);", POLKIT_LIB)


if __name__ == "__main__":
    unittest.main()
