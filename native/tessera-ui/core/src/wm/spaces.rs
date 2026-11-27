//! Spaces per display, as shell/services/Spaces.qml and SpaceMap.js number
//! and name them, for the window-management surfaces: each display owns a
//! block of 100 ids (its base, from the QML Spaces service, sent by
//! modules/overview/RustWindows.qml), its spaces are listed by the menu
//! bar's own groups (crate::spacemap), titled "Space 3" (or "Space 3 ·
//! U2720Q" when they belong to another display), and the Overview's edge
//! zones face the other displays (SpaceMap.edgeZones / edgeTarget).

use crate::services::hypr::Snapshot;
use crate::spacemap::{self as bar, STRIDE};
use std::collections::BTreeMap;

/// What the QML Spaces service knows: each connected display's base, every
/// known display's short name by base, and how many spaces always show
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub bases: BTreeMap<String, i32>,
    pub labels: BTreeMap<i32, String>,
    pub count: i32,
    /// menubar.spacesShown: only the spaces there are (open), or always
    /// `count` with no gaps (fixed)
    pub open: bool,
    /// workspaces.numbering "sequential" (with open): one sequence across
    /// the displays, in `order` (left to right)
    pub sequential: bool,
    pub order: Vec<String>,
    /// the sequence's numbers, from the last `renumber`
    pub numbers: BTreeMap<i32, i32>,
}

impl Default for Info {
    fn default() -> Self {
        Self { bases: BTreeMap::new(), labels: BTreeMap::new(), count: 5, open: true, sequential: false, order: Vec::new(), numbers: BTreeMap::new() }
    }
}

/// The spaces of a list that are there: Hyprland has them, or it's `active`
pub fn only_open(ids: Vec<i32>, active: Option<i32>, h: &Snapshot) -> Vec<i32> {
    ids.into_iter().filter(|id| Some(*id) == active || h.workspaces.iter().any(|w| w.id == *id)).collect()
}

pub fn base_of_id(id: i32) -> i32 {
    bar::base_of(id)
}

/// Spaces.numberOf: its number on its own display (1 for 101)
pub fn number_of(id: i32) -> i32 {
    if id > 0 {
        id - base_of_id(id)
    } else {
        id
    }
}

impl Info {
    /// From the feed's `spaces` object: {"bases": {name: base}, "labels": {base: label}, "count": n}
    pub fn from_json(v: &serde_json::Value, count: i32) -> Self {
        let bases = v["bases"].as_object().map(|o| o.iter().filter_map(|(k, b)| b.as_i64().map(|b| (k.clone(), b as i32))).collect()).unwrap_or_default();
        let labels = v["labels"].as_object().map(|o| o.iter().filter_map(|(k, l)| Some((k.parse().ok()?, l.as_str()?.to_string()))).collect()).unwrap_or_default();
        let order = v["order"].as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default();
        Self { bases, labels, count: v["count"].as_i64().map(|c| c as i32).unwrap_or(count), open: v["open"].as_bool().unwrap_or(true), sequential: v["sequential"].as_bool().unwrap_or(false), order, numbers: BTreeMap::new() }
    }

    /// The numbers spaces show, from what Hyprland has now: one sequence
    /// across the displays (sequential, with open), each display's own
    /// spaces in its shown order; otherwise each display's own (number_of)
    pub fn renumber(&mut self, h: &Snapshot) {
        self.numbers.clear();
        if !(self.sequential && self.open) {
            return;
        }
        let order: Vec<String> = if self.order.is_empty() { self.bases.keys().cloned().collect() } else { self.order.clone() };
        let mut n = 0;
        for name in &order {
            let ids: Vec<i32> = self.shown_for(name, h).into_iter().filter(|id| self.is_own(*id, name)).collect();
            for id in ids {
                n += 1;
                self.numbers.insert(id, n);
            }
        }
    }

    /// The number a space shows: in the sequence, else its own display's
    pub fn number(&self, id: i32) -> i32 {
        self.numbers.get(&id).copied().unwrap_or_else(|| number_of(id))
    }

