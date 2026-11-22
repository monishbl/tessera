//! Turning Wi-Fi and Bluetooth on and off, with the answer read: the
//! command's own (`nmcli radio wifi on|off`, `bluetoothctl power on|off`),
//! then the radio's state read again, and rfkill's (/sys/class/rfkill) when
//! it isn't as asked. A soft block (flight mode, `rfkill block`) is lifted
//! the way Bluetooth's notice already does it (`rfkill unblock <kind>`);
//! a hard block (the computer's wireless key or switch, or its firmware)
//! can't be, and says so. A failure is a notification (`toggle`), and the
//! shell's backends look again so the pages' notices follow.
//! 22 Nov: every toggle was fire-and-forget: blocked, it did nothing at all.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Radio {
    Wifi,
    Bluetooth,
}

impl Radio {
    /// rfkill's type for it (/sys/class/rfkill/*/type), and `rfkill`'s word
    pub fn rfkill_type(self) -> &'static str {
        match self {
            Radio::Wifi => "wlan",
            Radio::Bluetooth => "bluetooth",
        }
    }
    fn rfkill_word(self) -> &'static str {
        match self {
            Radio::Wifi => "wifi",
            Radio::Bluetooth => "bluetooth",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Radio::Wifi => "Wi-Fi",
            Radio::Bluetooth => "Bluetooth",
        }
    }
}

/// What rfkill says about a radio
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    None,
    /// in software (flight mode, `rfkill block`): can be lifted
    Soft,
    /// by the computer's switch, key or firmware: only there
    Hard,
}

/// rfkill's entries ((type, soft, hard), one per switch) for `kind`: hard
/// when any of its switches is hard-blocked, else soft when any is soft
pub fn block_in(entries: &[(String, bool, bool)], kind: &str) -> Block {
    let mine = entries.iter().filter(|(t, _, _)| t == kind);
    let (mut soft, mut hard) = (false, false);
    for (_, s, h) in mine {
        soft |= *s;
        hard |= *h;
    }
    if hard {
        Block::Hard
    } else if soft {
        Block::Soft
    } else {
        Block::None
    }
}

/// /sys/class/rfkill's switches: (type, soft, hard)
pub fn read_rfkill() -> Vec<(String, bool, bool)> {
    let Ok(dir) = std::fs::read_dir("/sys/class/rfkill") else { return Vec::new() };
    let read = |p: &std::path::Path, f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
    dir.flatten()
        .map(|e| e.path())
        .map(|p| (read(&p, "type"), read(&p, "soft") == "1", read(&p, "hard") == "1"))
        .filter(|(t, _, _)| !t.is_empty())
        .collect()
}

pub fn block(radio: Radio) -> Block {
    block_in(&read_rfkill(), radio.rfkill_type())
}

/// What the person reads when it's hard-blocked
pub fn hard_text(radio: Radio) -> String {
    format!("{} was turned off by the switch on the computer. Turn it on with the computer's wireless key or switch (or in its firmware settings), then try again.", radio.label())
}

/// What a toggle came to: `now_on` its state read again (None: couldn't be
/// read), `block` rfkill's after it, `said` the command's own output
pub fn verdict(radio: Radio, want_on: bool, now_on: Option<bool>, block: Block, said: &str) -> Result<(), String> {
    if now_on == Some(want_on) {
        return Ok(());
    }
    // (off is what a block makes it: asked off, that's done)
    if !want_on && block != Block::None {
        return Ok(());
    }
    let name = radio.label();
    let low = said.to_lowercase();
    Err(match block {
        Block::Hard => hard_text(radio),
        Block::Soft => format!("{name} is blocked (as in flight mode) and couldn't be unblocked. Run `rfkill unblock {}`, then try again.", radio.rfkill_word()),
        Block::None if radio == Radio::Bluetooth && (low.contains("no default controller") || low.contains("not available")) => "No Bluetooth adapter answered. Check that Bluetooth is running, then try again.".to_string(),
        Block::None if radio == Radio::Wifi && (low.contains("not running") || low.contains("could not be found") || low.contains("not found")) => "NetworkManager didn't answer, so Wi-Fi couldn't be changed. Start NetworkManager, then try again.".to_string(),
        Block::None if low.contains("not authorized") || low.contains("not allowed") || low.contains("permission") => format!("This account isn't allowed to turn {name} {}.", if want_on { "on" } else { "off" }),
        Block::None => format!("Couldn't turn {name} {}. Try again.", if want_on { "on" } else { "off" }),
    })
}

/// `nmcli radio wifi`'s answer read ("enabled" / "disabled")
pub fn wifi_on_in(out: &str) -> Option<bool> {
    match out.trim() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

/// `bluetoothctl show`'s Powered line read
pub fn powered_in(show: &str) -> Option<bool> {
    show.lines().map(str::trim).find_map(|l| match l {
        "Powered: yes" => Some(true),
        "Powered: no" => Some(false),
        _ => None,
    })
}

/// A command's exit code and output (stdout and stderr), given up after `secs`
fn run(cmd: &[&str], secs: u64) -> (Option<i32>, String) {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut c = Command::new(cmd[0]);
    crate::util::plain_locale(&mut c);
    let Ok(mut child) = c.args(&cmd[1..]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
        return (None, String::new());
    };
    let end = std::time::Instant::now() + Duration::from_secs(secs);
    let mut code = None;
    while std::time::Instant::now() < end {
        if let Ok(Some(st)) = child.try_wait() {
            code = st.code();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if code.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let mut out = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut out);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut out);
    }
    (code, out)
}

