//! `tessera-web --render URL --out DIR …`: a page drawn out of sight, for a
//! picture of it that the shell draws natively (Search's live web results,
//! a desktop web clip) and clicks mapped through its links.
//!
//! Never on screen: the page is a WebKitWebView in a GTK window that is
//! never shown (never presented, mapped or even realized, so there is no
//! Wayland surface at all: nothing for the compositor to place, nothing in
//! Alt-Tab, the dock or the Overview, no flash). Nothing lays an unshown
//! window out, so the view is allocated its size here by hand; WebKit lays
//! the page out at that size in its web process, and
//! webkit_web_view_get_snapshot has the web process paint it into a
//! picture (it paints the page itself, flattened, not what a compositor
//! shows, so it needs no window on screen). In this program's own WebKit
//! session (its cookies and consent, as the quick-look browser's) with the
//! ad blocker.
//!
//! Each render writes DIR/shot.png (the area asked for, in the picture's
//! pixels) and DIR/links.json: [{x, y, w, h, href, title}] for the links in
//! the picture (its pixels); a Google results page's results also carry
//! `result: true`, `url` and `snippet`. Both are written whole (a new file
//! moved over the old), then one line goes out on stdout:
//! {"event":"rendered","png":…,"links":…,"width":…,"height":…,"scale":…,"url":…}
//! With --every it stays and loads the page again every SECONDS; without,
//! it exits after one (0), or after {"event":"error","message":…} (1).

use crate::args::{Area, Render};
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use serde_json::{json, Value};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;
use webkit6::prelude::*;
use webkit6::{LoadEvent, NetworkSession, SnapshotOptions, SnapshotRegion, UserContentManager, WebView};

/// After the page has loaded: a moment for its scripts to finish drawing
const SETTLE: Duration = Duration::from_millis(400);
/// Until then, looked at this often: is the element there, the fonts in?
/// (a results page streams its results in after it says it has loaded)
const POLL: Duration = Duration::from_millis(250);
/// and drawn as it is after this long
const READY_MOST: Duration = Duration::from_secs(6);

/// Ready to draw: loaded, its fonts in, and the element asked for there
const READY_JS: &str = r#"(function (sel) {
  var el = null;
  try { el = sel ? document.querySelector(sel) : document.body; } catch (e) { el = document.body; }
  return document.readyState === 'complete' && !!el && (!document.fonts || document.fonts.status === 'loaded') ? 'yes' : 'no';
})"#;
/// After it scrolled to the area: a moment for the scroll to take
const SCROLLED: Duration = Duration::from_millis(120);
/// A page that never finishes loading is drawn as it is by then
const GIVE_UP: Duration = Duration::from_secs(15);

/// Phase one, in the page: where the area is (page CSS px), the page
/// scrolled to it; with a selector, what's fixed to the window (a cookie
/// bar, a sign-in prompt, a sticky header) hidden so it can't cover it
const MEASURE_JS: &str = r#"(function (c) {
  var a = null, el = null;
  if (c.selector) {
    try { el = document.querySelector(c.selector); } catch (e) { el = null; }
    if (el) {
      var b = el.getBoundingClientRect();
      if (b.width > 0 && b.height > 0) {
        a = { x: b.left + scrollX, y: b.top + scrollY, w: b.width, h: b.height };
        var all = document.body ? document.body.querySelectorAll('*') : [];
        for (var i = 0; i < all.length; i++) {
          var n = all[i];
          if (el.contains(n) || n.contains(el)) continue;
          var p = getComputedStyle(n).position;
          if (p === 'fixed' || p === 'sticky') n.style.setProperty('visibility', 'hidden', 'important');
        }
      }
    }
  }
  if (!a && c.clip) a = { x: c.clip[0], y: c.clip[1], w: c.clip[2], h: c.clip[3] };
  if (!a) a = { x: 0, y: 0, w: innerWidth, h: c.height };
  a.h = Math.min(a.h, c.height);
  window.scrollTo(0, Math.max(0, a.y));
  return JSON.stringify({ x: a.x, y: a.y, w: a.w, h: a.h, found: !!a && (!c.selector || !!el) });
})"#;

