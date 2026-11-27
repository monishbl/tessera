//! Wi-Fi sign-in pages (captive portals), Rust only. NetworkManager says a
//! sign-in page is in the way (Connectivity = portal) in core's status::Wifi
//! reading, the D-Bus watch the Wi-Fi menu and the bar already keep. Then,
//! while Settings → Network's "Open Wi-Fi sign-in pages automatically"
//! (`network.portalAuto`, on by default) is on, `tessera-web --portal` opens
//! NetworkManager's own check address (the network answers it with its
//! sign-in page) and closes by itself once online; the default browser when
//! tessera-web isn't installed. Once per connection: joining the same network
//! again is a new connection (tessera_core::weblink::PortalOnce).
//!
//! The Wi-Fi menu shows "Sign in to network" while it's so (bus
//! "CaptivePortal": {portal, url}, NetworkFeed passes it on), and
//! `portal.open` (its row) opens the page again, whatever the setting.

use serde_json::{json, Value};
use std::any::{Any, TypeId};
use tessera_core::host::{Host, Service};
use tessera_core::services::status::Wifi;
use tessera_core::weblink::{self, PortalOnce};

const NAME: &str = "CaptivePortal";

#[derive(Default)]
pub struct CaptivePortal {
    once: PortalOnce,
    /// the page to open: NetworkManager's check address, else GNOME's
    url: String,
}

/// Whether a reading has a sign-in page in the way (a connection, and
/// NetworkManager's word for it)
pub fn in_portal(w: &Wifi) -> bool {
    w.connectivity == weblink::PORTAL && !w.primary.is_empty()
}

impl CaptivePortal {
    fn look(&mut self, h: &mut Host) {
        let w = h.st.shell.get::<Wifi>().cloned().unwrap_or_default();
        // (a sandboxed shell shares the system bus: only the real one opens pages by itself)
        let auto = weblink::portal_auto(h.cfg()) && !tessera_core::env::sandboxed();
        self.url = weblink::probe_url(&w.check_uri);
        if self.once.step(w.connectivity, &w.primary, auto) {
            weblink::portal(&self.url);
        }
        crate::bus::put(NAME, json!({"portal": in_portal(&w), "url": self.url}));
    }
}

impl Service for CaptivePortal {
    fn name(&self) -> &'static str {
        NAME
    }
    fn calls(&self) -> &'static [&'static str] {
        &["portal"]
    }
    fn start(&mut self, h: &mut Host) {
        h.st.shell.want("status");
        self.look(h);
    }
    fn config_changed(&mut self, h: &mut Host) {
        self.look(h);
    }
    fn data(&mut self, h: &mut Host, what: TypeId) {
        if what == TypeId::of::<Wifi>() {
            self.look(h);
        }
    }
    fn call(&mut self, h: &mut Host, call: &str, _args: &Value) {
        // "portal.open": the Wi-Fi menu's "Sign in to network"
        if call == "portal.open" {
            if self.url.is_empty() {
                self.look(h);
            }
            weblink::portal(&self.url);
        }
    }
    fn message(&mut self, _h: &mut Host, _msg: Box<dyn Any + Send>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portal_needs_a_connection() {
        let mut w = Wifi { connectivity: weblink::PORTAL, primary: "/org/freedesktop/NetworkManager/ActiveConnection/3".into(), ..Default::default() };
        assert!(in_portal(&w));
        w.connectivity = 4;
        assert!(!in_portal(&w));
        w.connectivity = weblink::PORTAL;
        w.primary.clear();
        assert!(!in_portal(&w));
    }
}
