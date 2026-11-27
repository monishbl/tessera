//! Google Search's user-agent quirk: Google's search pages are opened as
//! Safari on a Mac, every other page with WebKitGTK's own user agent.
//!
//! Why: every Google search here landed on google.com/sorry ("unusual
//! traffic", a reCAPTCHA), the browser's and --render's alike (Search's
//! web panel), while Lens uploads worked. Measured with WebKit 6 in a new,
//! empty session, loading google.com/search?q=water+lily:
//! - WebKitGTK's own user agent ("… (X11; Linux x86_64) AppleWebKit/605.1.15
//!   (KHTML, like Gecko) Version/60.5 Safari/605.1.15") goes to /sorry;
//! - Chrome on Linux's goes to /sorry too (the engine behind it isn't
//!   Chrome's, and Google can tell);
//! - Safari on a Mac's ("… (Macintosh; Intel Mac OS X 10_15_7)
//!   AppleWebKit/605.1.15 … Version/18.1 Safari/605.1.15"), the same engine
//!   as this one, gets the results;
//! - curl, with any user agent, gets them as well: the address isn't blocked.
//! So it's the browser Google thinks it sees, not this computer. WebKit
//! itself keeps such site-specific quirks for sites that misjudge it; this
//! is one more, kept as narrow as it can be: Google's search, images and
//! Lens pages only. Google's sign-in (accounts.google.com, and the sign-in
//! and OAuth pages on its other hosts) always gets the honest user agent:
//! nothing here gets around Google's block on signing in inside an app.
//!
//! How: the view's settings carry the user agent, and WebKit puts it on
//! each request when it goes out (after the navigation is decided), so the
//! agent is set for the page being opened: before this program's own loads
//! (`set_for`), and in decide-policy for a link, a form, back or forward, a
//! reload and a redirect of the page itself (`Quirk::watch`). A frame inside
//! a page leaves it as it is: a page's frames and its pictures, scripts and
//! requests go out with the page's agent. Checked on WebKitGTK 2.54 against
//! a local server: the first request, its redirects and the page's own
//! requests all carry the agent chosen for the page.

use gtk4::glib;
use std::cell::Cell;
use std::rc::Rc;
use webkit6::prelude::*;
use webkit6::{NavigationPolicyDecision, NavigationType, PolicyDecisionType, WebView};

/// Safari on a Mac: the engine this is (AppleWebKit/605.1.15) and a
/// current Safari
pub const SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Safari/605.1.15";

/// The marker in the data folder: Google's cookies were cleared once (they
/// may remember the session as flagged from before the quirk)
pub const RESET_MARKER: &str = "google-cookies-reset";