/// Phase two: the area where the window shows it now, and the links in it
/// (CSS px from the area's top-left, cut to it); a Google results page's
/// results with their titles, addresses and snippets
const LINKS_JS: &str = r#"(function (A) {
  var ox = A.x - scrollX, oy = A.y - scrollY;
  var google = /(^|\.)google\.[a-z.]+$/.test(location.hostname);
  function clean(h) {
    try {
      var u = new URL(h, location.href);
      if (google && u.pathname === '/url') {
        var q = u.searchParams.get('q') || u.searchParams.get('url');
        if (q) return q;
      }
      return u.href;
    } catch (e) { return h; }
  }
  function text(s) { return (s || '').replace(/\s+/g, ' ').trim(); }
  var results = new Map();
  if (google) {
    document.querySelectorAll('a h3').forEach(function (h3) {
      var a = h3.closest('a');
      if (!a || results.has(a)) return;
      var n = a, snip = '';
      for (var k = 0; k < 6 && n && !snip; k++) {
        n = n.parentElement;
        if (!n) break;
        var s = n.querySelector('.VwiC3b, [data-sncf], [style*="line-clamp"]');
        if (s && !a.contains(s)) snip = s.innerText;
      }
      results.set(a, { title: text(h3.innerText), url: clean(a.href), snippet: text(snip).slice(0, 320) });
    });
  }
  var out = [];
  document.querySelectorAll('a[href]').forEach(function (a) {
    var href = clean(a.href);
    if (!/^https?:/i.test(href)) return;
    var b = a.getBoundingClientRect();
    if (b.width < 2 || b.height < 2) return;
    var x = b.left - ox, y = b.top - oy;
    var x0 = Math.max(0, x), y0 = Math.max(0, y);
    var x1 = Math.min(A.w, x + b.width), y1 = Math.min(A.h, y + b.height);
    if (x1 - x0 < 2 || y1 - y0 < 2) return;
    if (getComputedStyle(a).visibility === 'hidden') return;
    var e = { x: x0, y: y0, w: x1 - x0, h: y1 - y0, href: href, title: text(a.innerText || a.getAttribute('aria-label') || a.title).slice(0, 200) };
    var r = results.get(a);
    if (r) { e.result = true; e.title = r.title; e.url = r.url; e.snippet = r.snippet; }
    out.push(e);
  });
  return JSON.stringify({ ox: ox, oy: oy, vw: innerWidth, links: out });
})"#;

/// The area, from phase one
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A rectangle in the picture's pixels, inside it (None: nothing left)
pub fn crop(ox: f64, oy: f64, w: f64, h: f64, k: f64, tex_w: i32, tex_h: i32) -> Option<(i32, i32, i32, i32)> {
    let x0 = ((ox * k).round() as i32).clamp(0, tex_w);
    let y0 = ((oy * k).round() as i32).clamp(0, tex_h);
    let x1 = (((ox + w) * k).round() as i32).clamp(0, tex_w);
    let y1 = (((oy + h) * k).round() as i32).clamp(0, tex_h);
    (x1 - x0 >= 1 && y1 - y0 >= 1).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// links.json's entries: the page's (CSS px from the area's corner) in the
/// picture's pixels, cut to the picture (`pw` × `ph`)
pub fn links_in_picture(links: &[Value], k: f64, pw: i32, ph: i32) -> Vec<Value> {
    let mut out = Vec::new();
    for l in links {
        let f = |n: &str| l[n].as_f64().unwrap_or(0.0);
        let (x0, y0) = ((f("x") * k).round().max(0.0), (f("y") * k).round().max(0.0));
        let (x1, y1) = (((f("x") + f("w")) * k).round().min(pw as f64), ((f("y") + f("h")) * k).round().min(ph as f64));
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            continue;
        }
        let href = l["href"].as_str().unwrap_or("");
        if !crate::nav::is_http(href) {
            continue;
        }
        let mut e = json!({"x": x0 as i64, "y": y0 as i64, "w": (x1 - x0) as i64, "h": (y1 - y0) as i64, "href": href, "title": l["title"].as_str().unwrap_or("")});
        if l["result"].as_bool() == Some(true) {
            e["result"] = json!(true);
            e["url"] = json!(l["url"].as_str().filter(|u| crate::nav::is_http(u)).unwrap_or(href));
            e["snippet"] = json!(l["snippet"].as_str().unwrap_or(""));
        }
        out.push(e);
    }
    out
}

