//! Spaces.qml: spaces per display (blocks of 100 ids, SpaceMap.js), the moves
//! the Overview, Mission Control and the menu bar ask for (go, newSpace,
//! sendWindow, sendWindowToDisplay, moveSpaceTo, move), the blocks pushed to
//! Hyprland as workspace rules (spaces.lua) and the bookkeeping when a display
//! comes or goes. The views get {bases, labels, count} from here
//! (RustWindows.qml's sendSpaces) and the menu bar's state `spaces`.

use super::dynspace::{self as dynamic, Act};
use super::hx::{self, Fresh};
use super::spacemap::{self as map, Disp, Mon, Op, Plan, Ws};
use super::{Io, Msg};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};
use tessera_core::host::Host;
use tessera_core::services::hypr::Snapshot;

#[derive(Default)]
pub struct Spaces {
    /// connected displays: connector -> base
    pub bases: BTreeMap<String, i32>,
    /// every display we know: base -> short name
    pub labels: BTreeMap<i32, String>,
    /// connected displays: connector -> identity key
    keys: BTreeMap<String, String>,
    show_on: Vec<String>,
    known: Vec<i32>,
    started: bool,
    chain: u8,
    pushed: bool,
    quiet_until: Option<Instant>,
    reading: bool,
    sent: String,
    /// menubar.spacesShown is "open" (told to the views): dynamic spaces,
    /// always 1..n with no gaps (dynspace.rs)
    open: bool,
    /// workspaces.numbering is "sequential": one sequence across the displays
    sequential: bool,
    /// the connected displays left to right (then top to bottom)
    pub order: Vec<String>,
    /// what was last said to the keybinds (TESSERA_SPACES_DYNAMIC)
    said_dynamic: Option<bool>,
    /// dynamic spaces: what's asked (keys, the Overview's drag, closing up),
    /// planned and done one at a time, each on a fresh look at Hyprland
    queue: VecDeque<Act>,
    busy: Option<Instant>,
    /// Hyprland's keybinds.lua looked at (once a run: KEYS_CURRENT)
    keys_checked: bool,
}

/// keybinds.lua as Hyprland last read it knows dynamic spaces: it defines
/// TESSERA_SPACE_FIXED (27 Nov) and its TESSERA_SPACE hands the keys here.
/// An update writes the new file, but Hyprland keeps the one it read until
/// its config is read again; the TESSERA_SPACE before 27 Nov took any op but
/// "focus" for a window move without following (`TESSERA_SPACE("new", 7)`
/// sent the window to space 7 and left you on the emptied one, which this
/// side then closed up: the window on a new space, you on a blank one).
/// The chunk fails (hyprctl eval says why) when the one Hyprland has is older.
pub const KEYS_CURRENT: &str = "assert(type(rawget(_G, \"TESSERA_SPACE_FIXED\")) == \"function\", \"Tessera: Hyprland has keybinds.lua from before dynamic spaces\")\n";

/// Lua that leaves the pointer and the focused display where they were
fn keep_wrap(lua: &str) -> String {
    format!("local c = hl.get_cursor_pos(); local m = hl.get_active_monitor()\n{lua}if m then hl.dispatch(hl.dsp.focus({{ monitor = m.name }})) end\nif c then hl.dispatch(hl.dsp.cursor.move({{ x = c.x, y = c.y }})) end\n")
}

/// What Hyprland has, for the planner: the connected displays (by `bases`),
/// every window, every regular space, what has focus
fn world_of(f: &Fresh, bases: &BTreeMap<String, i32>, sequential: bool) -> dynamic::World {
    let displays = f
        .monitors
        .iter()
        .filter_map(|m| {
            let name = m["name"].as_str()?;
            let base = *bases.get(name)?;
            Some(dynamic::Display { name: name.into(), base, x: m["x"].as_i64().unwrap_or(0) as i32, y: m["y"].as_i64().unwrap_or(0) as i32, active: m["activeWorkspace"]["id"].as_i64().unwrap_or(0) as i32 })
        })
        .collect();
    dynamic::World {
        displays,
        windows: f.clients.iter().map(|c| dynamic::Win { address: hx::addr(&c.address), ws: c.workspace.id, pinned: c.pinned }).collect(),
        workspaces: f.workspaces.iter().filter_map(|w| w["id"].as_i64()).map(|i| i as i32).filter(|i| *i > 0).collect(),
        focused: f.focused_monitor().and_then(|m| m["name"].as_str()).unwrap_or("").into(),
        window: if f.active.is_empty() { String::new() } else { hx::addr(&f.active) },
        sequential,
    }
}

