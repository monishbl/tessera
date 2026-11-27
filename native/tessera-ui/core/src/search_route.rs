//! Where the shell's answers for Search go (27 Nov). Search runs in two
//! hosts: its own window (surfaces/spotlight) and the Overview, which hosts
//! the same Search as you type (tessera_spotlight::search). The shell
//! answers Search's asks (`search.query`: its rows; `ai.quickAsk`: the AI's
//! progress) to the surface "spotlight" (`{"to":"spotlight",…}`, from QML
//! and from the host alike); those lines go to the part that asked last.

use std::cell::Cell;

/// The surface the shell's answers for Search are addressed to
pub const SEARCH: &str = "spotlight";

thread_local! {
    static OWNER: Cell<&'static str> = const { Cell::new(SEARCH) };
}

/// A Search in this part asked the shell something: its answers come here
pub fn asked_by(part: &'static str) {
    OWNER.with(|o| o.set(part));
}

/// The part a shell line addressed to `to` goes to
pub fn route(to: &str) -> Option<&'static str> {
    route_with(to, OWNER.with(Cell::get))
}

fn route_with(to: &str, owner: &'static str) -> Option<&'static str> {
    (to == SEARCH).then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchs_answers_go_to_whoever_asked() {
        assert_eq!(route_with("spotlight", "spotlight"), Some("spotlight"));
        assert_eq!(route_with("spotlight", "overview"), Some("overview"));
        // (any other surface's lines are its own)
        assert_eq!(route_with("osd", "overview"), None);
        assert_eq!(route("osd"), None);
        assert_eq!(route("spotlight"), Some("spotlight"));
        asked_by("overview");
        assert_eq!(route("spotlight"), Some("overview"));
        asked_by("spotlight");
        assert_eq!(route("spotlight"), Some("spotlight"));
    }
}