    pub fn base_of(&self, name: &str) -> i32 {
        self.bases.get(name).copied().unwrap_or(0)
    }

    /// The connected display a space belongs to ("" when it isn't connected)
    pub fn owner_name(&self, id: i32) -> String {
        let b = base_of_id(id);
        self.bases.iter().find(|(_, v)| **v == b).map(|(n, _)| n.clone()).unwrap_or_default()
    }

    pub fn owner_label(&self, id: i32) -> String {
        self.labels.get(&base_of_id(id)).cloned().unwrap_or_default()
    }

    pub fn is_own(&self, id: i32, name: &str) -> bool {
        id > 0 && base_of_id(id) == self.base_of(name)
    }

    /// "Space 3", or "Space 3 · U2720Q" when it belongs to another display
    pub fn title(&self, id: i32, name: &str, word: &str) -> String {
        let t = format!("{word} {}", self.number(id));
        let owner = self.owner_label(id);
        if self.is_own(id, name) || owner.is_empty() {
            t
        } else {
            format!("{t} · {owner}")
        }
    }

    pub fn label_for_name(&self, name: &str) -> String {
        self.labels.get(&self.base_of(name)).cloned().unwrap_or_else(|| name.to_string())
    }

    /// Spaces.idsFor(name, false): its own spaces, then guests staying here
    pub fn ids_for(&self, name: &str, h: &Snapshot) -> Vec<i32> {
        bar::groups(name, &self.bases, h, self.count, false, &self.labels).into_iter().flat_map(|g| g.ids).collect()
    }

    /// The spaces Overview and All Windows show for a display, in its order
    /// (both from here, so they always agree, and the menu bar the same
    /// way): open (menubar.spacesShown, the default) the ones Hyprland has
    /// (with windows, on a screen, or kept by a rule) and the current one,
    /// + makes another; fixed: always the count, gaps filled (ids_for).
    pub fn shown_for(&self, name: &str, h: &Snapshot) -> Vec<i32> {
        let ids = self.ids_for(name, h);
        if self.open { only_open(ids, h.active.get(name).copied(), h) } else { ids }
    }

    /// A space from a display that isn't connected (a monitor badge)
    pub fn guest(&self, id: i32) -> bool {
        !self.bases.is_empty() && !self.bases.values().any(|b| *b == base_of_id(id))
    }

    /// Spaces.idFor(name, n)
    pub fn id_for(&self, name: &str, n: i32) -> i32 {
        self.base_of(name) + n
    }

    /// Spaces.nextFreeOn
    pub fn next_free(&self, name: &str, h: &Snapshot) -> i32 {
        let base = self.base_of(name);
        let mut n = 1;
        while n < STRIDE - 1 && h.workspaces.iter().any(|w| w.id == base + n) {
            n += 1;
        }
        base + n
    }
}

