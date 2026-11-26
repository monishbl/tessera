//! SpaceMap.js: each display owns a block of 100 workspace ids (its base),
//! kept per display identity. The parts the host needs to number and plan
//! (assign, basesByName, nextFree, plan); the listing of what a display shows
//! (groups) and the Overview's edge zones are core's (spacemap.rs, wm/spaces.rs).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const STRIDE: i32 = 100;

/// The laptop's own panel (in the nested sandbox, its test output plays it)
pub fn is_builtin(name: &str, sandbox: bool) -> bool {
    ["eDP-", "LVDS-", "DSI-"].iter().any(|p| name.starts_with(p)) || (sandbox && name == "TESSERA-TEST")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mon {
    pub name: String,
    pub description: String,
    pub model: String,
    pub x: i32,
    pub y: i32,
    pub active: i32,
}

/// A display's identity: its description (make, model, serial), else its connector
pub fn key_of(m: &Mon) -> String {
    let d = m.description.trim();
    if d.is_empty() {
        format!("name:{}", m.name)
    } else {
        format!("desc:{d}")
    }
}

/// A display's identity among those connected: key_of, except that a second
/// display with the same description (two of one model whose EDID has no
/// serial) is told apart by its connector, so each has its own block of
/// spaces (the one first by connector name keeps the description)
pub fn key_in(m: &Mon, monitors: &[Mon]) -> String {
    let d = m.description.trim();
    if !d.is_empty() && monitors.iter().any(|o| o.description.trim() == d && o.name < m.name) {
        return format!("name:{}", m.name);
    }
    key_of(m)
}

/// A short name for marks and rows: "Built-in", else the model, else the
/// first words of the description, else the connector
pub fn label_of(m: &Mon, sandbox: bool) -> String {
    if is_builtin(&m.name, sandbox) {
        return "Built-in".into();
    }
    let model = m.model.trim();
    if !model.is_empty() && !model.to_lowercase().starts_with("0x") {
        return model.chars().take(18).collect();
    }
    let d = m.description.trim();
    if !d.is_empty() {
        return d.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
    }
    m.name.clone()
}

pub fn base_of_id(id: i32) -> i32 {
    id.div_euclid(STRIDE) * STRIDE
}
pub fn number_of(id: i32) -> i32 {
    id - base_of_id(id)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Disp {
    pub base: i32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub name: String,
}

/// A base for each connected display (the built-in gets 0; with no built-in
/// connected and 0 unused, the first display in the arrangement does; others
/// the next free block), labels and connector names brought up to date
pub fn assign(displays: &BTreeMap<String, Disp>, monitors: &[Mon], sandbox: bool) -> BTreeMap<String, Disp> {
    let mut next = displays.clone();
    let mut list: Vec<&Mon> = monitors.iter().collect();
    list.sort_by_key(|m| (if is_builtin(&m.name, sandbox) { 0 } else { 1 }, m.x, m.y));
    let builtin_here = list.iter().any(|m| is_builtin(&m.name, sandbox));
    for m in list {
        let k = key_in(m, monitors);
        if !next.contains_key(&k) {
            let used = |b: i32, n: &BTreeMap<String, Disp>| n.values().any(|d| d.base == b);
            let base = if !used(0, &next) && (is_builtin(&m.name, sandbox) || !builtin_here) {
                0
            } else {
                let mut b = STRIDE;
                while used(b, &next) {
                    b += STRIDE;
                }
                b
            };
            next.insert(k.clone(), Disp { base, ..Default::default() });
        }
        let d = next.get_mut(&k).unwrap();
        d.label = label_of(m, sandbox);
        d.name = m.name.clone();
    }
    next
}

/// The connected displays' bases by connector
pub fn bases_by_name(displays: &BTreeMap<String, Disp>, monitors: &[Mon]) -> BTreeMap<String, i32> {
    monitors.iter().filter_map(|m| displays.get(&key_in(m, monitors)).map(|d| (m.name.clone(), d.base))).collect()
}

/// The first free number on a display (base), given every id in use
pub fn next_free(base: i32, ids: &[i32]) -> i32 {
    let mut n = 1;
    while n < STRIDE - 1 && ids.contains(&(base + n)) {
        n += 1;
    }
    base + n
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Renumber { from: i32, to: i32 },
    Move { id: i32, monitor: String },
}

pub struct Ws {
    pub id: i32,
    pub monitor: String,
    pub windows: i64,
}

pub struct Plan<'a> {
    pub bases: &'a BTreeMap<String, i32>,
    pub workspaces: &'a [Ws],
    pub active: &'a BTreeMap<String, i32>,
    pub known: &'a [i32],
    pub migrated: bool,
    pub returns: bool,
}

/// What to change so every space sits on the display it belongs to
pub fn plan(c: &Plan) -> Vec<Op> {
    let mut ops = Vec::new();
    let connected: BTreeMap<i32, &String> = c.bases.iter().map(|(n, b)| (*b, n)).collect();
    let mut ids: Vec<i32> = c.workspaces.iter().map(|w| w.id).filter(|id| *id > 0).collect();
    for w in c.workspaces {
        let Some(&here) = c.bases.get(&w.monitor) else { continue };
        if w.id <= 0 {
            continue;
        }
        let b = base_of_id(w.id);
        if b == here {
            continue;
        }
        if !c.migrated && here != 0 {
            let to = next_free(here, &ids);
            ids.push(to);
            ops.push(Op::Renumber { from: w.id, to });
            continue;
        }
        let Some(home) = connected.get(&b) else { continue }; // a guest
        let fresh = !c.known.contains(&w.id);
        if fresh && w.windows <= 0 && c.active.get(&w.monitor) == Some(&w.id) {
            let to = next_free(here, &ids);
            ids.push(to);
            ops.push(Op::Renumber { from: w.id, to });
        } else if c.returns {
            ops.push(Op::Move { id: w.id, monitor: (*home).clone() });
        }
    }
    ops
}

/// A new space after a display's last (`ids`: every id in use), so it shows
/// at the end of its strip (the first free number could be a gap in the middle)
pub fn after_last(base: i32, ids: &[i32]) -> i32 {
    let top = ids.iter().filter(|i| **i > base && base_of_id(**i) == base).map(|i| number_of(*i)).max().unwrap_or(0);
    let mut n = top + 1;
    while n < STRIDE - 1 && ids.contains(&(base + n)) {
        n += 1;
    }
    if n < STRIDE - 1 { base + n } else { next_free(base, ids) }
}

/// A window put on a new space in front of `order[pos]`: that space and the
/// ones after it move one along (the last onto `new_id`), and the window
/// takes `order[pos]`. `windows`: every window's (address, space); returns
/// each move (address, to), the window's last.
pub fn insert_moves(order: &[i32], pos: usize, new_id: i32, windows: &[(String, i32)], address: &str) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    for k in pos..order.len() {
        let to = order.get(k + 1).copied().unwrap_or(new_id);
        for (a, ws) in windows {
            if *ws == order[k] && a != address {
                out.push((a.clone(), to));
            }
        }
    }
    if let Some(at) = order.get(pos) {
        out.push((address.to_string(), *at));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twin_displays_without_a_serial_get_a_block_each() {
        let m = |name: &str, x: i32| Mon { name: name.into(), description: "Dell U2720Q".into(), model: "U2720Q".into(), x, ..Default::default() };
        let mons = vec![m("DP-1", 0), m("DP-2", 2560)];
        assert_eq!(key_in(&mons[0], &mons), "desc:Dell U2720Q");
        assert_eq!(key_in(&mons[1], &mons), "name:DP-2");
        let d = assign(&BTreeMap::new(), &mons, false);
        let bases = bases_by_name(&d, &mons);
        assert_ne!(bases["DP-1"], bases["DP-2"]);
        // alone, it is known by its description again
        assert_eq!(key_in(&mons[1], &mons[1..]), "desc:Dell U2720Q");
    }

    fn mon(name: &str, desc: &str, x: i32) -> Mon {
        Mon { name: name.into(), description: desc.into(), model: "U2720Q".into(), x, ..Default::default() }
    }

    #[test]
    fn blocks_follow_the_display() {
        assert_eq!((base_of_id(103), number_of(103)), (100, 3));
        assert!(is_builtin("eDP-1", false) && !is_builtin("DP-1", false));
        let d = assign(&BTreeMap::new(), &[mon("DP-1", "Dell A", 1920), mon("eDP-1", "Panel", 0)], false);
        assert_eq!(d["desc:Panel"].base, 0);
        assert_eq!(d["desc:Dell A"].base, 100);
        assert_eq!(d["desc:Panel"].label, "Built-in");
        let b = bases_by_name(&d, &[mon("DP-1", "Dell A", 0)]);
        assert_eq!(b["DP-1"], 100);
    }

    #[test]
    fn next_free_skips_used() {
        assert_eq!(next_free(100, &[101, 102, 104]), 103);
    }

    #[test]
    fn a_fresh_empty_space_is_renumbered_into_the_block() {
        let bases: BTreeMap<String, i32> = [("eDP-1".to_string(), 0), ("DP-1".to_string(), 100)].into();
        let ws = [Ws { id: 5, monitor: "DP-1".into(), windows: 0 }];
        let active: BTreeMap<String, i32> = [("DP-1".to_string(), 5)].into();
        let ops = plan(&Plan { bases: &bases, workspaces: &ws, active: &active, known: &[], migrated: true, returns: false });
        assert_eq!(ops, vec![Op::Renumber { from: 5, to: 101 }]);
    }

    #[test]
    fn a_new_space_goes_after_the_last() {
        // 1, 2 and 5 in use: the new one is 6, not the gap at 3
        assert_eq!(after_last(0, &[1, 2, 5, 101]), 6);
        assert_eq!(after_last(100, &[1, 2, 5, 101]), 102);
        assert_eq!(after_last(0, &[]), 1);
        // the block's end: the first free number instead
        assert_eq!(after_last(0, &[1, 98]), 2);
    }

    #[test]
    fn inserting_a_space_moves_the_ones_after_it_along() {
        let w = |a: &str, ws: i32| (a.to_string(), ws);
        let wins = vec![w("a", 1), w("b", 2), w("c", 3), w("d", 3), w("x", 1)];
        // x dropped between 1 and 2: 2's windows to 3, 3's to the new 4, x to 2
        assert_eq!(insert_moves(&[1, 2, 3], 1, 4, &wins, "x"), vec![w("b", 3), w("c", 4), w("d", 4), w("x", 2)]);
        // in front of its own space: the rest of that space moves along, x stays
        assert_eq!(insert_moves(&[1, 2, 3], 0, 4, &wins, "x"), vec![w("a", 2), w("b", 3), w("c", 4), w("d", 4), w("x", 1)]);
    }
}
