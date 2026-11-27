//! Addresses: what typed text means (an address or a search), how one is
//! shown in the bar, and the sign-in mode's --done match. Plain string work,
//! no URL crate: WebKit itself parses what it loads.

/// The engine searches go to: Settings → Default apps' (`search.engine`,
/// Google unless changed), as every search of Tessera's (tessera_core::weblink::search)
pub fn engine() -> &'static str {
    tessera_core::weblink::search::engine(&tessera_core::theme::read_json())
}

/// "scheme:" at the start (letters, then letters, digits, + - .)
pub fn has_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else { return false };
    let mut c = scheme.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic()) && c.all(|x| x.is_ascii_alphanumeric() || "+-.".contains(x)) && {
        // ("localhost:8080", "example.com:443/x": a host and port, not a scheme)
        let after = &s[scheme.len() + 1..];
        !(after.chars().next().is_some_and(|d| d.is_ascii_digit()) && (scheme.contains('.') || scheme.eq_ignore_ascii_case("localhost")))
    }
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n < 256))
}

/// What the address field's text means: an address to open, or a search
/// on the chosen engine. None for nothing typed.
pub fn typed(input: &str) -> Option<String> {
    typed_on(input, engine())
}

/// `typed` with searches on this engine
pub fn typed_on(input: &str, engine: &str) -> Option<String> {
    let search = |q: &str| search_on(engine, q);
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if has_scheme(s) {
        let lower = s.to_ascii_lowercase();
        // (what a page can be; anything else, "javascript:" too, is searched for)
        if ["http:", "https:", "file:", "about:"].iter().any(|p| lower.starts_with(p)) {
            return Some(s.to_string());
        }
        if !s.contains(char::is_whitespace) && ["mailto:", "tel:"].iter().any(|p| lower.starts_with(p)) {
            return Some(s.to_string());
        }
        return Some(search(s));
    }
    if s.contains(char::is_whitespace) {
        return Some(search(s));
    }
    let host_port = s.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_port.rsplit_once(':').filter(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).map_or(host_port, |(h, _)| h);
    if host.eq_ignore_ascii_case("localhost") || is_ipv4(host) {
        return Some(format!("http://{s}"));
    }
    let labels: Vec<&str> = host.split('.').collect();
    let tld = labels.last().copied().unwrap_or("");
    let looks_like_host = labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && tld.chars().count() >= 2
        && tld.chars().all(|c| c.is_alphabetic() || c == '-')
        || (labels.len() >= 2 && tld.starts_with("xn--"));
    if looks_like_host {
        return Some(format!("https://{s}"));
    }
    Some(search(s))
}

/// The results page for these words on this engine
pub fn search_on(engine: &str, q: &str) -> String {
    tessera_core::weblink::search::url_for(engine, q)
}

/// Percent-encoding for a query value: everything but A-Z a-z 0-9 - _ . ~
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A file's path for a file:// URI ("/" kept)
pub fn encode_path(p: &str) -> String {
    p.split('/').map(encode).collect::<Vec<_>>().join("/")
}

/// %XX back to bytes (invalid ones stay as they are)
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An http or https address (what --render may load)
pub fn is_http(uri: &str) -> bool {
    matches!(scheme_of(uri.trim()).as_str(), "http" | "https") && !host_of(uri.trim()).is_empty()
}