/// A display's place and logical size in the arrangement
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geo {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn vertical(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
}

/// SpaceMap.sideOf: which edge of `a` faces `b`
pub fn side_of(a: Geo, b: Geo) -> Side {
    let dx = (b.x + b.w / 2.0) - (a.x + a.w / 2.0);
    let dy = (b.y + b.h / 2.0) - (a.y + a.h / 2.0);
    let h_over = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let v_over = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    let horiz = if v_over > 0.0 && h_over <= 0.0 {
        true
    } else if h_over > 0.0 && v_over <= 0.0 {
        false
    } else {
        dx.abs() / ((a.w + b.w) / 2.0).max(1.0) >= dy.abs() / ((a.h + b.h) / 2.0).max(1.0)
    };
    match (horiz, dx < 0.0, dy < 0.0) {
        (true, true, _) => Side::Left,
        (true, false, _) => Side::Right,
        (false, _, true) => Side::Top,
        (false, _, false) => Side::Bottom,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: String,
    pub side: Side,
    pub index: usize,
    pub count: usize,
}

/// SpaceMap.edgeZones: one per other display, on the edge that faces it
pub fn edge_zones(here: Geo, others: &[(String, Geo)]) -> Vec<Zone> {
    let mut out = Vec::new();
    for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
        let mut list: Vec<&(String, Geo)> = others.iter().filter(|(_, g)| side_of(here, *g) == side).collect();
        list.sort_by(|(_, p), (_, q)| {
            let (a, b) = if side.vertical() { (p.y + p.h / 2.0 - (q.y + q.h / 2.0), p.x - q.x) } else { (p.x + p.w / 2.0 - (q.x + q.w / 2.0), p.y - q.y) };
            a.partial_cmp(&0.0).unwrap().then(b.partial_cmp(&0.0).unwrap())
        });
        let n = list.len();
        for (i, (name, _)) in list.into_iter().enumerate() {
            out.push(Zone { name: name.clone(), side, index: i, count: n });
        }
    }
    out
}

/// SpaceMap.edgeTarget: the display a drag is pushed against (the pointer,
/// in this display's own coordinates, at or past an edge facing it)
pub fn edge_target(here: Geo, others: &[(String, Geo)], px: f32, py: f32, margin: f32) -> String {
    let past = [(Side::Left, -px + margin), (Side::Right, px - (here.w - 1.0) + margin), (Side::Top, -py + margin), (Side::Bottom, py - (here.h - 1.0) + margin)];
    let mut side = None;
    let mut most = 0.0;
    for (s, v) in past {
        if v > most {
            most = v;
            side = Some(s);
        }
    }
    let Some(side) = side else { return String::new() };
    let (gx, gy) = (here.x + px, here.y + py);
    let mut best = String::new();
    let mut dist = 1e12;
    for (name, o) in others {
        if side_of(here, *o) != side {
            continue;
        }
        let (along, lo, len) = if side.vertical() { (gy, o.y, o.h) } else { (gx, o.x, o.w) };
        let d = if along < lo {
            lo - along
        } else if along >= lo + len {
            along - (lo + len) + 1.0
        } else {
            0.0
        };
        if d < dist {
            dist = d;
            best = name.clone();
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::hypr::Workspace;
    use serde_json::json;

    fn info() -> Info {
        Info::from_json(&json!({"bases": {"eDP-1": 0, "DP-1": 100}, "labels": {"0": "Built-in", "100": "U2720Q"}}), 3)
    }

    #[test]
    fn titles_say_whose_space_it_is() {
        let i = info();
        assert_eq!(i.count, 3);
        assert_eq!(i.title(2, "eDP-1", "Space"), "Space 2");
        assert_eq!(i.title(102, "eDP-1", "Desktop"), "Desktop 2 · U2720Q");
        assert_eq!(i.title(102, "DP-1", "Space"), "Space 2");
        assert_eq!(i.label_for_name("DP-1"), "U2720Q");
        assert_eq!(i.label_for_name("HDMI-A-1"), "Built-in"); // (base 0 when unknown)
        assert_eq!(i.owner_name(105), "DP-1");
        assert_eq!(i.owner_name(205), "");
        assert!(i.guest(205) && !i.guest(105));
        assert!(!Info::default().guest(205));
        assert_eq!((number_of(101), number_of(7), number_of(-99)), (1, 7, -99));
    }

    #[test]
    fn ids_and_the_next_free_space() {
        let i = info();
        let h = Snapshot { workspaces: vec![Workspace { id: 1, monitor: "eDP-1".into(), ..Default::default() }, Workspace { id: 2, monitor: "eDP-1".into(), ..Default::default() }, Workspace { id: 101, monitor: "DP-1".into(), ..Default::default() }], ..Default::default() };
        assert_eq!(i.ids_for("eDP-1", &h), vec![1, 2, 3]);
        assert_eq!(i.ids_for("DP-1", &h), vec![101, 102, 103]);
        assert_eq!(i.next_free("eDP-1", &h), 3);
        assert_eq!(i.next_free("DP-1", &h), 102);
        assert_eq!(i.id_for("DP-1", 1), 101);
    }

    #[test]
    fn overview_and_all_windows_show_the_same_spaces() {
        // 1–3 in use, 8 current (4–7 aren't there): 1, 2, 3, 8, not 1–8
        let i = Info::from_json(&json!({"bases": {"eDP-1": 0}}), 5);
        let ws = |id| Workspace { id, monitor: "eDP-1".into(), ..Default::default() };
        let mut h = Snapshot { workspaces: vec![ws(1), ws(2), ws(3), ws(8)], ..Default::default() };
        h.active.insert("eDP-1".into(), 8);
        assert_eq!(i.ids_for("eDP-1", &h), (1..=8).collect::<Vec<_>>(), "the menu bar's own list fills the gaps");
        assert_eq!(i.shown_for("eDP-1", &h), vec![1, 2, 3, 8]);
        // the current space shows even before Hyprland lists it
        h.active.insert("eDP-1".into(), 4);
        assert_eq!(i.shown_for("eDP-1", &h), vec![1, 2, 3, 4, 8]);
        // fixed (menubar.spacesShown): always the count, the gaps filled, as the bar
        let f = Info { open: false, ..i.clone() };
        assert_eq!(f.shown_for("eDP-1", &h), (1..=8).collect::<Vec<_>>());
        let g = Info { open: false, ..Info::from_json(&json!({"bases": {"eDP-1": 0}, "open": false}), 5) };
        assert!(!g.open);
        h.workspaces = vec![ws(1)];
        h.active.insert("eDP-1".into(), 1);
        assert_eq!(g.shown_for("eDP-1", &h), vec![1, 2, 3, 4, 5]);
        assert_eq!(i.shown_for("eDP-1", &h), vec![1]);
    }

    #[test]
    fn one_sequence_across_displays() {
        // the built-in on the left (1, 2), DP-1 on the right (101, 102, current 103)
        let mut i = Info::from_json(&json!({"bases": {"eDP-1": 0, "DP-1": 100}, "order": ["eDP-1", "DP-1"], "sequential": true}), 5);
        let ws = |id, m: &str| Workspace { id, monitor: m.into(), ..Default::default() };
        let mut h = Snapshot { workspaces: vec![ws(1, "eDP-1"), ws(2, "eDP-1"), ws(101, "DP-1"), ws(102, "DP-1")], ..Default::default() };
        h.active.insert("eDP-1".into(), 1);
        h.active.insert("DP-1".into(), 103);
        i.renumber(&h);
        assert_eq!((i.number(1), i.number(2), i.number(101), i.number(102), i.number(103)), (1, 2, 3, 4, 5));
        assert_eq!(i.title(102, "DP-1", "Space"), "Space 4");
        // fixed, or each display its own: their own numbers
        i.open = false;
        i.renumber(&h);
        assert_eq!(i.number(102), 2);
        let mut own = Info::from_json(&json!({"bases": {"eDP-1": 0, "DP-1": 100}}), 5);
        own.renumber(&h);
        assert!(!own.sequential);
        assert_eq!(own.title(102, "DP-1", "Space"), "Space 2");
    }

    #[test]
    fn zones_face_the_other_displays() {
        let here = Geo { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        let right = Geo { x: 1920.0, y: 0.0, w: 2560.0, h: 1440.0 };
        let above = Geo { x: 0.0, y: -1080.0, w: 1920.0, h: 1080.0 };
        assert_eq!(side_of(here, right), Side::Right);
        assert_eq!(side_of(here, above), Side::Top);
        let z = edge_zones(here, &[("DP-1".into(), right), ("DP-2".into(), above)]);
        assert_eq!(z.iter().map(|z| (z.name.as_str(), z.side)).collect::<Vec<_>>(), vec![("DP-1", Side::Right), ("DP-2", Side::Top)]);
        // pushed against the right edge, the one to the right; inside, none
        let others = [("DP-1".to_string(), right), ("DP-2".to_string(), above)];
        assert_eq!(edge_target(here, &others, 1919.0, 500.0, 3.0), "DP-1");
        assert_eq!(edge_target(here, &others, 900.0, 0.0, 3.0), "DP-2");
        assert_eq!(edge_target(here, &others, 900.0, 500.0, 3.0), "");
        // nothing to the left
        assert_eq!(edge_target(here, &others, 0.0, 500.0, 3.0), "");
    }
}