/// The page's settings for phase one
fn measure_cfg(r: &Render) -> Value {
    let (clip, selector) = match &r.area {
        Area::Top => (Value::Null, Value::Null),
        Area::Clip(c) => (json!([c.x, c.y, c.w, c.h]), Value::Null),
        Area::Selector(s) => (Value::Null, json!(s)),
    };
    json!({"height": r.height, "clip": clip, "selector": selector})
}

/// Write a file whole: a new one beside it, moved over it
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

struct R {
    cfg: Render,
    view: WebView,
    /// Google's results as Safari on a Mac (src/quirk.rs)
    quirk: Rc<crate::quirk::Quirk>,
    // (never shown: it only holds the view, as a window holds a page)
    _window: gtk::Window,
    main_loop: glib::MainLoop,
    /// this load's number: a newer load (--every) makes an older render stop
    pass: Cell<u64>,
    /// a render of this load is under way or done
    taken: Cell<u64>,
    /// how it ended: 0 still going, 1 rendered, 2 failed
    code: Cell<i32>,
}

impl R {
    fn shot_path(&self) -> PathBuf {
        self.cfg.out.join("shot.png")
    }
    fn links_path(&self) -> PathBuf {
        self.cfg.out.join("links.json")
    }

    /// Lay the page out at the size asked for: the view's allocation (an
    /// unshown window is never laid out, so it's given here)
    fn size(&self) {
        let w = ((self.cfg.width as f64) * self.cfg.scale).round() as i32;
        let h = ((self.cfg.height as f64) * self.cfg.scale).round() as i32;
        self.view.set_size_request(w, h);
        let _ = self.view.measure(gtk::Orientation::Horizontal, -1);
        let _ = self.view.measure(gtk::Orientation::Vertical, w);
        self.view.queue_allocate();
        self.view.allocate(w, h, -1, None);
    }

    fn fail(self: &Rc<Self>, why: &str) {
        crate::emit(json!({"event": "error", "message": why, "url": self.cfg.url}));
        self.done(false);
    }

    /// One render over: once, then out; with --every, again in a while
    fn done(self: &Rc<Self>, ok: bool) {
        match self.cfg.every {
            None => {
                self.code.set(if ok { 1 } else { 2 });
                self.main_loop.quit();
            }
            Some(s) => {
                let me = self.clone();
                glib::timeout_add_local_once(Duration::from_secs(s), move || me.load());
            }
        }
    }

    fn load(self: &Rc<Self>) {
        let pass = self.pass.get() + 1;
        self.pass.set(pass);
        self.size();
        self.quirk.set_for(&self.cfg.url);
        self.view.load_uri(&self.cfg.url);
        // (a page that never finishes: drawn as it is)
        let me = self.clone();
        glib::timeout_add_local_once(GIVE_UP, move || me.take(pass));
    }

    /// Render this load (once) when it's ready, a moment after
    fn settle(self: &Rc<Self>) {
        let pass = self.pass.get();
        self.poll(pass, std::time::Instant::now() + READY_MOST);
    }