/// The scheme, lower case ("" for none)
pub fn scheme_of(uri: &str) -> String {
    if !has_scheme(uri) {
        return String::new();
    }
    uri.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// The host, lower case, without user info or port ("" for none)
pub fn host_of(uri: &str) -> String {
    let Some((_, rest)) = uri.split_once("://") else { return String::new() };
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
    let host = if auth.starts_with('[') { auth.split(']').next().map(|h| &h[1..]).unwrap_or("") } else { auth.split(':').next().unwrap_or("") };
    host.to_ascii_lowercase()
}

/// Google's own (lens.google.com, www.google.com, google.co.uk…): where
/// Lens's results are
pub fn is_google(uri: &str) -> bool {
    let h = host_of(uri);
    let h = h.strip_prefix("www.").unwrap_or(&h);
    h.starts_with("google.") || h.ends_with(".google.com") || h.contains(".google.")
}

/// An address as the bar shows it while it isn't being edited: no
/// https://, no www., no lone trailing slash; a file by its path
pub fn display(uri: &str) -> String {
    if let Some(p) = uri.strip_prefix("file://") {
        return decode(p);
    }
    let mut s = uri;
    for p in ["https://", "http://"] {
        if let Some(r) = s.strip_prefix(p) {
            s = r;
            break;
        }
    }
    let s = s.strip_prefix("www.").unwrap_or(s);
    let s = if s.matches('/').count() == 1 { s.strip_suffix('/').unwrap_or(s) } else { s };
    decode(s)
}

/// What the address pill says while it isn't being edited. Lens: "Search
/// by picture" while the upload and Google's results show (the film's
/// pill). Else the page's title, then --title until the page has one, then
/// the address; "" for a blank page (the field's placeholder shows).
pub fn bar_text(lens: bool, given: Option<&str>, page_title: &str, uri: &str) -> String {
    let blank = uri.is_empty() || uri == "about:blank";
    if lens && (blank || is_google(uri)) {
        return "Search by picture".into();
    }
    if !page_title.trim().is_empty() {
        return page_title.trim().to_string();
    }
    if let Some(t) = given.filter(|t| !t.trim().is_empty()) {
        return t.trim().to_string();
    }
    if blank {
        return String::new();
    }
    display(uri)
}

/// The address with its scheme and host in lower case, the default port
/// gone: two ways of writing one place compare equal
fn normal(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else { return uri.to_string() };
    let scheme = scheme.to_ascii_lowercase();
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (auth, tail) = rest.split_at(end);
    let mut auth = auth.to_ascii_lowercase();
    for (s, port) in [("http", ":80"), ("https", ":443")] {
        if scheme == s {
            if let Some(a) = auth.strip_suffix(port) {
                auth = a.to_string();
            }
        }
    }
    format!("{scheme}://{auth}{tail}")
}

/// --signin's --done: navigation reached PREFIX. The address starts with
/// it (scheme and host in any case, a default port or not); when PREFIX
/// names only a host, the address's host must be that host exactly, so
/// "https://example.com" never matches "https://example.com.evil.org/".
pub fn done_matches(uri: &str, prefix: &str) -> bool {
    let (u, p) = (normal(uri.trim()), normal(prefix.trim()));
    if p.is_empty() || !u.starts_with(&p) {
        return false;
    }
    let host_only = p.split_once("://").is_some_and(|(_, rest)| !rest.is_empty() && !rest.contains(['/', '?', '#']));
    if !host_only {
        return true;
    }
    matches!(u[p.len()..].chars().next(), None | Some('/' | '?' | '#'))
}

/// NetworkManager's connectivity check address, from its config files'
/// text in the order NetworkManager reads them (the last `uri=` in a
/// [connectivity] section wins)
pub fn connectivity_uri(files: &[String]) -> Option<String> {
    let mut found = None;
    for f in files {
        let mut section = String::new();
        for line in f.lines() {
            let l = line.trim();
            if l.starts_with('#') || l.starts_with(';') {
                continue;
            }
            if l.starts_with('[') && l.ends_with(']') {
                section = l[1..l.len() - 1].trim().to_ascii_lowercase();
                continue;
            }
            if section == "connectivity" {
                if let Some((k, v)) = l.split_once('=') {
                    if k.trim() == "uri" && !v.trim().is_empty() {
                        found = Some(v.trim().to_string());
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_opens_or_searches() {
        assert_eq!(typed("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(typed(" example.com/a?b=c ").as_deref(), Some("https://example.com/a?b=c"));
        assert_eq!(typed("https://x.org").as_deref(), Some("https://x.org"));
        assert_eq!(typed("localhost:8080/x").as_deref(), Some("http://localhost:8080/x"));
        assert_eq!(typed("192.168.1.1").as_deref(), Some("http://192.168.1.1"));
        assert_eq!(typed("example.com:8443").as_deref(), Some("https://example.com:8443"));
        // (searches: Google unless Settings chose another)
        assert_eq!(typed_on("water lily", "google").as_deref(), Some("https://www.google.com/search?q=water%20lily"));
        assert_eq!(typed_on("rust", "google").as_deref(), Some("https://www.google.com/search?q=rust"));
        assert_eq!(typed_on("3.14", "google").as_deref(), Some("https://www.google.com/search?q=3.14"));
        assert_eq!(typed_on("javascript:alert(1)", "google").as_deref(), Some("https://www.google.com/search?q=javascript%3Aalert%281%29"));
        assert_eq!(typed_on("water lily", "duckduckgo").as_deref(), Some("https://duckduckgo.com/?q=water%20lily"));
        assert_eq!(typed("about:blank").as_deref(), Some("about:blank"));
        assert_eq!(typed("   "), None);
    }

    #[test]
    fn addresses_as_the_bar_shows_them() {
        assert_eq!(display("https://www.example.com/"), "example.com");
        assert_eq!(display("https://example.com/a/b/"), "example.com/a/b/");
        assert_eq!(display("http://example.com/a%20b"), "example.com/a b");
        assert_eq!(display("file:///tmp/a%20b.html"), "/tmp/a b.html");
        assert_eq!(host_of("https://user@Example.COM:8080/x"), "example.com");
        assert_eq!(host_of("http://[::1]:80/"), "::1");
        assert_eq!(host_of("about:blank"), "");
        assert!(is_google("https://www.google.com/search?q=x"));
        assert!(is_google("https://lens.google.com/v3/upload"));
        assert!(is_google("https://www.google.co.uk/"));
        assert!(!is_google("https://notgoogle.com/"));
        assert_eq!(decode("%41%zz%"), "A%zz%");
    }

    #[test]
    fn what_the_pill_says() {
        assert_eq!(bar_text(true, None, "", "about:blank"), "Search by picture");
        assert_eq!(bar_text(true, None, "Google Lens", "https://www.google.com/search?vsrid=1"), "Search by picture");
        assert_eq!(bar_text(true, None, "Water lily - Wikipedia", "https://en.wikipedia.org/wiki/Nymphaea"), "Water lily - Wikipedia");
        assert_eq!(bar_text(false, Some("Weather details"), "", "https://example.com/"), "Weather details");
        assert_eq!(bar_text(false, Some("Weather details"), "Forecast", "https://example.com/"), "Forecast");
        assert_eq!(bar_text(false, None, "", "https://www.example.com/a"), "example.com/a");
        assert_eq!(bar_text(false, None, "", "about:blank"), "");
    }

    #[test]
    fn the_done_prefix() {
        assert!(done_matches("http://127.0.0.1:8123/cb?code=abc", "http://127.0.0.1:8123/cb"));
        assert!(done_matches("HTTPS://Example.com:443/done?x", "https://example.com/done"));
        assert!(done_matches("https://example.com/", "https://example.com"));
        assert!(done_matches("https://example.com?x=1", "https://example.com"));
        assert!(!done_matches("https://example.com.evil.org/", "https://example.com"));
        assert!(!done_matches("https://example.com:8443/", "https://example.com"));
        assert!(!done_matches("https://accounts.example/auth", "http://127.0.0.1:8123/cb"));
        assert!(!done_matches("http://127.0.0.1:8123/cb", ""));
        // (a custom scheme the app registered)
        assert!(done_matches("com.example.app:/oauth2redirect?code=1", "com.example.app:/oauth2redirect"));
        // (GNOME Online Accounts' answer: the shell's --done goa-oauth2://)
        assert!(done_matches("goa-oauth2://localhost/?code=abc&state=x", "goa-oauth2://"));
        assert!(done_matches("goa-oauth2://callback#code=1", "goa-oauth2://"));
        assert!(!done_matches("https://login.microsoftonline.com/x?redirect_uri=goa-oauth2://", "goa-oauth2://"));
    }

    #[test]
    fn network_managers_check_address() {
        let lib = "[connectivity]\nuri=http://ping.archlinux.org/nm-check.txt\n".to_string();
        let etc = "[main]\nplugins=keyfile\n# uri=http://no\n[connectivity]\nuri = http://nmcheck.gnome.org/check_network_status.txt\n".to_string();
        assert_eq!(connectivity_uri(&[lib.clone()]).as_deref(), Some("http://ping.archlinux.org/nm-check.txt"));
        assert_eq!(connectivity_uri(&[lib, etc]).as_deref(), Some("http://nmcheck.gnome.org/check_network_status.txt"));
        assert_eq!(connectivity_uri(&["[main]\nuri=x\n".to_string()]), None);
    }
}
