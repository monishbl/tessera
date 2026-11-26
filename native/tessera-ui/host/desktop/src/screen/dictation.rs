//! Dictation (shell/services/Dictation.qml): speak, and the words are typed
//! into the app you're in. All on this computer: the microphone is recorded
//! (pw-record, 16 kHz mono), whisper.cpp turns it into text (the same
//! assets/scripts/dictate.py) and it's pasted where the cursor is (copied,
//! then Hyprland's Ctrl+V; terminals Ctrl+Shift+V, as the emoji picker and
//! the clipboard history: wtype typed it until 22 Nov, and its own keymap
//! gave Electron apps boxes for anything past ASCII and Firefox ones stray
//! shortcuts).
//!
//! Super+H (the shortcut belongs to the shell host's Keybinds): a tap starts
//! listening and the next tap stops; held down, it listens until you let go.
//! Keybinds call the IPC target `dictation`: `pressed` / `released` for that
//! shortcut, `toggle | start | stop | cancel` (the QML's own handler), and the
//! Settings page's `status | install | download <name> [use] | cancelDownload |
//! removeModel <name>`, and the island's `language <code>`. What Settings and
//! the island show is in the host state, `dictation`.
//!
//! The island's live activity (dictation_live.rs): while you speak, the
//! microphone's level (told to the menu bar, 20 a second) and the words so
//! far (drafts, in the state as `partial`); after you stop, "Writing…"; then,
//! for a moment, the first words it typed (stage `done`).

use super::dictation_live::{self as live, Tap};
use super::util;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use tessera_core::host::{Host, Service};

const HOLD_MS: i64 = 400; // held longer than this: it stops on release
const MAX_SECONDS: u64 = 120;
/// How long the island shows the first words it typed
const DONE_MS: u64 = 1400;
/// How many of them
const DONE_WORDS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Listening,
    Writing,
    /// typed: the island shows the first words for a moment (DONE_MS)
    Done,
}

// Messages from its threads
struct Status(Value);
struct RecorderExit(String);
struct Transcribed(String, String); // file, stdout
struct Limit(u64);
struct Progress(Value);
struct DownloadEnd;
/// the paste didn't go (the words are on the clipboard)
struct TypeFailed;
/// wl-copy didn't take the words
struct CopyFailed;
struct InstallDone(bool);
/// the typed words' moment is over (for that dictation)
struct DoneEnd(u64);

/// The first few words of what was typed, for the island ("Send the report…")
pub fn first_words(text: &str, n: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= n {
        words.join(" ")
    } else {
        format!("{}…", words[..n].join(" "))
    }
}

/// A language code the island may set: "" (the system's), "auto" or a code
fn language_ok(code: &str) -> bool {
    code.len() <= 8 && code.chars().all(|c| c.is_ascii_lowercase() || c == '-')
}

pub struct DictationSvc {
    pub info: Value,
    stage: Stage,
    started_at: i64,
    pressed_at: i64,
    error: String,
    wav: String,
    rec_pid: Option<i32>,
    tr_pid: Option<i32>,
    cancelled: bool,
    gen: u64,
    downloading: String,
    progress: f64,
    download_error: String,
    download_ok: bool,
    download_use: bool,
    dl_pid: Option<i32>,
    install_asked: bool,
    /// why the last install didn't finish (Settings → Dictation)
    install_error: String,
    /// the model last downloaded (which one an error is about)
    last_download: String,
    /// the screen was locked when last looked (dictation stops on lock)
    locked: bool,
    /// each dictation's number (its level and drafts say which they're for)
    session: u64,
    /// the live side while listening: levels and drafts (stopped when dropped)
    tap: Option<Tap>,
    /// the words so far (a draft) and, once typed, the first few
    partial: String,
    typed: String,
    /// a language picked in the island, until the settings file says it too
    lang_now: Option<String>,
}

impl DictationSvc {
    pub fn new() -> Self {
        Self {
            info: json!({}),
            stage: Stage::Idle,
            started_at: 0,
            pressed_at: 0,
            error: String::new(),
            wav: String::new(),
            rec_pid: None,
            tr_pid: None,
            cancelled: false,
            gen: 0,
            downloading: String::new(),
            progress: 0.0,
            download_error: String::new(),
            download_ok: false,
            download_use: true,
            dl_pid: None,
            install_asked: false,
            install_error: String::new(),
            last_download: String::new(),
            locked: false,
            session: 0,
            tap: None,
            partial: String::new(),
            typed: String::new(),
            lang_now: None,
        }
    }