fn q(s: &str) -> String {
    hx::q(s)
}

impl Spaces {
    pub fn base_of(&self, name: &str) -> i32 {
        self.bases.get(name).copied().unwrap_or(0)
    }
    /// The connected display a space belongs to ("" when it isn't connected)
    pub fn owner_name(&self, id: i32) -> String {
        let b = map::base_of_id(id);
        self.bases.iter().find(|(_, v)| **v == b).map(|(n, _)| n.clone()).unwrap_or_default()
    }
    /// Every regular space and the display it's on
    fn workspaces(h: &Host) -> Vec<(i32, String)> {
        let mut out: Vec<(i32, String)> = Vec::new();
        let Some(s) = h.st.shell.get::<Snapshot>() else { return out };
        for (m, id) in &s.active {
            if *id > 0 && !out.iter().any(|(i, _)| i == id) {
                out.push((*id, m.clone()));
            }
        }
        for w in &s.workspaces {
            if w.id > 0 && !out.iter().any(|(i, _)| *i == w.id) {
                out.push((w.id, w.monitor.clone()));
            }
        }
        out
    }
    fn exists(h: &Host, id: i32) -> bool {
        Self::workspaces(h).iter().any(|(i, _)| *i == id)
    }
    fn active_of(h: &Host, name: &str) -> i32 {
        h.st.shell.get::<Snapshot>().and_then(|s| s.active.get(name).copied()).unwrap_or(0)
    }
    fn next_free_on(&self, h: &Host, name: &str) -> i32 {
        let ids: Vec<i32> = Self::workspaces(h).iter().map(|w| w.0).collect();
        map::next_free(self.base_of(name), &ids)
    }

    // ── quiet: moves made from the Overview never take you anywhere ─────────
    fn hush(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        self.quiet_until = Some(self.quiet_until.map_or(until, |q| q.max(until)));
    }
    pub fn quiet(&self) -> bool {
        self.quiet_until.is_some_and(|q| Instant::now() < q)
    }
    /// Lua that leaves the pointer and the focused display where they were
    fn keeping(&mut self, lua: &str) -> String {
        self.hush(1200);
        keep_wrap(lua)
    }

