"""fastpair.py: Fast Pair service data (Google's public spec): a model ID in
pairing mode, and batteries (left, right, case) from the other adverts."""
import importlib.util
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "shell/assets/scripts/fastpair.py"
spec = importlib.util.spec_from_file_location("fastpair", SCRIPT)
fp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fp)


class Parse(unittest.TestCase):
    def test_pairing_mode(self):
        self.assertEqual(fp.parse(bytes([0x2C, 0xC1, 0x6A])), {"model": "2CC16A", "pairing": True})

    def test_batteries_after_the_key_filter(self):
        # flags, account key filter (len 4, type 0), salt (len 1, type 1), battery (len 3, type 3)
        data = bytes([0x00, 0x40, 1, 2, 3, 4, 0x11, 0x99, 0x33, 0x80 | 85, 70, 0x7F])
        r = fp.parse(data)
        self.assertFalse(r["pairing"])
        self.assertEqual(r["battery"], {"left": {"level": 85, "charging": True}, "right": {"level": 70, "charging": False}})
        self.assertTrue(r["showUi"])

    def test_hidden_battery(self):
        r = fp.parse(bytes([0x00, 0x34, 50, 50, 0x80 | 20]))
        self.assertFalse(r["showUi"])
        self.assertEqual(r["battery"]["case"], {"level": 20, "charging": True})

    def test_levels_over_100_are_not_levels(self):
        r = fp.parse(bytes([0x00, 0x33, 126, 101, 100]))
        self.assertEqual(r["battery"], {"case": {"level": 100, "charging": False}})

    def test_known_is_worked_out_once_per_snapshot(self):
        w = object.__new__(fp.Watcher)
        w.cached, w.known_cache, asked = (0, {}), {}, []
        objs = {"/d/x": {"org.bluez.Device1": {"Address": "11:22:33:44:55:66"}}}
        w.objects = lambda: (asked.append(1), objs)[1]
        self.assertIsNone(w.known("/d/x", 60))
        self.assertIsNone(w.known("/d/x", 60))
        self.assertEqual(len(asked), 1)

    def test_no_battery(self):
        self.assertEqual(fp.parse(bytes([0x00, 0x20, 9, 9])), {"pairing": False})

    def test_truncated(self):
        self.assertEqual(fp.parse(bytes([0x00, 0x00, 0x33, 60])), {"pairing": False, "battery": {"left": {"level": 60, "charging": False}}, "showUi": True})


class Signals(unittest.TestCase):
    """The bus is asked about a device only when an advert can't say."""

    class Params:
        def __init__(self, v):
            self.v = v

        def unpack(self):
            return self.v

    def setUp(self):
        self.w = object.__new__(fp.Watcher)
        self.w.devices, self.w.other, self.asked = {}, set(), []
        props = {"/d/buds": {"Address": "AA:BB", "Alias": "Buds", "RSSI": -50, "ServiceData": {fp.FAST_PAIR: bytes([0x2C, 0xC1, 0x6A])}},
                 "/d/tv": {"Address": "CC:DD", "RSSI": -40, "ServiceData": {}}}
        self.w.call = lambda path, *a, **k: (self.asked.append(path), (props[path],))[1]

    def changed(self, path, **props):
        self.w.on_signal(None, None, path, None, "PropertiesChanged", self.Params(("org.bluez.Device1", props, [])))

    def test_unknown_then_known(self):
        self.changed("/d/buds", RSSI=-50)
        self.assertEqual(self.asked, ["/d/buds"])
        self.assertTrue(self.w.devices["/d/buds"]["pairing"])
        self.changed("/d/buds", RSSI=-45)  # (known: no question)
        self.assertEqual(self.asked, ["/d/buds"])
        self.assertEqual(self.w.devices["/d/buds"]["rssi"], -45)

    def test_other_devices_are_asked_about_once(self):
        for rssi in (-40, -41, -42):
            self.changed("/d/tv", RSSI=rssi)
        self.assertEqual(self.asked, ["/d/tv"])
        self.changed("/d/tv", ServiceData={"0000180f-0000-1000-8000-00805f9b34fb": b"x"})
        self.assertEqual(self.asked, ["/d/tv"])
        self.assertEqual(self.w.devices, {})


