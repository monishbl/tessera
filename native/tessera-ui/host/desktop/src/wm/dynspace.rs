//! Dynamic spaces (menubar.spacesShown "open", the default): a display's
//! spaces are always 1..n with no gaps, counting only the spaces that have a
//! window and the ones on screen. The pure planning half: given what
//! Hyprland has now (`World`) and what was asked (`Act`), the steps to take
//! (`Step`), in order. spaces.rs fetches, plans and runs them one at a time.
//!
//! - A space that empties and isn't on screen closes up: the ones after it
//!   take one number less. The space you're on stays, blank, until you leave.
//! - Renumbering is Hyprland's own `change_id`: the space keeps its windows
//!   (floating, sizes, fullscreen) and nothing on screen moves. Up from the
//!   end backwards (inserting), down from the start (closing up), so no id is
//!   ever asked for while another space holds it.
//! - Pinned windows don't count; special spaces (the scratchpad, minimized)
//!   and other displays' blocks are never renumbered.
//! - Numbering (workspaces.numbering): each display its own 1..n (perDisplay,
//!   the default), or one sequence across all of them, left to right
//!   (sequential); positions past the end make a space on the display you're on.

use super::spacemap::{base_of_id, STRIDE};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Display {
    pub name: String,
    pub base: i32,
    pub x: i32,
    pub y: i32,
    /// the space it shows
    pub active: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Win {
    pub address: String,
    pub ws: i32,
    pub pinned: bool,
}

/// What Hyprland has right now
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    /// the connected displays
    pub displays: Vec<Display>,
    pub windows: Vec<Win>,
    /// every regular space Hyprland lists (ids > 0)
    pub workspaces: Vec<i32>,
    /// the focused display's name
    pub focused: String,
    /// the focused window's address ("" none)
    pub window: String,
    /// one sequence across the displays (workspaces.numbering "sequential")
    pub sequential: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// go there
    Focus,
    /// the focused window there; you stay
    Send,
    /// the focused window there; you follow
    Move,
    /// a new empty space there (the later ones one along), and go to it
    New,
    /// a new space there with the focused window, and go to it
    NewMove,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s {
            "focus" => Op::Focus,
            "send" => Op::Send,
            "move" => Op::Move,
            "new" => Op::New,
            "newmove" => Op::NewMove,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// position n (1-based: on the display you're on, or in the sequence)
    At(Op, i32),
    /// the next (+1) or previous (-1) one from where you are (the window, for Send and Move)
    Step(Op, i32),
    /// A window onto a new space on a display, in front of its `index`-th
    /// space (0-based; its count or more: after the last): the Overview's drag
    Insert { display: String, index: usize, window: String, follow: bool },
    /// only close up the gaps
    Tidy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// a space takes another id (Hyprland's change_id: its windows stay in it)
    Rename { from: i32, to: i32 },
    /// a window to a space (follow: and go with it)
    Window { address: String, to: i32, follow: bool },
    /// focus a display
    Monitor(String),
    /// a space onto a display
    Place { id: i32, monitor: String },
    /// go to a space
    Focus(i32),
}

fn in_block(id: i32, base: i32) -> bool {
    id > base && id < base + STRIDE && base_of_id(id) == base
}

impl World {
    fn has_windows(&self, id: i32) -> bool {
        self.windows.iter().any(|w| !w.pinned && w.ws == id)
    }
    fn on_screen(&self, id: i32) -> bool {
        self.displays.iter().any(|d| d.active == id)
    }
    /// A display's spaces in order: its block's spaces with a window
    /// (pinned ones don't count) and the ones on screen
    pub fn spaces(&self, base: i32) -> Vec<i32> {
        let set: BTreeSet<i32> = self
            .windows
            .iter()
            .filter(|w| !w.pinned)
            .map(|w| w.ws)
            .chain(self.displays.iter().map(|d| d.active))
            .filter(|id| in_block(*id, base))
            .collect();
        set.into_iter().collect()
    }
    /// The displays left to right (then top to bottom)
    fn order(&self) -> Vec<usize> {
        let mut o: Vec<usize> = (0..self.displays.len()).collect();
        o.sort_by(|a, b| {
            let (p, q) = (&self.displays[*a], &self.displays[*b]);
            (p.x, p.y, &p.name).cmp(&(q.x, q.y, &q.name))
        });
        o
    }
    fn focused_idx(&self) -> Option<usize> {
        self.displays.iter().position(|d| d.name == self.focused).or(if self.displays.is_empty() { None } else { Some(0) })
    }
    /// Every numbered position: (display, space), in order. Per display: the
    /// focused display's; sequential: all of them, left to right.
    pub fn slots(&self) -> Vec<(usize, i32)> {
        let Some(f) = self.focused_idx() else { return Vec::new() };
        let list = if self.sequential { self.order() } else { vec![f] };
        list.into_iter().flat_map(|d| self.spaces(self.displays[d].base).into_iter().map(move |id| (d, id))).collect()
    }
    /// The number a space shows (0: not numbered here)
    pub fn number_of(&self, id: i32) -> i32 {
        if self.sequential {
            return self.slots().iter().position(|(_, i)| *i == id).map_or(0, |p| p as i32 + 1);
        }
        self.displays.iter().find_map(|d| self.spaces(d.base).iter().position(|i| *i == id)).map_or(0, |p| p as i32 + 1)
    }
    /// The id a new space after a display's last takes (None: its block is full)
    fn after_last(&self, d: usize) -> Option<i32> {
        let base = self.displays[d].base;
        let top = self.spaces(base).last().copied().unwrap_or(base);
        let mut id = top + 1;
        while self.workspaces.contains(&id) {
            id += 1;
        }
        (id < base + STRIDE).then_some(id)
    }
    fn window(&self) -> Option<&Win> {
        if self.window.is_empty() {
            return None;
        }
        self.windows.iter().find(|w| w.address == self.window)
    }
    fn rename(&mut self, from: i32, to: i32) {
        for w in &mut self.windows {
            if w.ws == from {
                w.ws = to;
            }
        }
        for d in &mut self.displays {
            if d.active == from {
                d.active = to;
            }
        }
        for i in &mut self.workspaces {
            if *i == from {
                *i = to;
            }
        }
    }
    fn taken(&self, id: i32) -> bool {
        self.workspaces.contains(&id) || self.on_screen(id)
    }
}

/// Close up every display's gaps: the steps and the world after them. When
/// the new id is still held by an empty space Hyprland keeps (persistent, or
/// not dropped yet), the windows move into it instead, silently, as long as
/// the space they leave isn't on screen; otherwise the rest of that display
/// waits for the next look.
pub fn tidy(world: &World) -> (Vec<Step>, World) {
    let mut w = world.clone();
    let mut steps = Vec::new();
    for d in 0..w.displays.len() {
        let base = w.displays[d].base;
        let list = w.spaces(base);
        for (i, id) in list.into_iter().enumerate() {
            let to = base + i as i32 + 1;
            if id == to {
                continue;
            }
            if w.taken(to) {
                if w.on_screen(to) || w.on_screen(id) || w.has_windows(to) {
                    break;
                }
                for win in w.windows.iter_mut().filter(|x| !x.pinned && x.ws == id) {
                    steps.push(Step::Window { address: win.address.clone(), to, follow: false });
                    win.ws = to;
                }
                // (left empty and not on screen: Hyprland drops it)
                w.workspaces.retain(|i| *i != id);
                continue;
            }
            steps.push(Step::Rename { from: id, to });
            w.rename(id, to);
        }
    }
    (steps, w)
}

/// What to do for `act`: closing up first, then the action on the tidy world
pub fn plan(world: &World, act: &Act) -> Vec<Step> {
    let (mut steps, w) = tidy(world);
    steps.extend(act_on(&w, act));
    steps
}

fn act_on(w: &World, act: &Act) -> Vec<Step> {
    let Some(f) = w.focused_idx() else { return Vec::new() };
    match act {
        Act::Tidy => Vec::new(),
        Act::At(op, n) => at(w, f, *op, *n),
        Act::Step(op, d) => {
            let from = match op {
                Op::Send | Op::Move => match w.window() {
                    Some(win) => win.ws,
                    None => return Vec::new(),
                },
                _ => w.displays[f].active,
            };
            let slots = w.slots();
            // (from a space that isn't numbered here, a guest: the first)
            let n = slots.iter().position(|(_, id)| *id == from).map_or(1, |p| p as i32 + 1 + d);
            if n < 1 {
                return Vec::new();
            }
            at(w, f, *op, n)
        }
        Act::Insert { display, index, window, follow } => {
            let Some(d) = w.displays.iter().position(|x| x.name == *display) else { return Vec::new() };
            let Some(win) = w.windows.iter().find(|x| x.address == *window).filter(|x| !x.pinned) else { return Vec::new() };
            insert(w, f, d, *index, Some(win.address.clone()), *follow)
        }
    }
}

/// Op at position n
fn at(w: &World, f: usize, op: Op, n: i32) -> Vec<Step> {
    if n < 1 {
        return Vec::new();
    }
    let slots = w.slots();
    let target = slots.get(n as usize - 1).copied();
    let here = &w.displays[f];
    // the display you're on shows an empty space at its end: "past the end" is that one
    let blank_last = !w.has_windows(here.active) && w.spaces(here.base).last() == Some(&here.active);
    match op {
        Op::Focus => match target {
            Some((_, id)) if id == here.active => Vec::new(),
            Some((_, id)) => vec![Step::Focus(id)],
            None if blank_last => Vec::new(),
            None => w.after_last(f).map(|id| vec![Step::Focus(id)]).unwrap_or_default(),
        },
        Op::Send | Op::Move => {
            let Some(win) = w.window().filter(|x| !x.pinned) else { return Vec::new() };
            match target {
                Some((_, id)) if id == win.ws => Vec::new(),
                Some((_, id)) => vec![Step::Window { address: win.address.clone(), to: id, follow: op == Op::Move }],
                None => {
                    // (alone on the last space here already: it is at the end)
                    let alone = !w.windows.iter().any(|x| !x.pinned && x.ws == win.ws && x.address != win.address);
                    if alone && w.spaces(here.base).last() == Some(&win.ws) {
                        return Vec::new();
                    }
                    // past the end: a new space with it, and go there
                    w.after_last(f).map(|id| vec![Step::Window { address: win.address.clone(), to: id, follow: true }]).unwrap_or_default()
                }
            }
        }
        Op::New | Op::NewMove => {
            let window = if op == Op::NewMove {
                match w.window().filter(|x| !x.pinned) {
                    Some(win) => Some(win.address.clone()),
                    None => return Vec::new(),
                }
            } else {
                None
            };
            match target {
                Some((d, id)) => {
                    let index = w.spaces(w.displays[d].base).iter().position(|i| *i == id).unwrap_or(0);
                    insert(w, f, d, index, window, true)
                }
                None if op == Op::New && blank_last => Vec::new(),
                None => insert(w, f, f, usize::MAX, window, true),
            }
        }
    }
}

/// A new space on display `d` in front of its `index`-th (past its last:
/// after it): the ones from there take one number more, from the end
/// backwards; then `window` goes there (or, without one, you do)
fn insert(w: &World, f: usize, d: usize, index: usize, window: Option<String>, follow: bool) -> Vec<Step> {
    let disp = &w.displays[d];
    let list = w.spaces(disp.base);
    let mut steps = Vec::new();
    let new_id = if index >= list.len() {
        match w.after_last(d) {
            Some(id) => id,
            None => return Vec::new(),
        }
    } else {
        // (the block's end, or an id still held past the last: nothing moves)
        let last = *list.last().unwrap();
        if last + 1 >= disp.base + STRIDE || w.taken(last + 1) {
            return Vec::new();
        }
        for j in (index..list.len()).rev() {
            steps.push(Step::Rename { from: list[j], to: list[j] + 1 });
        }
        list[index]
    };
    let elsewhere = d != f;
    match window {
        Some(address) => {
            steps.push(Step::Window { address, to: new_id, follow });
            if elsewhere {
                steps.push(Step::Place { id: new_id, monitor: disp.name.clone() });
            }
        }
        None => {
            if elsewhere {
                steps.push(Step::Monitor(disp.name.clone()));
            }
            steps.push(Step::Focus(new_id));
        }
    }
    steps
}

/// The steps as one Lua chunk for `hyprctl eval`
pub fn lua(steps: &[Step], q: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    for s in steps {
        out += &match s {
            Step::Rename { from, to } => format!("hl.dispatch(hl.dsp.workspace.change_id({{ workspace = {from}, id = {to} }}))\n"),
            Step::Window { address, to, follow } => {
                let a = if address.starts_with("0x") { address.clone() } else { format!("0x{address}") };
                format!("hl.dispatch(hl.dsp.window.move({{ workspace = {to}, follow = {follow}, window = {} }}))\n", q(&format!("address:{a}")))
            }
            Step::Monitor(name) => format!("hl.dispatch(hl.dsp.focus({{ monitor = {} }}))\n", q(name)),
            Step::Place { id, monitor } => format!("hl.dispatch(hl.dsp.workspace.move({{ workspace = {id}, monitor = {} }}))\n", q(monitor)),
            Step::Focus(id) => format!("hl.dispatch(hl.dsp.focus({{ workspace = {id} }}))\n"),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Step::*;

    fn win(a: &str, ws: i32) -> Win {
        Win { address: a.into(), ws, pinned: false }
    }
    fn disp(name: &str, base: i32, x: i32, active: i32) -> Display {
        Display { name: name.into(), base, x, y: 0, active }
    }
    /// One display (eDP-1, base 0) on `active`, these windows, the first one focused
    fn one(active: i32, wins: &[(&str, i32)]) -> World {
        let windows: Vec<Win> = wins.iter().map(|(a, ws)| win(a, *ws)).collect();
        let mut ids: Vec<i32> = windows.iter().map(|w| w.ws).filter(|i| *i > 0).collect();
        ids.push(active);
        ids.sort();
        ids.dedup();
        World { displays: vec![disp("eDP-1", 0, 0, active)], windows, workspaces: ids, focused: "eDP-1".into(), window: wins.first().map(|w| w.0.to_string()).unwrap_or_default(), sequential: false }
    }
    fn rn(from: i32, to: i32) -> Step {
        Rename { from, to }
    }
    fn mv(a: &str, to: i32, follow: bool) -> Step {
        Window { address: a.into(), to, follow }
    }

    #[test]
    fn the_users_case_an_emptied_space_closes_up() {
        // a window went from 3 to 4, so 3 is empty and gone: 4 and 5 move down
        let w = one(1, &[("a", 1), ("b", 2), ("c", 4), ("d", 5)]);
        let w = World { workspaces: vec![1, 2, 4, 5], ..w };
        assert_eq!(plan(&w, &Act::Tidy), vec![rn(4, 3), rn(5, 4)]);
        // then "next" from 2 lands on what was 4 (now 3), not an empty space
        let (_, t) = tidy(&w);
        let t = World { displays: vec![disp("eDP-1", 0, 0, 2)], ..t };
        assert_eq!(plan(&t, &Act::Step(Op::Focus, 1)), vec![Focus(3)]);
        // a tidy world: nothing to do
        assert_eq!(plan(&t, &Act::Tidy), vec![]);
    }

    #[test]
    fn removing_in_the_middle_keeps_the_order() {
        // 1, 3, 4, 6 have windows (2 and 5 emptied): they become 1..4
        let w = one(1, &[("a", 1), ("b", 3), ("c", 4), ("d", 6), ("e", 6)]);
        assert_eq!(plan(&w, &Act::Tidy), vec![rn(3, 2), rn(4, 3), rn(6, 4)]);
    }

    #[test]
    fn the_space_you_are_on_stays_while_you_are_there() {
        // you moved 3's last window away: 3 stays, blank, and 4 keeps its number
        let w = one(3, &[("a", 1), ("b", 2), ("c", 4)]);
        assert_eq!(w.spaces(0), vec![1, 2, 3, 4]);
        assert_eq!(plan(&w, &Act::Tidy), vec![]);
        // you left it for 4: Hyprland dropped 3, and 4 becomes 3 (renamed:
        // the windows you're looking at don't move)
        let left = World { displays: vec![disp("eDP-1", 0, 0, 4)], workspaces: vec![1, 2, 4], ..w };
        assert_eq!(plan(&left, &Act::Tidy), vec![rn(4, 3)]);
    }

    #[test]
    fn go_to_a_space_and_past_the_end() {
        let w = one(1, &[("a", 1), ("b", 2), ("c", 3)]);
        assert_eq!(plan(&w, &Act::At(Op::Focus, 2)), vec![Focus(2)]);
        assert_eq!(plan(&w, &Act::At(Op::Focus, 1)), vec![], "already there");
        // past the last: a new empty one at the end (4), whatever N
        assert_eq!(plan(&w, &Act::At(Op::Focus, 4)), vec![Focus(4)]);
        assert_eq!(plan(&w, &Act::At(Op::Focus, 9)), vec![Focus(4)]);
        // on that blank last one already: nothing more
        let blank = one(4, &[("a", 1), ("b", 2), ("c", 3)]);
        assert_eq!(plan(&blank, &Act::At(Op::Focus, 7)), vec![]);
    }

    #[test]
    fn next_and_previous() {
        let w = one(1, &[("a", 1), ("b", 2)]);
        assert_eq!(plan(&w, &Act::Step(Op::Focus, -1)), vec![], "previous stops at 1");
        assert_eq!(plan(&w, &Act::Step(Op::Focus, 1)), vec![Focus(2)]);
        let last = World { displays: vec![disp("eDP-1", 0, 0, 2)], ..w.clone() };
        assert_eq!(plan(&last, &Act::Step(Op::Focus, 1)), vec![Focus(3)], "next from the last: a new one");
        let blank = one(3, &[("a", 1), ("b", 2)]);
        assert_eq!(plan(&blank, &Act::Step(Op::Focus, 1)), vec![], "already on a new blank one");
        // the window to the next: past the end makes a space and follows
        let mut two = one(2, &[("b", 2), ("a", 1)]);
        assert_eq!(plan(&two, &Act::Step(Op::Move, -1)), vec![mv("b", 1, true)]);
        two.windows.push(win("c", 2));
        assert_eq!(plan(&two, &Act::Step(Op::Move, 1)), vec![mv("b", 3, true)]);
    }

    #[test]
    fn send_and_move_to_a_space_or_past_the_end() {
        let w = one(1, &[("a", 1), ("x", 1), ("b", 2), ("c", 3)]);
        assert_eq!(plan(&w, &Act::At(Op::Send, 3)), vec![mv("a", 3, false)]);
        assert_eq!(plan(&w, &Act::At(Op::Move, 2)), vec![mv("a", 2, true)]);
        assert_eq!(plan(&w, &Act::At(Op::Send, 1)), vec![], "its own space");
        // past the last: create, move, and go there (both keys)
        assert_eq!(plan(&w, &Act::At(Op::Send, 8)), vec![mv("a", 4, true)]);
        assert_eq!(plan(&w, &Act::At(Op::Move, 4)), vec![mv("a", 4, true)]);
        // alone on the last space: it's at the end already
        let alone = one(3, &[("c", 3), ("a", 1), ("b", 2)]);
        assert_eq!(plan(&alone, &Act::At(Op::Send, 9)), vec![]);
    }

    #[test]
    fn insert_in_the_middle_shifts_from_the_end_backwards() {
        let w = one(1, &[("a", 1), ("b", 2), ("c", 3)]);
        // Super+Shift+2: 3→4, then 2→3, then go to the new 2
        assert_eq!(plan(&w, &Act::At(Op::New, 2)), vec![rn(3, 4), rn(2, 3), Focus(2)]);
        // with the window
        assert_eq!(plan(&w, &Act::At(Op::NewMove, 1)), vec![rn(3, 4), rn(2, 3), rn(1, 2), mv("a", 1, true)]);
        // past the end: at the end
        assert_eq!(plan(&w, &Act::At(Op::New, 7)), vec![Focus(4)]);
        assert_eq!(plan(&w, &Act::At(Op::NewMove, 4)), vec![mv("a", 4, true)]);
        // a gap first: closed up, then the insert on the tidy numbers
        let gap = one(1, &[("a", 1), ("c", 3)]);
        assert_eq!(plan(&gap, &Act::At(Op::New, 2)), vec![rn(3, 2), rn(2, 3), Focus(2)]);
    }

    #[test]
    fn the_overviews_drag_inserts_without_following() {
        let w = one(2, &[("a", 1), ("b", 2), ("c", 3), ("x", 2)]);
        let act = Act::Insert { display: "eDP-1".into(), index: 1, window: "x".into(), follow: false };
        assert_eq!(plan(&w, &act), vec![rn(3, 4), rn(2, 3), mv("x", 2, false)]);
        let end = Act::Insert { display: "eDP-1".into(), index: 99, window: "x".into(), follow: false };
        assert_eq!(plan(&w, &end), vec![mv("x", 4, false)]);
    }

    #[test]
    fn pinned_and_special_windows_dont_count() {
        let mut w = one(1, &[("a", 1), ("s", -98), ("p", 4), ("c", 5)]);
        w.windows[2].pinned = true;
        w.workspaces = vec![1, 4, 5];
        // 4 holds only a pinned window: not a space; 5 closes up to 2; the
        // scratchpad (-98) is never renumbered
        assert_eq!(w.spaces(0), vec![1, 5]);
        assert_eq!(plan(&w, &Act::Tidy), vec![rn(5, 2)]);
        // a pinned window isn't sent anywhere
        let pinned = World { window: "p".into(), ..w.clone() };
        assert_eq!(plan(&pinned, &Act::At(Op::Send, 1)), vec![rn(5, 2)]);
        // a window on the scratchpad can be sent to a space
        let special = World { window: "s".into(), ..w };
        assert_eq!(plan(&special, &Act::At(Op::Send, 1)), vec![rn(5, 2), mv("s", 1, false)]);
    }

    #[test]
    fn a_held_id_takes_the_windows_instead() {
        // 2 is an empty space Hyprland still keeps (not on screen): 3's
        // windows move into it silently, and 4 renames down to 3
        let w = World { workspaces: vec![1, 2, 3, 4], ..one(1, &[("a", 1), ("c", 3), ("d", 3), ("e", 4)]) };
        assert_eq!(plan(&w, &Act::Tidy), vec![mv("c", 2, false), mv("d", 2, false), rn(4, 3)]);
        // the space you're looking at never has its windows moved: it waits
        let here = World { workspaces: vec![1, 2, 3], ..one(3, &[("a", 1), ("c", 3)]) };
        assert_eq!(plan(&here, &Act::Tidy), vec![]);
    }

    fn two_displays(sequential: bool) -> World {
        // the built-in (0) on the left with 1, 2; DP-1 (100) on the right with 101, 103
        World {
            displays: vec![disp("DP-1", 100, 1920, 101), disp("eDP-1", 0, 0, 1)],
            windows: vec![win("a", 1), win("b", 2), win("c", 101), win("d", 103)],
            workspaces: vec![1, 2, 101, 103],
            focused: "eDP-1".into(),
            window: "a".into(),
            sequential,
        }
    }

    #[test]
    fn each_display_numbers_its_own() {
        let w = two_displays(false);
        // DP-1's gap closes up within its own block
        assert_eq!(plan(&w, &Act::Tidy), vec![rn(103, 102)]);
        let (_, t) = tidy(&w);
        assert_eq!((t.number_of(2), t.number_of(102)), (2, 2));
        // Super+3 on the built-in: a new one there, not DP-1's
        assert_eq!(plan(&t, &Act::At(Op::Focus, 3)), vec![Focus(3)]);
        // on DP-1: its own 2
        let on_dp = World { focused: "DP-1".into(), ..t };
        assert_eq!(plan(&on_dp, &Act::At(Op::Focus, 2)), vec![Focus(102)]);
        assert_eq!(plan(&on_dp, &Act::At(Op::New, 1)), vec![rn(102, 103), rn(101, 102), Focus(101)]);
    }

    #[test]
    fn one_sequence_across_displays() {
        let (_, w) = tidy(&two_displays(true));
        // left to right: 1, 2 (built-in), then 3, 4 (DP-1's 101, 102)
        assert_eq!(w.slots(), vec![(1, 1), (1, 2), (0, 101), (0, 102)]);
        assert_eq!((w.number_of(101), w.number_of(102)), (3, 4));
        // Super+3 from the built-in: DP-1's first
        assert_eq!(plan(&w, &Act::At(Op::Focus, 3)), vec![Focus(101)]);
        // send to 4: DP-1's second
        assert_eq!(plan(&w, &Act::At(Op::Send, 4)), vec![mv("a", 102, false)]);
        // next from the built-in's last goes on to DP-1
        let on2 = World { displays: vec![disp("DP-1", 100, 1920, 101), disp("eDP-1", 0, 0, 2)], ..w.clone() };
        assert_eq!(plan(&on2, &Act::Step(Op::Focus, 1)), vec![Focus(101)]);
        // insert at 4: on DP-1 in front of its second, made there
        assert_eq!(plan(&w, &Act::At(Op::New, 4)), vec![rn(102, 103), Monitor("DP-1".into()), Focus(102)]);
        assert_eq!(plan(&w, &Act::At(Op::NewMove, 3)), vec![rn(102, 103), rn(101, 102), mv("a", 101, true), Place { id: 101, monitor: "DP-1".into() }]);
        // past the end: on the display you're on (the built-in's 3)
        assert_eq!(plan(&w, &Act::At(Op::Focus, 9)), vec![Focus(3)]);
        assert_eq!(plan(&w, &Act::At(Op::NewMove, 9)), vec![mv("a", 3, true)]);
    }

    #[test]
    fn sequential_removal_closes_the_whole_sequence() {
        // the built-in's 2 emptied (you're on 1): DP-1's spaces become 2 and 3
        let w = World { windows: vec![win("a", 1), win("c", 101), win("d", 102)], workspaces: vec![1, 101, 102], ..two_displays(true) };
        let (steps, t) = tidy(&w);
        assert_eq!(steps, vec![]);
        assert_eq!((t.number_of(1), t.number_of(101), t.number_of(102)), (1, 2, 3));
        // a gap on DP-1 closes within its block; the sequence follows
        let gap = World { windows: vec![win("a", 1), win("c", 101), win("d", 104)], workspaces: vec![1, 101, 104], ..two_displays(true) };
        let (steps, t) = tidy(&gap);
        assert_eq!(steps, vec![rn(104, 102)]);
        assert_eq!(t.number_of(102), 3);
    }

    #[test]
    fn the_steps_as_lua() {
        let q = |s: &str| format!("\"{s}\"");
        let l = lua(&[rn(4, 3), mv("abc", 2, true), Focus(5)], q);
        assert_eq!(
            l,
            "hl.dispatch(hl.dsp.workspace.change_id({ workspace = 4, id = 3 }))\nhl.dispatch(hl.dsp.window.move({ workspace = 2, follow = true, window = \"address:0xabc\" }))\nhl.dispatch(hl.dsp.focus({ workspace = 5 }))\n"
        );
        assert_eq!(Op::parse("newmove"), Some(Op::NewMove));
        assert_eq!(Op::parse("x"), None);
    }
}