/// Its state now (None: it couldn't be read)
fn state(radio: Radio) -> Option<bool> {
    match radio {
        Radio::Wifi => wifi_on_in(&run(&["nmcli", "radio", "wifi"], 5).1),
        Radio::Bluetooth => powered_in(&super::bluez::ctl_full(&["show"], 5).1),
    }
}

/// Turn it on or off and read the answer (blocking: call on a thread)
pub fn set(radio: Radio, on: bool) -> Result<(), String> {
    if on {
        match block(radio) {
            Block::Hard => return Err(hard_text(radio)),
            // (lifted as Bluetooth's notice does: rfkill unblock; the
            // adapter comes back a moment later)
            Block::Soft => {
                run(&["rfkill", "unblock", radio.rfkill_word()], 5);
                std::thread::sleep(Duration::from_millis(800));
            }
            Block::None => {}
        }
    }
    let word = if on { "on" } else { "off" };
    let (_, said) = match radio {
        Radio::Wifi => run(&["nmcli", "radio", "wifi", word], 15),
        Radio::Bluetooth => super::bluez::ctl_full(&["power", word], 10),
    };
    let mut now = state(radio);
    if now != Some(on) {
        // (BlueZ and NetworkManager change it a moment after answering)
        std::thread::sleep(Duration::from_millis(700));
        now = state(radio);
    }
    verdict(radio, on, now, block(radio), &said)
}

/// `set` on a thread; a failure is a notification, and the shell's backends
/// look again (the pages' Bluetooth and Wi-Fi notices follow)
pub fn toggle(radio: Radio, on: bool) {
    std::thread::spawn(move || {
        if let Err(why) = set(radio, on) {
            eprintln!("tessera-ui: {} {}: {why}", radio.label(), if on { "on" } else { "off" });
            let icon = match radio {
                Radio::Wifi => "network-wireless-disabled",
                Radio::Bluetooth => "bluetooth-disabled",
            };
            let title = format!("{} didn't turn {}", radio.label(), if on { "on" } else { "off" });
            crate::util::notify_live("Tessera", icon, "normal", &crate::util::Live::key(&format!("radio-{}", radio.rfkill_word()), 1), &title, &why);
        }
        // (standalone only: the host's Probes; the QML's Backends look by themselves)
        crate::host::route_ask("backends.recheck", &serde_json::json!([]));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(t: &str, soft: bool, hard: bool) -> (String, bool, bool) {
        (t.to_string(), soft, hard)
    }

    #[test]
    fn rfkill_is_read_per_kind() {
        let list = vec![e("wlan", false, false), e("bluetooth", true, false), e("bluetooth", false, false)];
        assert_eq!(block_in(&list, "wlan"), Block::None);
        assert_eq!(block_in(&list, "bluetooth"), Block::Soft);
        // (a hard block wins over a soft one)
        let list = vec![e("wlan", true, true), e("wlan", true, false)];
        assert_eq!(block_in(&list, "wlan"), Block::Hard);
        assert_eq!(block_in(&[], "wlan"), Block::None);
    }

    #[test]
    fn states_are_read_back() {
        assert_eq!(wifi_on_in("enabled\n"), Some(true));
        assert_eq!(wifi_on_in("disabled\n"), Some(false));
        assert_eq!(wifi_on_in("Error: NetworkManager is not running.\n"), None);
        assert_eq!(powered_in("Controller 00:00 (public)\n\tPowered: yes\n\tPairable: no\n"), Some(true));
        assert_eq!(powered_in("\tPowered: no\n"), Some(false));
        assert_eq!(powered_in("No default controller available\n"), None);
    }

    #[test]
    fn a_toggle_says_why_it_didnt() {
        assert_eq!(verdict(Radio::Wifi, true, Some(true), Block::None, ""), Ok(()));
        assert_eq!(verdict(Radio::Bluetooth, false, Some(false), Block::None, ""), Ok(()));
        // (asked off while blocked: off it is)
        assert_eq!(verdict(Radio::Bluetooth, false, None, Block::Hard, ""), Ok(()));
        assert!(verdict(Radio::Wifi, true, Some(false), Block::Hard, "").unwrap_err().contains("switch on the computer"));
        assert!(verdict(Radio::Bluetooth, true, Some(false), Block::Soft, "").unwrap_err().contains("rfkill unblock bluetooth"));
        assert!(verdict(Radio::Bluetooth, true, None, Block::None, "No default controller available\n").unwrap_err().contains("No Bluetooth adapter"));
        assert!(verdict(Radio::Wifi, true, None, Block::None, "Error: NetworkManager is not running.\n").unwrap_err().contains("Start NetworkManager"));
        assert!(verdict(Radio::Wifi, false, Some(true), Block::None, "Error: Not authorized to control networking.\n").unwrap_err().contains("isn't allowed to turn Wi-Fi off"));
        assert_eq!(verdict(Radio::Bluetooth, true, Some(false), Block::None, "Failed to set power on: org.bluez.Error.Failed\n"), Err("Couldn't turn Bluetooth on. Try again.".to_string()));
    }

    #[test]
    fn the_hard_block_names_the_switch() {
        assert!(hard_text(Radio::Wifi).starts_with("Wi-Fi was turned off by the switch on the computer"));
        assert!(hard_text(Radio::Bluetooth).starts_with("Bluetooth was turned off by the switch on the computer"));
    }
}