if __name__ == "__main__":
    unittest.main()


class ClassicTwin(unittest.TestCase):
    """Connect pairs the earbuds' classic side: the LE advert has no name
    (BlueZ calls it by its address), and pairing with it is refused."""
    AUDIO = 0x240404

    def dev(self, address, name=None, paired=False, rssi=-50, cls=AUDIO):
        return ("/org/bluez/hci0/dev_" + address.replace(":", "_"),
                {"Address": address, "Alias": name or address.replace(":", "-"), "Name": name, "Paired": paired, "RSSI": rssi, "Class": cls})

    def test_an_address_is_not_a_name(self):
        self.assertEqual(fp.real_name("98-34-8C-03-82-DC"), "")
        self.assertEqual(fp.real_name("realme Buds Air 5"), "realme Buds Air 5")

    def test_never_by_name_alone(self):
        # (audit #128: any radio can advertise a name; only the address proves the device)
        devs = [self.dev("11:22:33:44:55:66", "Other buds"), self.dev("AA:BB:CC:00:00:01", "realme Buds Air 5")]
        self.assertIsNone(fp.pick_twin(devs, "realme Buds Air 5", "98:34:8C:03:82:DC"))

    def test_by_address_tail_when_nameless(self):
        # (the user's buds: LE 98:34:8C:03:82:DC, the classic side shares 03:82:DC)
        devs = [self.dev("11:22:33:44:55:66", "Other buds"), self.dev("29:46:8C:03:82:DC", "Buds")]
        self.assertEqual(fp.pick_twin(devs, "98-34-8C-03-82-DC", "98:34:8C:03:82:DC"), devs[1][0])

    def test_paired_and_non_audio_are_skipped(self):
        devs = [self.dev("29:46:8C:03:82:DC", "Buds", paired=True), self.dev("AA:46:8C:03:82:DC", "Phone", cls=0x5a020c)]
        self.assertIsNone(fp.pick_twin(devs, "", "98:34:8C:03:82:DC", fallback=True))

    def test_never_the_one_close_by(self):
        # (audit #128: "the one audio device near" is a stranger's as easily as yours)
        one = [self.dev("11:22:33:44:55:66", "Buds X", rssi=-55), self.dev("11:22:33:44:55:77", "Far", rssi=-90)]
        self.assertIsNone(fp.pick_twin(one, "", "98:34:8C:03:82:DC", fallback=True))


class AlreadyPaired(unittest.TestCase):
    """The realme Buds T100 in pairing mode: a nameless advert from an
    address of its own that names realme's company ID; the buds paired
    before are 98:34:8C:78:20:94 (as seen on 1 Nov)."""
    BASE = "-0000-1000-8000-00805f9b34fb"
    ADVERT = {"Address": "04:29:8C:03:82:DC", "Alias": "04-29-8C-03-82-DC", "Paired": False,
              "UUIDs": ["000005d6" + BASE, "0000fe2c" + BASE]}
    BUDS = {"Address": "98:34:8C:78:20:94", "Alias": "realme TechLife Buds T100", "Paired": True,
            "Modalias": "bluetooth:v05D6p000Ad0240", "UUIDs": ["0000110b" + BASE]}
    SONY = {"Address": "A8:E6:E8:0B:C3:55", "Alias": "WH-CH720N", "Paired": True,
            "Modalias": "usb:v054Cp0EAFd0114", "UUIDs": ["0000110b" + BASE, "0000fe2c" + BASE]}

    def devices(self, *extra):
        return [("/d/advert", self.ADVERT), ("/d/buds", self.BUDS), ("/d/sony", self.SONY)] + list(extra)

    def test_by_the_company_it_names(self):
        self.assertEqual(fp.advert_vendors(self.ADVERT), {0x05D6})
        self.assertEqual(fp.device_vendor(self.BUDS), 0x05D6)
        self.assertIsNone(fp.device_vendor(self.SONY))  # (a USB vendor ID, another numbering)
        self.assertEqual(fp.pick_known(self.devices(), self.ADVERT), "/d/buds")

    def test_two_of_that_company_is_no_answer(self):
        other = dict(self.BUDS, Address="11:22:33:44:55:66", Alias="realme Buds Air")
        self.assertIsNone(fp.pick_known(self.devices(("/d/other", other)), self.ADVERT))

    def test_the_advert_itself_paired(self):
        self.assertEqual(fp.pick_known(self.devices(), self.SONY), "/d/sony")

    def test_by_the_front_of_the_address(self):
        advert = dict(self.ADVERT, Address="98:34:8C:03:82:DC", UUIDs=["0000fe2c" + self.BASE])
        self.assertEqual(fp.pick_known(self.devices(), advert), "/d/buds")

    def test_unpaired_earbuds_are_not_known(self):
        self.assertIsNone(fp.pick_known(self.devices(), dict(self.ADVERT, Address="11:11:11:11:11:11", UUIDs=[])))