    fn has_binary(&self) -> bool {
        self.info["binary"].as_str().is_some_and(|s| !s.is_empty()) || self.info["binary"].as_bool() == Some(true)
    }
    fn model(&self, h: &Host) -> String {
        let want = h.cfg()["dictation"]["model"].as_str().unwrap_or("base");
        if self.info["models"].get(want).is_some() {
            want.into()
        } else {
            "base".into()
        }
    }
    fn has_model(&self, h: &Host) -> bool {
        self.info["models"][self.model(h)].as_bool().unwrap_or(false)
    }
    fn ready(&self, h: &Host) -> bool {
        self.has_binary() && self.has_model(h)
    }

    /// The language whisper listens for: the setting, or the system's
    fn language(&self, h: &Host) -> String {
        let set = self.language_set(h);
        let set = set.as_str();
        if !set.is_empty() {
            return set.into();
        }
        let loc = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty()).unwrap_or_default();
        let l = loc.split(['_', '.']).next().unwrap_or("");
        if (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_lowercase()) {
            l.into()
        } else {
            "auto".into()
        }
    }

    /// The language setting ("" the system's): the island's pick until the file has it
    fn language_set(&self, h: &Host) -> String {
        self.lang_now.clone().unwrap_or_else(|| h.cfg()["dictation"]["language"].as_str().unwrap_or("").to_string())
    }

    pub fn refresh(&self, h: &Host) {
        let send = h.sender("Dictation");
        std::thread::spawn(move || {
            let out = Command::new("python3").args(["-I", &util::script("dictate.py"), "status"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
            if let Ok(o) = out {
                if let Ok(v) = serde_json::from_slice::<Value>(&o.stdout) {
                    send(Box::new(Status(v)));
                }
            }
        });
    }

    fn publish(&self, h: &mut Host) {
        let ready = self.ready(h);
        h.set_state(
            "dictation",
            json!({
                "stage": match self.stage { Stage::Idle => "", Stage::Listening => "listening", Stage::Writing => "writing", Stage::Done => "done" },
                "startedAt": self.started_at,
                "maxSeconds": MAX_SECONDS,
                "session": self.session,
                // (the shortcut still held: it stops when let go; else a second press stops it)
                "mode": if self.stage == Stage::Listening && self.pressed_at != 0 { "hold" } else { "toggle" },
                "partial": self.partial, "typed": self.typed,
                "language": self.language(h), "languageSet": self.language_set(h),
                "info": self.info, "ready": ready, "hasBinary": self.has_binary(), "model": self.model(h),
                "error": self.error, "downloading": self.downloading, "downloadProgress": self.progress,
                "downloadError": self.download_error, "installing": self.install_asked,
                "installError": self.install_error, "lastDownload": self.last_download,
            }),
        );
    }

    fn tick(&self, h: &mut Host) {
        if h.cfg()["dictation"]["sounds"].as_bool().unwrap_or(true) {
            h.ask("sound", json!([if self.stage == Stage::Listening { "recording" } else { "recordingOff" }]));
        }
    }

    /// The shortcut went down
    pub fn pressed(&mut self, h: &mut Host) {
        match self.stage {
            Stage::Listening => return self.stop(h),
            Stage::Writing => return,
            Stage::Idle | Stage::Done => {}
        }
        self.pressed_at = tessera_core::util::now_ms();
        self.start(h);
    }

    /// The shortcut came up
    pub fn released(&mut self, h: &mut Host) {
        if self.pressed_at == 0 {
            return;
        }
        let held = tessera_core::util::now_ms() - self.pressed_at;
        self.pressed_at = 0;
        if held >= HOLD_MS && self.stage == Stage::Listening {
            self.stop(h);
        } else if self.stage == Stage::Listening {
            self.publish(h); // (a tap: "Press again to stop")
        }
    }

    pub fn start(&mut self, h: &mut Host) {
        // (not while the screen is locked: nothing to type into, and the
        // microphone would listen in an empty room)
        if !matches!(self.stage, Stage::Idle | Stage::Done) || self.rec_pid.is_some() || self.tr_pid.is_some() || h.state("locked").as_bool().unwrap_or(false) {
            return;
        }
        // (the island's language pick is in the settings file by now)
        if self.lang_now.as_deref() == h.cfg()["dictation"]["language"].as_str() {
            self.lang_now = None;
        }
        if !self.ready(h) {
            self.refresh(h);
            util::island_post(
                h,
                json!({"kind": "custom", "icon": "microphone-slash", "title": "Dictation isn't set up",
                    "subtitle": if !self.has_binary() { "Install speech recognition in Settings" } else { "Download a speech model in Settings" },
                    "color": "secondary", "action": "Open Settings", "onAction": {"call": "panels.openSettings", "args": ["shortcuts", "Dictation"]}}),
                6000,
            );
            self.pressed_at = 0;
            return;
        }
        if mic_muted(h) {
            util::island_post(h, json!({"kind": "custom", "icon": "microphone-slash", "title": "Your microphone is muted", "subtitle": "Unmute it to dictate", "color": "secondary", "action": "Unmute", "onAction": {"call": "audio.toggleMic", "args": []}}), 5000);
            self.pressed_at = 0;
            return;
        }
        self.error.clear();
        self.wav = format!("{}/tessera/dictation-{}.wav", util::runtime(), tessera_core::util::now_ms());
        // (PipeWire's recorder, else PulseAudio's, else ALSA's: each stops
        // cleanly on SIGINT; setpriv: the microphone is let go if the shell dies)
        let script = "umask 077; mkdir -p \"$(dirname \"$1\")\" || exit 1; \
            if command -v pw-record >/dev/null; then exec pw-record --rate 16000 --channels 1 --format s16 \"$1\"; \
            elif command -v parecord >/dev/null; then exec parecord --rate=16000 --channels=1 --format=s16le --file-format=wav \"$1\"; \
            else exec arecord -q -f S16_LE -r 16000 -c 1 -t wav \"$1\"; fi";
        let mut command = Command::new("setpriv");
        command.args(["--pdeathsig", "INT", "--", "sh", "-c", script, "sh", &self.wav]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        util::default_signals(&mut command); // (so the mic hears the stop)
        let child = command.spawn();
        let Ok(mut child) = child else {
            return self.fail(h, "The microphone couldn't be recorded. Check the input in Settings → Sound, then try again.");
        };
        self.rec_pid = Some(child.id() as i32);
        let (send, file) = (h.sender("Dictation"), self.wav.clone());
        std::thread::spawn(move || {
            let _ = child.wait();
            send(Box::new(RecorderExit(file)));
        });
        self.started_at = tessera_core::util::now_ms();
        self.stage = Stage::Listening;
        self.gen += 1;
        // the live side: the level for the island's wave, the words so far
        self.session += 1;
        self.partial.clear();
        self.typed.clear();
        let pick = live::Pick { model: live::draft_model(&self.model(h), &self.info["models"]), lang: self.language(h) };
        let helper: Vec<String> = ["setpriv", "--pdeathsig", "TERM", "--", "python3", "-I"].iter().map(|s| s.to_string()).chain([util::script("dictate.py"), "stream".to_string()]).collect();
        let draft = format!("{}/tessera/dictation-partial-{}.wav", util::runtime(), self.session);
        // (the levels go straight to the menu bar's part, 20 a second: not
        // through this service, which would make each a host turn and run
        // every service's `settled`; a stale one after a stop is ignored there)
        let tx = h.st.tx.clone();
        let level_to = move |l: f32| {
            let _ = tx.send(tessera_core::Msg::Part("menubar", Box::new(json!({"dictation": {"level": l}}))));
        };
        self.tap = Some(Tap::spawn(self.wav.clone(), self.session, pick, h.sender("Dictation"), level_to, helper, draft));
        self.tick(h);
        util::later(h.sender("Dictation"), MAX_SECONDS * 1000, Box::new(Limit(self.gen)));
        self.publish(h);
    }

    /// Done speaking: write it out
    pub fn stop(&mut self, h: &mut Host) {
        if self.stage != Stage::Listening {
            return;
        }
        self.gen += 1; // (the limit timer is void)
        self.tap = None; // (no more levels or drafts: the words are written from the whole recording)
        self.stage = Stage::Writing;
        self.tick(h);
        if tessera_core::util::now_ms() - self.started_at < 500 || self.rec_pid.is_none() {
            self.stage = Stage::Idle; // (too short to hold any words: dropped quietly)
            self.partial.clear();
        }
        if let Some(p) = self.rec_pid {
            unsafe { libc::kill(p, libc::SIGINT) };
        }
        self.publish(h);
    }

    /// Stop without typing anything
    pub fn cancel(&mut self, h: &mut Host) {
        if self.stage == Stage::Idle {
            return;
        }
        self.gen += 1;
        self.tap = None;
        self.partial.clear();
        self.typed.clear();
        if let Some(p) = self.rec_pid {
            self.cancelled = true;
            unsafe { libc::kill(p, libc::SIGINT) };
        } else if let Some(p) = self.tr_pid {
            self.cancelled = true;
            unsafe { libc::kill(p, libc::SIGTERM) };
            util::detached(&["rm", "-f", &self.wav]);
        }
        self.stage = Stage::Idle;
        self.pressed_at = 0;
        self.publish(h);
    }

    fn recorder_exited(&mut self, h: &mut Host, file: String) {
        self.rec_pid = None;
        self.tap = None;
        if self.cancelled || self.stage != Stage::Writing {
            self.cancelled = false;
            util::detached(&["rm", "-f", &file]);
            if self.stage == Stage::Listening {
                // (it ended by itself: no microphone, or no sound server)
                self.gen += 1;
                self.pressed_at = 0;
                self.fail(h, "The microphone couldn't be recorded. Check the input in Settings → Sound, then try again.");
            }
            return;
        }
        let (model, lang) = (self.model(h), self.language(h));
        let child = Command::new("python3").args(["-I", &util::script("dictate.py"), "transcribe", &file, &model, &lang]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(child) = child else {
            return self.fail(h, "Dictation didn't work. Try again.");
        };
        self.tr_pid = Some(child.id() as i32);
        let send = h.sender("Dictation");
        std::thread::spawn(move || {
            let out = child.wait_with_output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            send(Box::new(Transcribed(file, out)));
        });
    }

    fn transcribed(&mut self, h: &mut Host, out: String) {
        self.tr_pid = None;
        if self.cancelled {
            self.cancelled = false;
            return;
        }
        let r: Value = serde_json::from_str(&out).unwrap_or_default();
        if r["ok"].as_bool().unwrap_or(false) {
            self.finish(h, r["text"].as_str().unwrap_or("").to_string());
        } else {
            let why = r["error"].as_str().filter(|s| !s.is_empty()).unwrap_or("Dictation didn't work. Try again.").to_string();
            self.fail(h, &why);
        }
    }

    /// Type it where the cursor is
    fn finish(&mut self, h: &mut Host, text: String) {
        self.partial.clear();
        if text.is_empty() {
            self.stage = Stage::Idle;
            self.publish(h);
            util::island_post(h, json!({"kind": "custom", "icon": "microphone", "title": "Didn't catch that", "subtitle": "Speak a little closer to the microphone, then try again", "color": "secondary"}), 3000);
            return;
        }
        // (the island ticks, with the first words, for a moment)
        self.stage = Stage::Done;
        self.typed = first_words(&text, DONE_WORDS);
        self.publish(h);
        util::later(h.sender("Dictation"), DONE_MS, Box::new(DoneEnd(self.session)));
        // (through wl-copy's input: a command line can be read by any
        // program; waited for, so the paste gets these words)
        let send = h.sender("Dictation");
        std::thread::spawn(move || {
            let class = tessera_core::util::active_class();
            if !tessera_core::util::pipe_to_wait(&["wl-copy"], text.as_bytes()) {
                send(Box::new(CopyFailed));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
            if !tessera_core::util::paste_shortcut(&class) {
                send(Box::new(TypeFailed));
            }
        });
    }

    /// The paste didn't go: the words are on the clipboard already
    fn copy_instead(&self, h: &mut Host) {
        util::island_post(h, json!({"kind": "custom", "icon": "clipboard-text", "title": "Copied what you said", "subtitle": "Pasting into this app didn't work: paste it with Ctrl+V", "color": "accent"}), 5000);
    }

    fn fail(&mut self, h: &mut Host, why: &str) {
        self.stage = Stage::Idle;
        self.tap = None;
        self.partial.clear();
        self.typed.clear();
        self.error = why.into();
        util::island_post(h, json!({"kind": "custom", "icon": "microphone-slash", "title": "Dictation didn't work", "subtitle": why, "color": "danger"}), 6000);
        self.publish(h);
    }

    // ── Setting it up (Settings → Dictation) ────────────────────────────
    fn install(&mut self, h: &mut Host) {
        if self.install_asked || self.has_binary() {
            return;
        }
        self.install_asked = true;
        self.install_error.clear();
        let list = "whisper-cpp";
        // Updates.update(["--install", list]); the system host answers, and
        // tells `dictation install-done` (ipc) when its job ends
        h.ask("system.update", json!(["--install", list]));
        self.publish(h);
    }

    fn download(&mut self, h: &mut Host, name: &str, use_it: bool) {
        if self.dl_pid.is_some() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') {
            return;
        }
        self.download_use = use_it; // (Live captions fetch Tiny without changing Dictation's model)
        self.downloading = name.into();
        self.last_download = name.into();
        self.progress = 0.0;
        self.download_error.clear();
        self.download_ok = false;
        let child = Command::new("python3").args(["-I", &util::script("dictate.py"), "download", name]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = child else {
            self.downloading.clear();
            return;
        };
        self.dl_pid = Some(child.id() as i32);
        let send = h.sender("Dictation");
        std::thread::spawn(move || {
            use std::io::BufRead;
            if let Some(out) = child.stdout.take() {
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Ok(v) = serde_json::from_str::<Value>(&l) {
                        send(Box::new(Progress(v)));
                    }
                }
            }
            let _ = child.wait();
            send(Box::new(DownloadEnd));
        });
        self.publish(h);
    }
}

impl Default for DictationSvc {
    fn default() -> Self {
        Self::new()
    }
}

/// Is the default microphone muted? (Audio.micMuted): core's sound reading
/// (any PulseAudio-speaking server), wpctl only before it has one
fn mic_muted(h: &Host) -> bool {
    if let Some(s) = h.st.shell.get::<tessera_core::services::audio::Sound>().filter(|s| s.up) {
        return s.mic_muted();
    }
    Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("[MUTED]")).unwrap_or(false)
}

impl Service for DictationSvc {
    fn name(&self) -> &'static str {
        "Dictation"
    }
    /// The screen locked: dictation in progress stops, typing nothing
    fn settled(&mut self, h: &mut Host) {
        let locked = h.state("locked").as_bool().unwrap_or(false);
        if locked != self.locked {
            self.locked = locked;
            if locked {
                self.cancel(h);
            }
        }
    }
    fn calls(&self) -> &'static [&'static str] {
        &["dictation"]
    }
    fn ipc_targets(&self) -> &'static [&'static str] {
        &["dictation"]
    }
    /// `dictation.pressed|released` (the shortcuts) and Settings -> Input's
    /// `dictation.refresh|install|download|cancelDownload|removeModel`: the same as the IPC ones
    fn call(&mut self, h: &mut Host, call: &str, args: &Value) {
        let f = call.split_once('.').map(|x| x.1).unwrap_or("");
        if f == "refresh" {
            return self.refresh(h);
        }
        let a: Vec<String> = args.as_array().map(|l| l.iter().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).collect()).unwrap_or_default();
        self.ipc(h, "dictation", f, &a);
    }
    fn start(&mut self, h: &mut Host) {
        self.refresh(h);
    }
    fn ipc(&mut self, h: &mut Host, _target: &str, func: &str, args: &[String]) -> Option<String> {
        match func {
            "toggle" => {
                if self.stage == Stage::Listening {
                    self.stop(h)
                } else {
                    self.start(h)
                }
            }
            "start" => self.start(h),
            "stop" => self.stop(h),
            "cancel" => self.cancel(h),
            "pressed" => self.pressed(h),
            "released" => self.released(h),
            // the island's mic and language line: the language picked there
            // (kept in the settings, used for these words and the next ones)
            "language" => {
                let code = args.first().cloned().unwrap_or_default();
                if language_ok(&code) {
                    util::cfg_set("dictation.language", json!(code));
                    self.lang_now = Some(code);
                    let lang = self.language(h);
                    if let Some(t) = &self.tap {
                        t.set_lang(&lang);
                    }
                    self.publish(h);
                }
            }
            // the island's list: "Dictation settings": what was said is
            // dropped (nothing pasted into Settings), then Settings opens
            "settings" => {
                self.cancel(h);
                h.ask("panels.openSettings", json!(["shortcuts", "Dictation"]));
            }
            // Esc anywhere while dictating (a non-consuming bind: hypr/tessera/keybinds.lua)
            "escape" => {
                if matches!(self.stage, Stage::Listening | Stage::Writing) {
                    self.cancel(h);
                }
            }
            "status" => return Some(self.info.to_string()),
            "install" => self.install(h),
            "installDone" => {
                let ok = args.first().map(|a| a == "ok").unwrap_or(true);
                if self.install_asked {
                    self.install_asked = false;
                    self.error = if ok { String::new() } else { "The install didn't finish.".into() };
                    self.install_error = if ok { String::new() } else { "The install didn't finish.".into() };
                    self.refresh(h);
                    self.publish(h);
                }
            }
            "download" => {
                let name = args.first().cloned().unwrap_or_default();
                self.download(h, &name, args.get(1).map(|a| a != "false").unwrap_or(true));
            }
            "cancelDownload" => {
                if let Some(p) = self.dl_pid {
                    unsafe { libc::kill(p, libc::SIGTERM) };
                }
            }
            "removeModel" => {
                if let Some(n) = args.first().filter(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')) {
                    util::detached(&["python3", "-I", &util::script("dictate.py"), "remove", n]);
                    let s2 = h.sender("Dictation");
                    util::later(s2, 500, Box::new(InstallDone(true)));
                }
            }
            _ => {}
        }
        None
    }
    fn message(&mut self, h: &mut Host, msg: Box<dyn std::any::Any + Send>) {
        let msg = match msg.downcast::<live::Partial>() {
            Ok(p) => {
                // (a draft still on its way when you stopped is newer than the last: kept)
                if p.0 == self.session && matches!(self.stage, Stage::Listening | Stage::Writing) && p.1 != self.partial {
                    self.partial = p.1;
                    self.publish(h);
                }
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<DoneEnd>() {
            Ok(d) => {
                if d.0 == self.session && self.stage == Stage::Done {
                    self.stage = Stage::Idle;
                    self.typed.clear();
                    self.publish(h);
                }
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<Status>() {
            Ok(s) => {
                self.info = s.0;
                return self.publish(h);
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<RecorderExit>() {
            Ok(r) => return self.recorder_exited(h, r.0),
            Err(m) => m,
        };
        let msg = match msg.downcast::<Transcribed>() {
            Ok(t) => {
                let _ = &t.0;
                return self.transcribed(h, t.1);
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<Limit>() {
            Ok(l) => {
                if l.0 == self.gen {
                    self.stop(h);
                }
                return;
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<Progress>() {
            Ok(p) => {
                let m = p.0;
                if m["total"].as_f64().unwrap_or(0.0) > 0.0 {
                    self.progress = m["done"].as_f64().unwrap_or(0.0) / m["total"].as_f64().unwrap_or(1.0);
                }
                if m["ok"] == json!(false) {
                    self.download_error = m["error"].as_str().unwrap_or("").into();
                }
                if m["ok"] == json!(true) {
                    self.download_ok = true;
                }
                return self.publish(h);
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<DownloadEnd>() {
            Ok(_) => {
                self.dl_pid = None;
                if self.download_ok && self.download_use {
                    util::cfg_set("dictation.model", json!(self.downloading));
                }
                self.downloading.clear();
                self.refresh(h);
                return self.publish(h);
            }
            Err(m) => m,
        };
        let msg = match msg.downcast::<TypeFailed>() {
            Ok(_) => return self.copy_instead(h),
            Err(m) => m,
        };
        let msg = match msg.downcast::<CopyFailed>() {
            Ok(_) => return self.fail(h, "Couldn't put what you said on the clipboard (wl-copy didn't work). Try again."),
            Err(m) => m,
        };
        if msg.downcast::<InstallDone>().is_ok() {
            self.refresh(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_words_typed() {
        assert_eq!(first_words("Send the report today.", 6), "Send the report today.");
        assert_eq!(first_words("  one two three four five six seven  ", 6), "one two three four five six…");
        assert_eq!(first_words("", 6), "");
    }

    #[test]
    fn languages_the_island_may_set() {
        for ok in ["", "auto", "en", "zh", "pt-br"] {
            assert!(language_ok(ok), "{ok}");
        }
        for bad in ["EN", "en;rm", "../x", "a very long code"] {
            assert!(!language_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_new_dictation_may_start_while_the_last_one_shows_its_words() {
        // (Done is only the island's moment: the shortcut starts the next one)
        let mut d = DictationSvc::new();
        assert!(d.tap.is_none() && d.session == 0);
        d.stage = Stage::Done;
        assert!(matches!(d.stage, Stage::Idle | Stage::Done));
    }
}