    fn poll(self: &Rc<Self>, pass: u64, until: std::time::Instant) {
        if pass != self.pass.get() || self.taken.get() >= pass {
            return;
        }
        let sel = match &self.cfg.area {
            Area::Selector(s) => json!(s),
            _ => Value::Null,
        };
        let me = self.clone();
        self.view.evaluate_javascript(&format!("{READY_JS}({sel})"), None, Some("tessera-render"), None::<&gio::Cancellable>, move |r| {
            let ready = r.is_ok_and(|v| v.to_str().as_str() == "yes");
            let me2 = me.clone();
            if ready || std::time::Instant::now() >= until {
                glib::timeout_add_local_once(SETTLE, move || me2.take(pass));
            } else {
                glib::timeout_add_local_once(POLL, move || me2.poll(pass, until));
            }
        });
    }

    fn take(self: &Rc<Self>, pass: u64) {
        if pass != self.pass.get() || self.taken.get() >= pass {
            return;
        }
        self.taken.set(pass);
        self.size();
        let script = format!("{MEASURE_JS}({})", measure_cfg(&self.cfg));
        let me = self.clone();
        self.view.evaluate_javascript(&script, None, Some("tessera-render"), None::<&gio::Cancellable>, move |r| {
            let found = r.ok().map(|v| v.to_str().to_string()).and_then(|s| serde_json::from_str::<Value>(&s).ok());
            let Some(f) = found else { return me.fail("the page couldn't be measured") };
            let n = |k: &str| f[k].as_f64().unwrap_or(0.0);
            let area = Found { x: n("x"), y: n("y"), w: n("w"), h: n("h") };
            if area.w < 1.0 || area.h < 1.0 {
                return me.fail("nothing to show in the area asked for");
            }
            let me2 = me.clone();
            glib::timeout_add_local_once(SCROLLED, move || me2.links_then_shoot(pass, area));
        });
    }

    fn links_then_shoot(self: &Rc<Self>, pass: u64, area: Found) {
        if pass != self.pass.get() {
            return;
        }
        let script = format!("{LINKS_JS}({})", json!({"x": area.x, "y": area.y, "w": area.w, "h": area.h}));
        let me = self.clone();
        self.view.evaluate_javascript(&script, None, Some("tessera-render"), None::<&gio::Cancellable>, move |r| {
            let got = match r {
                Ok(v) => serde_json::from_str::<Value>(&v.to_str()).unwrap_or(Value::Null),
                Err(e) => {
                    // (the picture still goes out, without its links)
                    eprintln!("tessera-web: the page's links weren't read: {e}");
                    Value::Null
                }
            };
            let me2 = me.clone();
            me.view.snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, None::<&gio::Cancellable>, move |shot| match shot {
                Ok(tex) => me2.write(pass, area, &got, &tex),
                Err(e) => me2.fail(&format!("the page couldn't be drawn: {e}")),
            });
        });
    }

    /// The picture and its links into the folder, then the line on stdout
    fn write(self: &Rc<Self>, pass: u64, area: Found, got: &Value, tex: &gdk::Texture) {
        if pass != self.pass.get() {
            return;
        }
        let (tw, th) = (tex.width(), tex.height());
        // (picture pixels per CSS px: what the snapshot came out at)
        let vw = got["vw"].as_f64().filter(|v| *v > 0.0).unwrap_or(self.cfg.width as f64);
        let k = tw as f64 / vw;
        let (ox, oy) = (got["ox"].as_f64().unwrap_or(area.x), got["oy"].as_f64().unwrap_or(area.y));
        let Some((x0, y0, cw, ch)) = crop(ox, oy, area.w, area.h, k, tw, th) else { return self.fail("the area asked for is off the page") };
        let stride = tw as usize * 4;
        let mut all = vec![0u8; stride * th as usize];
        tex.download(&mut all, stride);
        let row = cw as usize * 4;
        let mut cut = Vec::with_capacity(row * ch as usize);
        for y in y0..y0 + ch {
            let at = y as usize * stride + x0 as usize * 4;
            cut.extend_from_slice(&all[at..at + row]);
        }
        drop(all);
        let picture = gdk::MemoryTexture::new(cw, ch, gdk::MemoryFormat::B8g8r8a8Premultiplied, &glib::Bytes::from_owned(cut), row);
        if let Err(e) = std::fs::create_dir_all(&self.cfg.out) {
            return self.fail(&format!("{}: {e}", self.cfg.out.display()));
        }
        let png = self.shot_path();
        let tmp = png.with_extension("tmp.png");
        if let Err(e) = picture.save_to_png(&tmp).map_err(|e| e.to_string()).and_then(|_| std::fs::rename(&tmp, &png).map_err(|e| e.to_string())) {
            let _ = std::fs::remove_file(&tmp);
            return self.fail(&format!("the picture wasn't written: {e}"));
        }
        let links = links_in_picture(got["links"].as_array().map(|a| a.as_slice()).unwrap_or(&[]), k, cw, ch);
        let lp = self.links_path();
        if let Err(e) = write_whole(&lp, serde_json::to_string(&links).unwrap_or_else(|_| "[]".into()).as_bytes()) {
            return self.fail(&format!("the links weren't written: {e}"));
        }
        let uri = self.view.uri().map(|s| s.to_string()).unwrap_or_default();
        crate::emit(json!({"event": "rendered", "png": png, "links": lp, "width": cw, "height": ch, "scale": k, "url": uri}));
        self.done(true);
    }
}