/// A country's ending after "google.": "com", "de", "co.in", "com.au"
fn google_ending(rest: &str) -> bool {
    let labels: Vec<&str> = rest.split('.').collect();
    (1..=2).contains(&labels.len()) && labels.iter().all(|l| (2..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Google's own domain: google.com, google.co.in (and a leading dot, as a
/// cookie's domain has)
pub fn is_google_domain(name: &str) -> bool {
    let n = name.trim_start_matches('.').trim_end_matches('.').to_ascii_lowercase();
    n.strip_prefix("google.").is_some_and(google_ending)
}

/// The host of an http(s) address, lower case, without its port
fn host_of(uri: &str) -> Option<(String, &str)> {
    let (scheme, rest) = uri.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.rsplit_once(':').filter(|(_, p)| p.chars().all(|c| c.is_ascii_digit())).map_or(host, |(h, _)| h);
    Some((host.trim_end_matches('.').to_ascii_lowercase(), path))
}

/// Sign-in and OAuth pages on Google's hosts: never the quirk
fn sign_in_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    ["/accounts", "/servicelogin", "/signin", "/o/oauth2", "/oauth", "/gsi/", "/v3/signin", "/embedded/setup"].iter().any(|s| p.starts_with(s))
}

/// Whether this address gets Safari on a Mac: Google's search hosts
/// (google.*, www.google.*, images.google.*, consent.google.*,
/// lens.google.com) and any of their pages (search, /url, /sorry,
/// images) but sign-in. Never accounts.google.com or any other host.
pub fn wants_safari(uri: &str) -> bool {
    let Some((host, path)) = host_of(uri) else { return false };
    let search_host = host == "lens.google.com"
        || is_google_domain(&host)
        || ["www.", "images.", "consent."].iter().any(|p| host.strip_prefix(p).is_some_and(is_google_domain));
    search_host && !sign_in_path(path)
}

/// Google's own host or one under it (www.google.com, ipv4.google.com)
fn google_host(host: &str) -> bool {
    is_google_domain(host) || host.split_once('.').is_some_and(|(_, rest)| is_google_domain(rest))
}

/// Google's robot check ("unusual traffic", a reCAPTCHA): google.*/sorry.
/// Never drawn by --render: the page shows the computer's address
pub fn is_robot_check(uri: &str) -> bool {
    host_of(uri).is_some_and(|(host, path)| {
        let p = path.split(['?', '#']).next().unwrap_or("");
        google_host(&host) && (p == "/sorry" || p.starts_with("/sorry/"))
    })
}

/// A page with Google's reCAPTCHA form, as a script in it sees (for a page
/// of Google's that isn't at /sorry): "yes" or "no"
pub const CAPTCHA_JS: &str = r#"(function () {
  var g = /(^|\.)google\.[a-z.]+$/.test(location.hostname);
  var f = document.querySelector('form#captcha-form, #captcha-form, .g-recaptcha, iframe[src*="/recaptcha/"]');
  return g && !!f ? 'yes' : 'no';
})()"#;

/// Google's results page (/search on a search host), the robot check passed
pub fn is_results_page(uri: &str) -> bool {
    wants_safari(uri) && !is_robot_check(uri) && host_of(uri).is_some_and(|(_, path)| path.split(['?', '#']).next() == Some("/search"))
}

/// The note Search reads (its web results back off after a robot check
/// until Google lets this browser through): data folder/google-ok.json
pub const OK_FILE: &str = "google-ok.json";

pub fn ok_json(now: u64) -> String {
    format!("{{\"at\": {now}}}")
}

/// Whether a navigation decided now is the page's own (so its agent follows
/// the address): a link, a form, back or forward, a reload; a redirect
/// while the page is still loading (a frame's redirect comes after the page
/// has committed). An "other" one that isn't a redirect is a frame loading
/// or a script's; this program's own loads set the agent before they start.
pub fn follows(other: bool, redirect: bool, page_loading: bool) -> bool {
    if redirect {
        page_loading
    } else {
        !other
    }
}

/// The quirk for one view
pub struct Quirk {
    settings: webkit6::Settings,
    /// WebKitGTK's own user agent (as the settings had it at the start)
    honest: String,
    /// off: the sign-in and Wi-Fi sign-in windows (the honest agent only)
    on: bool,
    /// the page is loading, not yet committed
    page_loading: Cell<bool>,
}

impl Quirk {
    pub fn new(view: &WebView, on: bool) -> Rc<Self> {
        let settings = WebViewExt::settings(view).unwrap_or_default();
        let honest = settings.user_agent().map(|s| s.to_string()).unwrap_or_default();
        Rc::new(Quirk { settings, honest, on, page_loading: Cell::new(false) })
    }

    /// Whether this view uses the quirk (not the sign-in windows)
    pub fn on(&self) -> bool {
        self.on
    }

    /// The agent for this address, set before it's opened
    pub fn set_for(&self, uri: &str) {
        if !self.on {
            return;
        }
        let want = if wants_safari(uri) { SAFARI_MAC } else { self.honest.as_str() };
        if !self.settings.user_agent().is_some_and(|u| u.as_str() == want) {
            self.settings.set_user_agent(Some(want));
        }
    }