class Forgetting(unittest.TestCase):
    """Devices BlueZ removes are forgotten (addresses rotate: `other` grew
    by every phone and watch that ever passed)."""

    def test_removed_devices_are_forgotten(self):
        w = object.__new__(fp.Watcher)
        w.devices, w.other = {"/d/buds": {"at": 0}}, {"/d/tv", "/d/watch"}
        P = Signals.Params
        w.on_signal(None, None, "/", None, "InterfacesRemoved", P(("/d/tv", ["org.bluez.Device1"])))
        w.on_signal(None, None, "/", None, "InterfacesRemoved", P(("/d/buds", ["org.bluez.Device1", "org.bluez.Battery1"])))
        self.assertEqual(w.other, {"/d/watch"})
        self.assertEqual(w.devices, {})


class AudioAfterConnect(unittest.TestCase):
    """Connect answering AlreadyConnected (Fast Pair's own LE link was up)
    still connects the audio profiles: 26 Nov, headphones connected with no
    sound until ConnectProfile(A2DP) was asked."""

    class Bus:
        def __init__(self, connect_error=None):
            self.calls = []
            self.connect_error = connect_error

        def __call__(self, path, iface, method, args=None, sig=None, timeout=10000):
            self.calls.append((method, args))
            if method == "GetAll":
                return ({"Address": "A8:E6:E8:0B:C3:55", "UUIDs": [
                    "00001108-0000-1000-8000-00805f9b34fb", "0000110b-0000-1000-8000-00805f9b34fb",
                    "0000110e-0000-1000-8000-00805f9b34fb", "0000111e-0000-1000-8000-00805f9b34fb"]},)
            if method == "Connect" and self.connect_error:
                raise RuntimeError(self.connect_error)
            if method == "ConnectProfile" and args == ("00001108-0000-1000-8000-00805f9b34fb",):
                raise RuntimeError("org.bluez.Error.AlreadyConnected")
            return ()

    def watcher(self, bus):
        w = fp.Watcher.__new__(fp.Watcher)
        w.call = bus
        w.GLib = type("GLib", (), {"Variant": staticmethod(lambda *a: a)})
        return w

    def profiles(self, bus):
        return [a[0] for m, a in bus.calls if m == "ConnectProfile"]

    def test_already_connected_still_connects_the_audio(self):
        bus = self.Bus("GDBus.Error:org.bluez.Error.AlreadyConnected: Already Connected")
        r = self.watcher(bus).connect_known("/org/bluez/hci0/dev_A8_E6_E8_0B_C3_55")
        self.assertTrue(r["ok"])
        self.assertEqual(self.profiles(bus), list(fp.CONNECT_UUIDS))

    def test_a_plain_connect_connects_the_audio_too(self):
        bus = self.Bus()
        r = self.watcher(bus).connect_known("/org/bluez/hci0/dev_A8_E6_E8_0B_C3_55")
        self.assertEqual(r, {"ok": True, "error": "", "address": "A8:E6:E8:0B:C3:55"})
        self.assertEqual(self.profiles(bus)[0], "0000110b-0000-1000-8000-00805f9b34fb")

    def test_no_audio_profiles_no_profile_calls(self):
        bus = self.Bus()
        self.watcher(bus).connect_audio("/d", {"UUIDs": ["00001124-0000-1000-8000-00805f9b34fb"]})
        self.assertEqual(self.profiles(bus), [])