    // ── dynamic spaces (dynspace.rs) ─────────────────────────────────────────
    /// Whether spaces close up and number 1..n (menubar.spacesShown open)
    pub fn dynamic(&self) -> bool {
        self.open
    }
    /// The settings (menubar.spacesShown, workspaces.numbering); true when they changed
    pub fn read_cfg(&mut self, cfg: &Value) -> bool {
        let open = tessera_core::wm::spaces_open(cfg);
        let sequential = tessera_core::wm::spaces_sequential(cfg);
        let changed = open != self.open || sequential != self.sequential;
        self.open = open;
        self.sequential = sequential;
        changed
    }
    /// The settings changed: the keybinds told, the gaps closed (switching
    /// the numbering renumbers nothing: the ids stay per display, only the
    /// numbers shown change)
    pub fn config_changed(&mut self, h: &Host, io: &mut Io) {
        if self.read_cfg(h.cfg()) {
            self.say_dynamic();
            if self.open {
                self.ask(io, Act::Tidy);
            }
        }
    }
    /// Tell keybinds.lua whether the space keys come here
    /// (TESSERA_SPACE hands them over as tessera:space-<op>-<n>)
    fn say_dynamic(&mut self) {
        if self.said_dynamic != Some(self.open) {
            self.said_dynamic = Some(self.open);
            hx::eval(format!("TESSERA_SPACES_DYNAMIC = {}\n", self.open));
            self.check_keys();
        }
    }
    /// With dynamic spaces, the keybinds.lua Hyprland has must be one that
    /// hands the keys here (KEYS_CURRENT); an older one is read again, once
    /// a run (the new file is on disk after an update; if it's still the old
    /// one, reading it again changes nothing and isn't tried twice). The
    /// reload's configreloaded says the blocks and TESSERA_SPACES_DYNAMIC again.
    fn check_keys(&mut self) {
        if !self.open || self.keys_checked {
            return;
        }
        self.keys_checked = true;
        tessera_core::services::hypr::IN_ORDER.run(|| {
            if !tessera_core::services::hypr::eval_logged(KEYS_CURRENT) {
                eprintln!("tessera-ui: spaces: Hyprland's keybinds.lua predates dynamic spaces; reading Hyprland's config again");
                let _ = std::process::Command::new("hyprctl").args(["reload", "config-only"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
            }
        });
    }
    /// The way out: the keybinds number spaces themselves again
    pub fn quit(&mut self) {
        if self.said_dynamic == Some(true) {
            tessera_core::services::hypr::eval_logged("TESSERA_SPACES_DYNAMIC = nil\n");
        }
    }
    /// A space key (Super+N …, the `spaces` ipc): op and "N", "next" or "prev"
    pub fn key(&mut self, io: &mut Io, op: &str, arg: &str) {
        if !self.open {
            // (fixed: the keybinds' own numbering, as before)
            let call = match arg {
                "next" => format!("TESSERA_SPACE_FIXED({}, nil, 1)", hx::q(op)),
                "prev" => format!("TESSERA_SPACE_FIXED({}, nil, -1)", hx::q(op)),
                n => match n.parse::<i32>() {
                    Ok(n) => format!("TESSERA_SPACE_FIXED({}, {n})", hx::q(op)),
                    Err(_) => return,
                },
            };
            hx::eval(format!("if TESSERA_SPACE_FIXED then {call} end\n"));
            return;
        }
        let Some(o) = dynamic::Op::parse(op) else { return };
        let act = match arg {
            "next" => Act::Step(o, 1),
            "prev" => Act::Step(o, -1),
            n => match n.parse::<i32>() {
                Ok(n) if n >= 1 => Act::At(o, n),
                _ => return,
            },
        };
        self.ask(io, act);
    }
    /// Something changed: close up the gaps once things settle (a burst of
    /// events is one look)
    pub fn tick_tidy(&mut self, io: &mut Io) {
        if !self.open {
            return;
        }
        if self.quiet() {
            io.arm("spaces.tidy", 300);
            return;
        }
        self.ask(io, Act::Tidy);
    }
    pub fn ask(&mut self, io: &mut Io, act: Act) {
        if act == Act::Tidy && self.queue.contains(&Act::Tidy) {
            return;
        }
        self.queue.push_back(act);
        self.pump(io);
    }
    /// The next one asked: a fresh look, the plan, the steps in one chunk,
    /// all on Hyprland's own lane (after every dispatch queued before it);
    /// only then the one after (so two quick key presses never plan from
    /// the same, stale look)
    fn pump(&mut self, io: &mut Io) {
        // (an answer that never came back doesn't hold the queue for good)
        if self.busy.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) || self.bases.is_empty() {
            return;
        }
        self.busy = None;
        let Some(act) = self.queue.pop_front() else { return };
        self.busy = Some(Instant::now());
        let bases = self.bases.clone();
        let sequential = self.sequential;
        // (the Overview's drag: you stay where you were)
        let keep = matches!(&act, Act::Insert { follow: false, .. });
        if keep {
            self.hush(1200);
        }
        let post = io.post.clone();
        tessera_core::services::hypr::IN_ORDER.run(move || {
            let f = hx::fetch();
            if !f.monitors.is_empty() {
                let steps = dynamic::plan(&world_of(&f, &bases, sequential), &act);
                if !steps.is_empty() {
                    let lua = dynamic::lua(&steps, hx::q);
                    tessera_core::services::hypr::eval_logged(&if keep { keep_wrap(&lua) } else { lua });
                    tessera_core::services::hypr::refresh();
                }
            }
            post(Msg::Run(
                "spaces::Spaces::pump",
                Box::new(|wm: &mut super::Wm, _h: &mut Host, io: &mut Io| {
                    wm.spaces.busy = None;
                    wm.spaces.pump(io);
                }),
            ));
        });
    }

    // ── actions ──────────────────────────────────────────────────────────────
    /// Go to a space (made on its own display if it doesn't exist yet)
    pub fn go(&mut self, h: &Host, id: i32) {
        let owner = self.owner_name(id);
        if !Self::exists(h, id) && !owner.is_empty() {
            hx::eval(format!("hl.dispatch(hl.dsp.focus({{ monitor = {} }}))\nhl.dispatch(hl.dsp.focus({{ workspace = {id} }}))\n", q(&owner)));
        } else {
            hx::dispatch(format!("hl.dsp.focus({{ workspace = {id} }})"));
        }
    }
    /// A new empty space on a display (after its last), and go there
    pub fn new_space(&mut self, h: &Host, name: &str) {
        let id = self.after_last_on(h, name);
        self.go(h, id);
    }
    fn after_last_on(&self, h: &Host, name: &str) -> i32 {
        let ids: Vec<i32> = Self::workspaces(h).iter().map(|w| w.0).collect();
        map::after_last(self.base_of(name), &ids)
    }
    /// A window dragged off the Overview's spaces: onto a new space on a
    /// display, after its last (`before` 0), or in front of `before` in
    /// `order` (the display's spaces as shown), which moves that space and
    /// the ones after it one along. You stay with what you were looking at.
    pub fn send_window_new(&mut self, h: &Host, io: &mut Io, address: &str, name: &str, order: &[i32], before: i32) {
        if !self.bases.contains_key(name) {
            return;
        }
        if self.open {
            // (dynamic spaces: the planner's insert, as Super+Shift+Alt+N but staying)
            let base = self.base_of(name);
            let list: Vec<i32> = order.iter().copied().filter(|i| *i > 0 && map::base_of_id(*i) == base).collect();
            let index = list.iter().position(|i| *i == before).filter(|_| before > 0).unwrap_or(usize::MAX);
            return self.ask(io, Act::Insert { display: name.to_string(), index, window: hx::addr(address), follow: false });
        }
        let new_id = self.after_last_on(h, name);
        let list: Vec<i32> = order.iter().copied().filter(|i| *i > 0).collect();
        let pos = list.iter().position(|i| *i == before).filter(|_| before > 0);
        let Some(pos) = pos else {
            self.send_window(h, address, new_id);
            return;
        };
        let Some(win) = h.st.shell.get::<tessera_core::services::clients::Windows>() else { return };
        let all: Vec<(String, i32)> = win.clients.iter().map(|c| (c.address.clone(), c.workspace.id)).collect();
        let moves = map::insert_moves(&list, pos, new_id, &all, address);
        let step = |a: &str, ws: &str| format!("hl.dispatch(hl.dsp.window.move({{ workspace = {ws}, follow = false, window = \"address:{}\" }}))\n", hx::addr(a));
        let mut lua = String::new();
        for (a, _) in &moves {
            lua += &step(a, "\"special:tessera-reorder\"");
        }
        for (a, ws) in &moves {
            lua += &step(a, &ws.to_string());
        }
        if moves.iter().any(|(_, ws)| *ws == new_id) {
            lua += &format!("hl.dispatch(hl.dsp.workspace.move({{ workspace = {new_id}, monitor = {} }}))\n", q(name));
        }
        // the space you were on moved along: go with it
        let here = win.monitor(name).map(|m| m.active.id).unwrap_or(0);
        if let Some(k) = list.iter().position(|x| *x == here).filter(|k| *k >= pos) {
            let had = all.iter().any(|(a, ws)| *ws == here && a != address);
            if had {
                lua += &format!("hl.dispatch(hl.dsp.focus({{ workspace = {} }}))\n", list.get(k + 1).copied().unwrap_or(new_id));
            }
        }
        let lua = self.keeping(&lua);
        hx::eval(lua);
    }
    /// A window to a space (on that space's own display, even if it doesn't exist yet)
    pub fn send_window(&mut self, h: &Host, address: &str, id: i32) {
        self.hush(1200);
        let mut lua = format!("hl.dispatch(hl.dsp.window.move({{ workspace = {id}, follow = false, window = {} }}))\n", q(&format!("address:{}", hx::addr(address))));
        let owner = self.owner_name(id);
        if !Self::exists(h, id) && !owner.is_empty() {
            let more = format!("{lua}hl.dispatch(hl.dsp.workspace.move({{ workspace = {id}, monitor = {} }}))\n", q(&owner));
            lua = self.keeping(&more);
        }
        hx::eval(lua);
    }
    /// A window to another display: onto the space it shows
    pub fn send_window_to_display(&mut self, h: &Host, address: &str, name: &str) {
        if !self.bases.contains_key(name) {
            return;
        }
        let a = Self::active_of(h, name);
        let id = if a > 0 { a } else { self.next_free_on(h, name) };
        self.send_window(h, address, id);
    }
    /// A whole space to another display: it becomes that display's next number there
    pub fn move_space_to(&mut self, h: &Host, id: i32, name: &str) {
        if id <= 0 || !self.bases.contains_key(name) {
            return;
        }
        let to = self.next_free_on(h, name);
        let lua = self.keeping(&format!("hl.dispatch(hl.dsp.workspace.change_id({{ workspace = {id}, id = {to} }}))\nhl.dispatch(hl.dsp.workspace.move({{ workspace = {to}, monitor = {} }}))\n", q(name)));
        hx::eval(lua);
    }
    /// Reordering (drag a space): the dropped space takes that one's place and
    /// the ones between shift by one; the windows move, through a hidden
    /// holding space, in one Lua chunk. If you were on a space that moved, you follow it.
    pub fn reorder(&mut self, h: &Host, order: &[i32], from: i32, to: i32) {
        let list: Vec<i32> = order.iter().copied().filter(|i| *i > 0).collect();
        let (Some(a), Some(b)) = (list.iter().position(|i| *i == from), list.iter().position(|i| *i == to)) else { return };
        if a == b {
            return;
        }
        let mut next = list.clone();
        next.remove(a);
        next.insert(b, from);
        let (lo, hi) = (a.min(b), a.max(b));
        let Some(win) = h.st.shell.get::<tessera_core::services::clients::Windows>() else { return };
        let mut moves: Vec<(String, i32)> = Vec::new();
        for i in lo..=hi {
            for c in win.clients.iter().filter(|c| c.workspace.id == next[i]) {
                moves.push((hx::addr(&c.address), list[i]));
            }
        }
        if moves.is_empty() {
            return;
        }
        self.hush(1200);
        let step = |addr: &str, ws: &str| format!("hl.dispatch(hl.dsp.window.move({{ workspace = {ws}, follow = false, window = \"address:{addr}\" }}))\n");
        let mut lua = String::new();
        for (a, _) in &moves {
            lua += &step(a, "\"special:tessera-reorder\"");
        }
        for (a, ws) in &moves {
            lua += &step(a, &ws.to_string());
        }
        let here = win.focused_monitor().map(|m| m.active.id).unwrap_or(0);
        if let Some(i) = next.iter().position(|x| *x == here) {
            if here > 0 && i >= lo && i <= hi && list[i] != here {
                lua += &format!("hl.dispatch(hl.dsp.focus({{ workspace = {} }}))\n", list[i]);
            }
        }
        hx::eval(lua);
    }

    // ── keeping track of displays ────────────────────────────────────────────
    /// A display came (back), as its connect effect hands over: it shows the
    /// space it showed before
    pub fn arrive(&mut self, io: &mut Io, name: &str) {
        if !self.show_on.iter().any(|n| n == name) {
            self.show_on.push(name.to_string());
        }
        io.arm("spaces.reconcile", 1);
    }
    pub fn reconcile(&mut self, io: &mut Io, ms: u64) {
        io.arm("spaces.reconcile", ms);
    }
    pub fn tick_reconcile(&mut self, io: &mut Io) {
        if self.reading {
            io.arm("spaces.reconcile", 250);
            return;
        }
        self.reading = true;
        io.fetch_then(|wm, h, io, f| {
            wm.spaces.reading = false;
            wm.spaces.apply(h, io, f);
        });
    }

    fn apply(&mut self, h: &mut Host, io: &mut Io, f: Fresh) {
        if f.monitors.is_empty() {
            return;
        }
        let sandbox = tessera_core::env::sandboxed();
        let ignored = |n: &str| n.is_empty() || n.starts_with("HEADLESS") || (n.starts_with("TESSERA-") && !sandbox);
        let mons: Vec<Mon> = f
            .monitors
            .iter()
            .filter(|m| m["disabled"] != true && !ignored(m["name"].as_str().unwrap_or("")))
            .map(|m| Mon {
                name: m["name"].as_str().unwrap_or("").into(),
                description: m["description"].as_str().unwrap_or("").into(),
                model: m["model"].as_str().unwrap_or("").into(),
                x: m["x"].as_i64().unwrap_or(0) as i32,
                y: m["y"].as_i64().unwrap_or(0) as i32,
                active: m["activeWorkspace"]["id"].as_i64().unwrap_or(0) as i32,
            })
            .collect();
        if mons.is_empty() {
            return;
        }
        let store = hx::state_get("spaces");
        let before: BTreeMap<String, Disp> = serde_json::from_value(store["displays"].clone()).unwrap_or_default();
        let displays = map::assign(&before, &mons, sandbox);
        let next_bases = map::bases_by_name(&displays, &mons);
        let next_keys: BTreeMap<String, String> = mons.iter().map(|m| (m.name.clone(), map::key_in(m, &mons))).collect();
        let next_labels: BTreeMap<i32, String> = displays.values().map(|d| (d.base, d.label.clone())).collect();
        let changed = next_bases != self.bases || displays != before;
        let mut by_place: Vec<&Mon> = mons.iter().filter(|m| next_bases.contains_key(&m.name)).collect();
        by_place.sort_by(|a, b| (a.x, a.y, &a.name).cmp(&(b.x, b.y, &b.name)));
        let next_order: Vec<String> = by_place.into_iter().map(|m| m.name.clone()).collect();
        let moved = changed || next_labels != self.labels || next_order != self.order;
        self.order = next_order;
        self.bases = next_bases.clone();
        self.keys = next_keys.clone();
        self.labels = next_labels;
        if changed || !self.pushed {
            self.push(&displays);
        }

        let ws: Vec<Ws> = f
            .workspaces
            .iter()
            .filter(|w| w["id"].as_i64().unwrap_or(0) > 0)
            .map(|w| Ws { id: w["id"].as_i64().unwrap_or(0) as i32, monitor: w["monitor"].as_str().unwrap_or("").into(), windows: w["windows"].as_i64().unwrap_or(0) })
            .collect();
        let active: BTreeMap<String, i32> = mons.iter().map(|m| (m.name.clone(), m.active)).collect();
        let known: Vec<i32> = if self.started { self.known.clone() } else { ws.iter().map(|w| w.id).collect() };
        let migrated = store["migrated"] == true;
        let ops = map::plan(&Plan { bases: &next_bases, workspaces: &ws, active: &active, known: &known, migrated, returns: false });
        let shown_before = store["shown"].clone();
        let show: Vec<String> = self.show_on.iter().filter(|n| next_bases.contains_key(*n)).cloned().collect();
        self.show_on.clear();

        let mut lua = String::new();
        let mut ids: Vec<i32> = ws.iter().map(|w| w.id).collect();
        for op in &ops {
            match op {
                Op::Renumber { from, to } => {
                    lua += &format!("hl.dispatch(hl.dsp.workspace.change_id({{ workspace = {from}, id = {to} }}))\n");
                    ids.retain(|i| i != from);
                    ids.push(*to);
                }
                Op::Move { id, monitor } => lua += &format!("hl.dispatch(hl.dsp.workspace.move({{ workspace = {id}, monitor = {} }}))\n", q(monitor)),
            }
        }
        // A display that came back shows what it showed before (if that space is still there)
        for n in &show {
            let id = next_keys.get(n).and_then(|k| shown_before[k].as_i64()).unwrap_or(0) as i32;
            if id > 0 && map::base_of_id(id) == next_bases[n] && ids.contains(&id) {
                lua += &format!("hl.dispatch(hl.dsp.focus({{ monitor = {} }}))\nhl.dispatch(hl.dsp.focus({{ workspace = {id} }}))\n", q(n));
            }
        }
        self.known = ids;
        self.started = true;
        let mut next = if store.is_object() { store.clone() } else { json!({}) };
        next["displays"] = serde_json::to_value(&displays).unwrap_or(Value::Null);
        next["migrated"] = json!(true);
        if next != store {
            hx::state_set("spaces", next);
        }
        if !lua.is_empty() {
            let wrapped = self.keeping(&lua);
            hx::eval(wrapped);
            // (what Hyprland does in turn: a display left without a space gets one)
            if self.chain < 3 {
                self.chain += 1;
                io.arm("spaces.reconcile", 600);
            }
        } else {
            self.chain = 0;
        }
        self.note_shown(&f);
        if moved {
            self.tell(h);
        }
        // (what was asked before the displays were known)
        self.pump(io);
    }

    /// The blocks to Hyprland: every display we know, by identity, as its
    /// workspace rules (now, and in spaces.lua for the next login), and the
    /// connected ones by connector for the keybinds
    pub fn push(&mut self, displays: &BTreeMap<String, Disp>) {
        let known = if displays.is_empty() { serde_json::from_value(hx::state_get("spaces")["displays"].clone()).unwrap_or_default() } else { displays.clone() };
        let mut b = String::from("TESSERA_SPACE_BASES = {");
        for (k, d) in &known {
            b += &format!(" [{}] = {},", q(k), d.base);
        }
        b += " }\n";
        let mut t = String::from("TESSERA_SPACES = {");
        for (n, base) in &self.bases {
            t += &format!(" [{}] = {base},", q(n));
        }
        t += " }\n";
        // (and whether the space keys come here: dynamic spaces)
        self.said_dynamic = Some(self.open);
        hx::eval(format!("{b}{t}TESSERA_SPACES_DYNAMIC = {}\nif TESSERA_SPACE_RULES then TESSERA_SPACE_RULES(TESSERA_SPACE_BASES) end\n", self.open));
        self.check_keys();
        if !tessera_core::env::sandboxed() || !known.is_empty() {
            let text = format!("-- Generated by Tessera (shell/services/Spaces.qml): which display each block of spaces\n-- belongs to. Loaded by hypr/tessera/keybinds.lua; changes here are overwritten.\n{b}");
            let path = hx::config_dir().join("spaces.lua");
            // (on the files lane: in order, waited for on the way out)
            tessera_core::cfgfile::write_later(path, text.into_bytes());
        }
        self.pushed = true;
    }
    pub fn rules_again(&mut self) {
        let d = BTreeMap::new();
        self.push(&d);
    }

    /// The space each display shows (its own ones only), kept for when it's
    /// unplugged and comes back
    fn note_shown(&mut self, f: &Fresh) {
        let store = hx::state_get("spaces");
        let mut shown = if store["shown"].is_object() { store["shown"].clone() } else { json!({}) };
        let mut changed = false;
        for m in &f.monitors {
            let name = m["name"].as_str().unwrap_or("");
            let id = m["activeWorkspace"]["id"].as_i64().unwrap_or(0) as i32;
            if let (Some(k), Some(base)) = (self.keys.get(name), self.bases.get(name)) {
                if id > 0 && map::base_of_id(id) == *base && shown[k] != json!(id) {
                    shown[k] = json!(id);
                    changed = true;
                }
            }
        }
        if changed {
            let mut next = if store.is_object() { store } else { json!({}) };
            next["shown"] = shown;
            hx::state_set("spaces", next);
        }
    }
    pub fn tick_shown(&mut self, io: &mut Io) {
        io.fetch_then(|wm, _h, _io, f| wm.spaces.note_shown(&f));
    }

    /// Hyprland's events (Spaces.qml's Connections)
    pub fn event(&mut self, io: &mut Io, name: &str) {
        match name {
            "configreloaded" => self.rules_again(), // (a reload starts Hyprland's Lua afresh)
            "workspacev2" | "focusedmonv2" => io.arm("spaces.shown", 150),
            "monitoraddedv2" | "monitorremovedv2" | "createworkspacev2" | "moveworkspacev2" => self.reconcile(io, 250),
            _ => {}
        }
        // dynamic spaces: a space may have emptied (or you left an empty one)
        if self.open && matches!(name, "openwindow" | "closewindow" | "movewindow" | "movewindowv2" | "workspacev2" | "destroyworkspacev2" | "createworkspacev2" | "moveworkspacev2" | "monitoraddedv2" | "monitorremovedv2") {
            io.arm("spaces.tidy", 200);
        }
    }

    // ── what the views get ───────────────────────────────────────────────────
    pub fn json(&self, count: i64) -> Value {
        let labels: serde_json::Map<String, Value> = self.labels.iter().map(|(b, l)| (b.to_string(), json!(l))).collect();
        json!({"bases": self.bases, "labels": labels, "count": count, "open": self.open, "order": self.order, "sequential": self.sequential && self.open})
    }
    /// To the views and the menu bar's state, when it changed
    pub fn tell(&mut self, h: &mut Host) {
        // (clamped as everywhere: tessera_core::wm::workspace_count)
        let count = tessera_core::wm::workspace_count(h.cfg()) as i64;
        self.read_cfg(h.cfg());
        let v = self.json(count);
        h.set_state("spaces", json!({"bases": v["bases"], "labels": v["labels"], "order": v["order"], "sequential": v["sequential"]}));
        self.sent = v.to_string();
        for n in ["overview", "missioncontrol", "switcher"] {
            h.tell(n, json!({"spaces": v}));
        }
    }
    /// Said again (a surface started drawing)
    pub fn resend(&mut self, h: &mut Host) {
        if !self.bases.is_empty() {
            self.tell(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KEYS_CURRENT;

    #[test]
    fn the_keybinds_check_knows_the_current_file_from_the_old_one() {
        // (the laptop, 27 Nov: Hyprland still had the keybinds.lua from before
        // dynamic spaces, whose TESSERA_SPACE("new", 7) moved the window to 7
        // without following; KEYS_CURRENT is what tells the two apart)
        let now = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../hypr/tessera/keybinds.lua"));
        assert!(KEYS_CURRENT.contains("\"TESSERA_SPACE_FIXED\""));
        assert!(now.contains("\nfunction TESSERA_SPACE_FIXED("), "keybinds.lua defines what the check looks for");
        // its TESSERA_SPACE hands the dynamic keys over, and a "new" never moves a window
        let space = &now[now.find("\nfunction TESSERA_SPACE(").unwrap()..];
        let space = &space[..space.find("\nend\n").unwrap()];
        assert!(space.contains("TESSERA_SPACES_DYNAMIC") && space.contains("tessera:space-"));
        let fixed = &now[now.find("\nfunction TESSERA_SPACE_FIXED(").unwrap()..];
        assert!(fixed.contains(r#"if op == "new" then op = "focus""#));
    }
}