    /// Follows the page's own navigations. Connected after the program's
    /// own decide-policy, so it sees only what goes ahead.
    pub fn watch(self: &Rc<Self>, view: &WebView) {
        let q = self.clone();
        view.connect_load_changed(move |_, ev| match ev {
            webkit6::LoadEvent::Started => q.page_loading.set(true),
            webkit6::LoadEvent::Committed | webkit6::LoadEvent::Finished => q.page_loading.set(false),
            _ => {}
        });
        let q = self.clone();
        view.connect_decide_policy(move |_, decision, kind| {
            if kind == PolicyDecisionType::NavigationAction {
                if let Some(action) = decision.downcast_ref::<NavigationPolicyDecision>().and_then(|d| d.navigation_action()) {
                    let uri = action.request().and_then(|r| r.uri()).map(|s| s.to_string()).unwrap_or_default();
                    if follows(action.navigation_type() == NavigationType::Other, action.is_redirect(), q.page_loading.get()) {
                        q.set_for(&uri);
                    }
                }
            }
            false
        });
    }
}

/// Once per session folder: Google's cookies cleared (only Google's), as
/// they may still mark the session as the one that was sent to /sorry.
/// Then `then` (at once when it's done already, or after a moment at most).
pub fn reset_once(session: &webkit6::NetworkSession, data_dir: &std::path::Path, then: impl FnOnce() + 'static) {
    let marker = data_dir.join(RESET_MARKER);
    let Some(manager) = session.website_data_manager().filter(|_| !marker.exists()) else {
        then();
        return;
    };
    let then: Rc<Cell<Option<Box<dyn FnOnce()>>>> = Rc::new(Cell::new(Some(Box::new(then))));
    let run = {
        let then = then.clone();
        move || {
            if let Some(f) = then.take() {
                f();
            }
        }
    };
    {
        // (a data store that never answers doesn't keep the page waiting)
        let run = run.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), run);
    }
    let types = webkit6::WebsiteDataTypes::COOKIES;
    let m2 = manager.clone();
    manager.fetch(types, None::<&gtk4::gio::Cancellable>, move |r| {
        let google: Vec<webkit6::WebsiteData> = match r {
            Ok(all) => all.into_iter().filter(|d| d.name().is_some_and(|n| is_google_domain(n.as_str()))).collect(),
            Err(e) => {
                eprintln!("tessera-web: the cookies weren't read: {e}");
                run();
                return;
            }
        };
        if google.is_empty() {
            let _ = std::fs::write(&marker, b"");
            run();
            return;
        }
        let refs: Vec<&webkit6::WebsiteData> = google.iter().collect();
        // (the answer comes on this thread: the guard only carries it there)
        let run = glib::thread_guard::ThreadGuard::new(run);
        m2.remove(types, &refs, None::<&gtk4::gio::Cancellable>, move |r| {
            match r {
                Ok(()) => {
                    let _ = std::fs::write(&marker, b"");
                }
                Err(e) => eprintln!("tessera-web: Google's cookies weren't cleared: {e}"),
            }
            (run.into_inner())();
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn googles_search_pages_get_safari() {
        for u in [
            "https://www.google.com/search?q=water+lily",
            "https://google.com/search?q=x",
            "https://www.google.co.in/search?q=x",
            "https://www.google.com.au/url?q=https://example.com",
            "https://www.google.com/sorry/index?continue=x",
            "https://www.google.com/",
            "https://www.google.de",
            "https://images.google.com/imghp",
            "https://www.google.com/imgres?imgurl=x",
            "https://lens.google.com/upload?ep=x",
            "https://consent.google.com/ml?continue=x",
            "HTTPS://WWW.GOOGLE.COM/search?q=x",
            "https://www.google.com:443/search?q=x",
            "https://www.google.com./search?q=x",
            "http://www.google.com/search?q=x",
        ] {
            assert!(wants_safari(u), "{u}");
        }
    }

    #[test]
    fn sign_in_and_other_sites_keep_the_honest_agent() {
        for u in [
            "https://accounts.google.com/ServiceLogin",
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=x",
            "https://www.google.com/accounts/ServiceLogin",
            "https://www.google.com/o/oauth2/auth",
            "https://www.google.com/signin",
            "https://www.google.com/gsi/client",
            "https://mail.google.com/",
            "https://myaccount.google.com/",
            "https://maps.google.com/",
            "https://lens.google.co.in/",
            "https://www.youtube.com/results?search_query=x",
            "https://www.gstatic.com/x.js",
            "https://google.example.com/search",
            "https://www.google.com.evil.org/search",
            "https://notgoogle.com/search",
            "https://www.google.attacker/search",
            "https://www.google.comm/search",
            "https://user@evil.org/@www.google.com",
            "https://duckduckgo.com/?q=google.com",
            "https://example.com/?u=https://www.google.com/search",
            "about:blank",
            "file:///home/x/www.google.com/search",
            "",
        ] {
            assert!(!wants_safari(u), "{u}");
        }
    }

    #[test]
    fn a_user_in_the_address_doesnt_fool_it() {
        // (the host is after the @: this is google's)
        assert!(wants_safari("https://someone@www.google.com/search?q=x"));
        assert!(!wants_safari("https://www.google.com@evil.org/search"));
    }

    #[test]
    fn googles_robot_check() {
        for u in [
            "https://www.google.com/sorry/index?continue=https://www.google.com/search%3Fq%3Dx&q=EgQ",
            "https://www.google.co.in/sorry/index",
            "https://ipv4.google.com/sorry/index?continue=x",
            "https://google.com/sorry",
            "https://www.google.com/sorry?x=1",
            "HTTPS://WWW.GOOGLE.COM/sorry/index",
        ] {
            assert!(is_robot_check(u), "{u}");
        }
        for u in [
            "https://www.google.com/search?q=sorry",
            "https://www.google.com/search?q=x#sorry/",
            "https://www.google.com/sorryish",
            "https://example.com/sorry/index",
            "https://www.google.com.evil.org/sorry/index",
            "https://www.google.com/",
            "",
        ] {
            assert!(!is_robot_check(u), "{u}");
        }
        assert!(CAPTCHA_JS.contains("captcha-form") && CAPTCHA_JS.contains("google"));
    }

    #[test]
    fn a_results_page_means_google_let_it_through() {
        assert!(is_results_page("https://www.google.com/search?q=water+lily"));
        assert!(is_results_page("https://www.google.co.in/search?q=x&sei=y"));
        assert!(!is_results_page("https://www.google.com/sorry/index?continue=https://www.google.com/search"));
        assert!(!is_results_page("https://www.google.com/"));
        assert!(!is_results_page("https://www.google.com/searchx"));
        assert!(!is_results_page("https://duckduckgo.com/search?q=x"));
        assert!(!is_results_page("https://accounts.google.com/search"));
        assert_eq!(ok_json(1_764_000_000), r#"{"at": 1764000000}"#);
    }

    #[test]
    fn googles_cookie_domains() {
        for d in ["google.com", ".google.com", "google.co.in", "google.com.au", "GOOGLE.DE"] {
            assert!(is_google_domain(d), "{d}");
        }
        for d in ["youtube.com", "accounts.google.com", "google", "google.", "googleapis.com", "notgoogle.com", "google.example.org", "google.c0m"] {
            assert!(!is_google_domain(d), "{d}");
        }
    }

    #[test]
    fn the_agent_follows_the_page_not_its_frames() {
        // a link, a form, back or forward, a reload: the page's own
        assert!(follows(false, false, false));
        assert!(follows(false, false, true));
        // a redirect while the page loads: the page's
        assert!(follows(true, true, true));
        assert!(follows(false, true, true));
        // a redirect after the page committed: a frame's
        assert!(!follows(true, true, false));
        // a frame (or a script) loading: never
        assert!(!follows(true, false, false));
        assert!(!follows(true, false, true));
    }

    #[test]
    fn the_agent_is_safari_on_a_mac_with_this_engine() {
        assert!(SAFARI_MAC.contains("Macintosh"));
        assert!(SAFARI_MAC.contains("AppleWebKit/605.1.15"));
        assert!(SAFARI_MAC.contains("Version/18."));
        assert!(!SAFARI_MAC.contains("Chrome"));
    }
}