/// --render: 0 rendered, 1 it couldn't (the error is on stdout)
pub fn run(cfg: Render) -> i32 {
    let palette = crate::theme::current();
    if let Some(s) = gtk::Settings::default() {
        // (a page that follows the system's light or dark: the shell's)
        s.set_property("gtk-application-prefer-dark-theme", palette.dark);
    }
    let (data, cache) = (crate::data_dir(), crate::cache_dir());
    for d in [&data, &cache] {
        let _ = std::fs::create_dir_all(d);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
    }
    let (site, site_cache) = (data.join("site").to_string_lossy().into_owned(), cache.join("site").to_string_lossy().into_owned());
    let session = NetworkSession::new(Some(site.as_str()), Some(site_cache.as_str()));
    session.set_itp_enabled(true);
    let content = UserContentManager::new();
    let settings = webkit6::Settings::new();
    settings.set_enable_developer_extras(false);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_media_playback_requires_user_gesture(true);
    settings.set_allow_file_access_from_file_urls(false);
    settings.set_allow_universal_access_from_file_urls(false);
    settings.set_enable_smooth_scrolling(false);
    let view = WebView::builder().network_session(&session).user_content_manager(&content).settings(&settings).build();
    // (Google's results as Safari on a Mac, as the browser's: src/quirk.rs)
    let quirk = crate::quirk::Quirk::new(&view, true);
    view.set_is_muted(true);
    view.set_zoom_level(cfg.scale);
    let pg = palette.page;
    view.set_background_color(&gdk::RGBA::new(pg[0] as f32 / 255.0, pg[1] as f32 / 255.0, pg[2] as f32 / 255.0, 1.0));
    // (the window is never presented: no surface, so never on screen)
    let window = gtk::Window::builder().title("Tessera Web render").decorated(false).build();
    window.set_child(Some(&view));
    let main_loop = glib::MainLoop::new(None, false);
    let r = Rc::new(R { cfg, view, quirk, _window: window, main_loop: main_loop.clone(), pass: Cell::new(0), taken: Cell::new(0), code: Cell::new(0) });
    {
        let me = r.clone();
        r.view.connect_load_changed(move |_, ev| {
            if ev == LoadEvent::Finished {
                me.settle();
            }
        });
        let me = r.clone();
        r.view.connect_load_failed(move |_, _, _, e| {
            if !e.matches(webkit6::NetworkError::Cancelled) {
                me.taken.set(me.pass.get());
                me.fail(&format!("the page didn't load: {e}"));
            }
            true
        });
        // (no new windows, no dialogs: nobody would see them)
        r.view.connect_create(|_, _| None);
        r.view.connect_script_dialog(|_, d| {
            d.close();
            true
        });
        r.view.connect_decide_policy(|_, decision, kind| {
            if kind == webkit6::PolicyDecisionType::Response {
                if let Some(d) = decision.downcast_ref::<webkit6::ResponsePolicyDecision>() {
                    if d.is_main_frame_main_resource() && !d.is_mime_type_supported() {
                        // (a download: never here)
                        decision.ignore();
                        return true;
                    }
                }
            }
            false
        });
        r.view.connect_permission_request(|_, req| {
            req.deny();
            true
        });
        // (after the decide-policy above: the agent follows the page's own navigations)
        r.quirk.watch(&r.view);
    }
    // Once, Google's cookies from before the user agent quirk
    // (src/quirk.rs); then the blocker (as the browser's), then the page
    {
        let me = r.clone();
        crate::quirk::reset_once(&session, &data, move || {
            let started = Rc::new(Cell::new(false));
            let (s2, me2) = (started.clone(), me.clone());
            crate::with_blocker(&content, move || {
                if !s2.replace(true) {
                    me2.load();
                }
            });
            glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                if !started.replace(true) {
                    me.load();
                }
            });
        });
    }
    main_loop.run();
    if r.code.get() == 1 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_area_in_the_picture() {
        // (2 px per CSS px; the area 10 CSS px down, 300 × 200)
        assert_eq!(crop(0.0, 10.0, 300.0, 200.0, 2.0, 1280, 1200), Some((0, 20, 600, 400)));
        // (cut to the picture)
        assert_eq!(crop(0.0, 500.0, 300.0, 200.0, 2.0, 1280, 1200), Some((0, 1000, 600, 200)));
        assert_eq!(crop(0.0, 700.0, 300.0, 200.0, 2.0, 1280, 1200), None);
        assert_eq!(crop(-5.0, 0.0, 10.0, 10.0, 1.0, 100, 100), Some((0, 0, 5, 10)));
    }

    #[test]
    fn links_in_the_pictures_pixels() {
        let l = vec![
            json!({"x": 10, "y": 20, "w": 100, "h": 18, "href": "https://example.com/", "title": "Example"}),
            json!({"x": 0, "y": 290, "w": 50, "h": 30, "href": "https://cut.example/", "title": "Cut"}),
            json!({"x": 0, "y": 400, "w": 50, "h": 30, "href": "https://below.example/", "title": "Below"}),
            json!({"x": 0, "y": 0, "w": 50, "h": 30, "href": "javascript:alert(1)", "title": "Script"}),
            json!({"x": 0, "y": 40, "w": 300, "h": 20, "href": "https://www.google.com/url?q=x", "title": "Water lily", "result": true, "url": "https://en.wikipedia.org/wiki/Nymphaea", "snippet": "A water lily…"}),
        ];
        let out = links_in_picture(&l, 2.0, 600, 600);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0], json!({"x": 20, "y": 40, "w": 200, "h": 36, "href": "https://example.com/", "title": "Example"}));
        assert_eq!((out[1]["y"].as_i64(), out[1]["h"].as_i64()), (Some(580), Some(20)));
        assert_eq!(out[2]["url"], "https://en.wikipedia.org/wiki/Nymphaea");
        assert_eq!(out[2]["result"], true);
        assert_eq!(out[2]["snippet"], "A water lily…");
    }

    #[test]
    fn the_pages_settings() {
        let r = |area: Area| Render { url: "https://a.b".into(), out: "/tmp/x".into(), width: 640, height: 480, area, every: None, scale: 2.0 };
        assert_eq!(measure_cfg(&r(Area::Top)), json!({"height": 480, "clip": null, "selector": null}));
        assert_eq!(measure_cfg(&r(Area::Selector("#rso".into())))["selector"], "#rso");
        assert_eq!(measure_cfg(&r(Area::Clip(crate::args::Rect { x: 1, y: 2, w: 3, h: 4 })))["clip"], json!([1, 2, 3, 4]));
        // (the scripts take their settings as one JSON value)
        assert!(MEASURE_JS.starts_with("(function (c)") && LINKS_JS.starts_with("(function (A)"));
    }
}
