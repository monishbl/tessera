//! The command line: the contract with the shell (~/tessera-verify/web/SPEC.md,
//! "Command line"). Every option is written as there; `--opt=value` works too.
//!
//!   tessera-web [OPTIONS] [URL|FILE]
//!     --from X,Y,W,H         grow the window from this rectangle (screen logical px)
//!     --at X,Y,W,H           open at exactly this rectangle
//!     --lens PICTURE         search this picture with Google Lens
//!     --portal               captive-portal mode; closes by itself once online
//!     --signin URL --done PREFIX   sign-in mode: prints the final URL when navigation reaches PREFIX
//!     --quicklook            Quick Look mode: read-only preview, Space/Esc closes
//!     --title TEXT           a title for the bar when there's no page title yet
//!     --reader               start in reading mode (the reading list's links)
//!
//!   tessera-web --render URL --out DIR [--width W] [--height H]
//!               [--clip X,Y,W,H | --selector CSS] [--every SECONDS] [--scale S]
//!     renders the page out of sight (src/render.rs): DIR/shot.png, DIR/links.json

use std::path::{Path, PathBuf};

/// A rectangle on the screen, logical px
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn centre(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// a page, the address field editable
    Browse,
    /// a picture searched with Google Lens (its path)
    Lens(PathBuf),
    /// a Wi-Fi network's sign-in page
    Portal,
    /// an account's sign-in: done when navigation reaches `done`
    SignIn { url: String, done: String },
    /// a read-only preview of a link or an HTML file
    QuickLook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// where the window rules put it (centred, about 66% × 70%)
    Rules,
    /// grows from this rectangle to where the rules would put it
    From(Rect),
    /// exactly here (the shell animated to it already)
    At(Rect),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub mode: Mode,
    pub place: Place,
    pub title: Option<String>,
    /// URL|FILE as given (None: a blank page, the address field ready)
    pub target: Option<String>,
    /// --reader: reading mode once the page has loaded (Browse and Quick Look)
    pub reader: bool,
}

/// The part of a rendered page the picture shows
#[derive(Clone, Debug, PartialEq)]
pub enum Area {
    /// the page's top, the width of the window and --height tall
    Top,
    /// this rectangle of the page (CSS px from its top-left)
    Clip(Rect),
    /// this element's box (its top at the picture's top), at most --height tall
    Selector(String),
}

/// --render: a page drawn out of sight, for a picture of it (Search's web
/// results, a desktop web clip)
#[derive(Clone, Debug, PartialEq)]
pub struct Render {
    pub url: String,
    pub out: PathBuf,
    /// the page's width and the picture's most height, CSS px
    pub width: i32,
    pub height: i32,
    pub area: Area,
    /// stay and draw it again this often (seconds); None: once, then exit
    pub every: Option<u64>,
    /// pixels per CSS px in the picture (2 for a sharp one on a 2× screen)
    pub scale: f64,
}

pub const RENDER_DEFAULT_W: i32 = 1024;
pub const RENDER_DEFAULT_H: i32 = 768;

#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    Run(Args),
    Render(Render),
    Help,
    Version,
}

pub const USAGE: &str = "\
tessera-web: Tessera's quick-look browser

usage: tessera-web [OPTIONS] [URL|FILE]
  --from X,Y,W,H         grow the window from this rectangle (screen logical px)
  --at X,Y,W,H           open at exactly this rectangle
  --lens PICTURE         search this picture with Google Lens
  --portal               open the network's sign-in page; close by itself once online
  --signin URL --done PREFIX
                         sign in: no address editing; when navigation reaches PREFIX,
                         print {\"event\":\"done\",\"url\":…} and close
  --quicklook            read-only preview: Space or Esc closes
  --title TEXT           a title for the bar until the page has one
  --reader               start in reading mode (a link from the reading list)
  --version              which build this is

usage: tessera-web --render URL --out DIR [--width W] [--height H]
                   [--clip X,Y,W,H | --selector CSS] [--every SECONDS] [--scale S]
  renders the page out of sight (never on screen) in this program's own
  session, writes DIR/shot.png and DIR/links.json ([{x,y,w,h,href,title}]
  in the picture's pixels; a search result's also has result, url, snippet)
  and prints {\"event\":\"rendered\",\"png\":…,\"links\":…}; with --every it
  stays and renders again every SECONDS. W×H: the page's width and the
  picture's most height, CSS px (1024×768); --scale 2 for a sharp picture
  on a 2× screen.

Esc closes the window. --signin and --portal print one JSON line on stdout:
{\"event\":\"done\",\"url\":…} (signed in, or online) or {\"event\":\"closed\"}.
Exit status: 0 shown and closed; 1 it couldn't start (no display, no WebKitGTK);
2 a mistake on the command line.";

/// "X,Y,W,H" (numbers may have decimals: rounded); W and H above zero
pub fn parse_rect(s: &str) -> Result<Rect, String> {
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!("{s:?} isn't X,Y,W,H"));
    }
    let mut n = [0i32; 4];
    for (i, p) in parts.iter().enumerate() {
        let v: f64 = p.parse().map_err(|_| format!("{s:?} isn't X,Y,W,H"))?;
        if !v.is_finite() || v.abs() > 1e6 {
            return Err(format!("{s:?} isn't X,Y,W,H"));
        }
        n[i] = v.round() as i32;
    }
    if n[2] <= 0 || n[3] <= 0 {
        return Err(format!("{s:?}: the width and height must be above zero"));
    }
    Ok(Rect { x: n[0], y: n[1], w: n[2], h: n[3] })
}

/// The arguments after the program's name
pub fn parse(argv: &[String]) -> Result<Parsed, String> {
    let mut place = Place::Rules;
    let mut lens: Option<PathBuf> = None;
    let mut portal = false;
    let mut signin: Option<String> = None;
    let mut done: Option<String> = None;
    let mut quicklook = false;
    let mut title: Option<String> = None;
    let mut target: Option<String> = None;
    let mut render: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let (mut width, mut height): (Option<i32>, Option<i32>) = (None, None);
    let mut clip: Option<Rect> = None;
    let mut selector: Option<String> = None;
    let mut every: Option<u64> = None;
    let mut scale: Option<f64> = None;
    let mut reader = false;
    let mut i = 0;
    let mut only_positional = false;
    while i < argv.len() {
        let a = argv[i].as_str();
        i += 1;
        if only_positional || !a.starts_with("--") || a == "--" {
            if a == "--" && !only_positional {
                only_positional = true;
                continue;
            }
            if target.is_some() {
                return Err(format!("one URL or file at a time ({a:?} too)"));
            }
            target = Some(a.to_string());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a, None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            match argv.get(i) {
                Some(v) => {
                    i += 1;
                    Ok(v.clone())
                }
                None => Err(format!("{name} needs {what}")),
            }
        };
        match name {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--version" => return Ok(Parsed::Version),
            "--from" | "--at" => {
                let r = parse_rect(&value("X,Y,W,H")?).map_err(|e| format!("{name}: {e}"))?;
                if place != Place::Rules {
                    return Err("--from and --at: one of them".into());
                }
                place = if name == "--from" { Place::From(r) } else { Place::At(r) };
            }
            "--lens" => lens = Some(PathBuf::from(value("a picture")?)),
            "--portal" => portal = true,
            "--signin" => signin = Some(value("a URL")?),
            "--done" => done = Some(value("a URL prefix")?),
            "--quicklook" => quicklook = true,
            "--title" => title = Some(value("a title")?),
            "--render" => render = Some(value("a URL")?),
            "--out" => out = Some(PathBuf::from(value("a folder")?)),
            "--width" | "--height" => {
                let v = value("a number")?;
                let n: i32 = v.trim().parse().ok().filter(|n| (16..=8192).contains(n)).ok_or_else(|| format!("{name}: {v:?} isn't a size from 16 to 8192"))?;
                if name == "--width" {
                    width = Some(n);
                } else {
                    height = Some(n);
                }
            }
            "--clip" => clip = Some(parse_rect(&value("X,Y,W,H")?).map_err(|e| format!("--clip: {e}"))?),
            "--selector" => selector = Some(value("a CSS selector")?).filter(|s| !s.trim().is_empty()),
            "--every" => {
                let v = value("seconds")?;
                every = Some(v.trim().parse::<u64>().ok().filter(|n| *n >= 5).ok_or_else(|| format!("--every: {v:?} isn't a number of seconds, 5 or more"))?);
            }
            "--scale" => {
                let v = value("a number")?;
                scale = Some(v.trim().parse::<f64>().ok().filter(|n| n.is_finite() && (0.5..=4.0).contains(n)).ok_or_else(|| format!("--scale: {v:?} isn't from 0.5 to 4"))?);
            }
            "--reader" => reader = true,
            _ => return Err(format!("unknown option {name}")),
        }
        if inline.is_some() && matches!(name, "--portal" | "--quicklook" | "--reader") {
            return Err(format!("{name} takes no value"));
        }
    }
    let render_opts = out.is_some() || width.is_some() || height.is_some() || clip.is_some() || selector.is_some() || every.is_some() || scale.is_some();
    if let Some(url) = render {
        if lens.is_some() || portal || signin.is_some() || done.is_some() || quicklook || reader || place != Place::Rules || title.is_some() || target.is_some() {
            return Err("--render takes only its own options".into());
        }
        if !crate::nav::is_http(&url) {
            return Err(format!("--render: {url:?} isn't an http or https address"));
        }
        let out = out.ok_or("--render needs --out DIR")?;
        if clip.is_some() && selector.is_some() {
            return Err("--clip and --selector: one of them".into());
        }
        let area = match (clip, selector) {
            (Some(r), _) => Area::Clip(r),
            (_, Some(s)) => Area::Selector(s),
            _ => Area::Top,
        };
        return Ok(Parsed::Render(Render { url: url.trim().to_string(), out, width: width.unwrap_or(RENDER_DEFAULT_W), height: height.unwrap_or(RENDER_DEFAULT_H), area, every, scale: scale.unwrap_or(1.0) }));
    }
    if render_opts {
        return Err("--out, --width, --height, --clip, --selector, --every and --scale go with --render".into());
    }
    let modes = [lens.is_some(), portal, signin.is_some(), quicklook].iter().filter(|x| **x).count();
    if modes > 1 {
        return Err("--lens, --portal, --signin and --quicklook: one of them".into());
    }
    if done.is_some() != signin.is_some() {
        return Err("--signin URL and --done PREFIX go together".into());
    }
    let mode = if let Some(p) = lens {
        if target.is_some() {
            return Err("--lens searches its picture: no URL with it".into());
        }
        Mode::Lens(p)
    } else if let (Some(url), Some(done)) = (signin, done) {
        if target.is_some() {
            return Err("--signin has its URL: no other with it".into());
        }
        if url.trim().is_empty() || done.trim().is_empty() {
            return Err("--signin and --done need a URL each".into());
        }
        Mode::SignIn { url, done }
    } else if portal {
        Mode::Portal
    } else if quicklook {
        if target.is_none() {
            return Err("--quicklook needs a URL or a file".into());
        }
        Mode::QuickLook
    } else {
        Mode::Browse
    };
    if reader && !matches!(mode, Mode::Browse | Mode::QuickLook) {
        return Err("--reader is for a page: not with --lens, --portal or --signin".into());
    }
    if reader && target.is_none() {
        return Err("--reader needs a URL or a file".into());
    }
    let title = title.filter(|t| !t.trim().is_empty());
    Ok(Parsed::Run(Args { mode, place, title, target, reader }))
}

/// What the window loads first for a URL|FILE argument: a file that's there
/// as a file:// URI (relative to `cwd`), anything else as typed into the
/// address field (crate::nav::typed). None for nothing to load.
pub fn start_uri(target: &str, cwd: &Path, is_file: impl Fn(&Path) -> bool) -> Option<String> {
    let t = target.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix("file://") {
        return Some(format!("file://{}", crate::nav::encode_path(&crate::nav::decode(rest))));
    }
    if !crate::nav::has_scheme(t) {
        let p = if Path::new(t).is_absolute() { PathBuf::from(t) } else { cwd.join(t) };
        if is_file(&p) {
            let p = std::fs::canonicalize(&p).unwrap_or(p);
            return Some(format!("file://{}", crate::nav::encode_path(&p.to_string_lossy())));
        }
    }
    crate::nav::typed(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Parsed, String> {
        parse(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    fn run(a: &[&str]) -> Args {
        match p(a) {
            Ok(Parsed::Run(x)) => x,
            other => panic!("{a:?}: {other:?}"),
        }
    }

    #[test]
    fn a_url_alone_browses() {
        let a = run(&["https://example.com"]);
        assert_eq!(a.mode, Mode::Browse);
        assert_eq!(a.place, Place::Rules);
        assert_eq!(a.target.as_deref(), Some("https://example.com"));
        assert_eq!(run(&[]).target, None);
    }

    #[test]
    fn rectangles() {
        assert_eq!(parse_rect("10,20,300,400"), Ok(Rect { x: 10, y: 20, w: 300, h: 400 }));
        assert_eq!(parse_rect(" 10.4, -20.6 ,300.5,400"), Ok(Rect { x: 10, y: -21, w: 301, h: 400 }));
        assert!(parse_rect("10,20,300").is_err());
        assert!(parse_rect("10,20,0,400").is_err());
        assert!(parse_rect("a,b,c,d").is_err());
        assert!(parse_rect("1,2,3,NaN").is_err());
        let a = run(&["--from", "1,2,3,4", "https://a.b"]);
        assert_eq!(a.place, Place::From(Rect { x: 1, y: 2, w: 3, h: 4 }));
        let a = run(&["--at=5,6,7,8"]);
        assert_eq!(a.place, Place::At(Rect { x: 5, y: 6, w: 7, h: 8 }));
        assert!(p(&["--from", "1,2,3,4", "--at", "1,2,3,4"]).is_err());
        assert!(p(&["--from"]).is_err());
    }

    #[test]
    fn every_mode_as_the_spec_writes_it() {
        assert_eq!(run(&["--lens", "/tmp/x.png", "--from", "1,2,3,4"]).mode, Mode::Lens("/tmp/x.png".into()));
        assert_eq!(run(&["--portal"]).mode, Mode::Portal);
        assert_eq!(run(&["--portal", "http://portal.example/login"]).target.as_deref(), Some("http://portal.example/login"));
        assert_eq!(
            run(&["--signin", "https://accounts.example/auth", "--done", "http://127.0.0.1:8123/cb"]).mode,
            Mode::SignIn { url: "https://accounts.example/auth".into(), done: "http://127.0.0.1:8123/cb".into() }
        );
        let a = run(&["--quicklook", "--title", "Report", "page.html"]);
        assert_eq!(a.mode, Mode::QuickLook);
        assert_eq!(a.title.as_deref(), Some("Report"));
        assert_eq!(run(&["--title", "  ", "x"]).title, None);
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(p(&["--signin", "https://a"]).is_err());
        assert!(p(&["--done", "https://a"]).is_err());
        assert!(p(&["--lens", "a.png", "--portal"]).is_err());
        assert!(p(&["--lens", "a.png", "https://x"]).is_err());
        assert!(p(&["--quicklook"]).is_err());
        assert!(p(&["--portal=yes"]).is_err());
        assert!(p(&["--nonsense"]).is_err());
        assert!(p(&["a", "b"]).is_err());
        assert_eq!(p(&["--help"]), Ok(Parsed::Help));
        assert_eq!(p(&["--version"]), Ok(Parsed::Version));
        // (after --, a name starting with -- is a file)
        assert_eq!(run(&["--", "--odd.html"]).target.as_deref(), Some("--odd.html"));
    }

    #[test]
    fn render_as_the_spec_writes_it() {
        let r = |a: &[&str]| match p(a) {
            Ok(Parsed::Render(r)) => r,
            other => panic!("{a:?}: {other:?}"),
        };
        let x = r(&["--render", "https://www.google.com/search?q=x", "--out", "/tmp/o"]);
        assert_eq!((x.width, x.height, x.area.clone(), x.every, x.scale), (1024, 768, Area::Top, None, 1.0));
        assert_eq!(x.out, PathBuf::from("/tmp/o"));
        let x = r(&["--render", "https://a.b", "--out", "/tmp/o", "--width", "640", "--height", "480", "--selector", "#rso", "--every", "300", "--scale", "2"]);
        assert_eq!((x.width, x.height, x.area, x.every, x.scale), (640, 480, Area::Selector("#rso".into()), Some(300), 2.0));
        let x = r(&["--render=https://a.b", "--out=/tmp/o", "--clip", "0,100,600,300"]);
        assert_eq!(x.area, Area::Clip(Rect { x: 0, y: 100, w: 600, h: 300 }));
        // (mistakes)
        assert!(p(&["--render", "https://a.b"]).is_err());
        assert!(p(&["--render", "file:///etc/passwd", "--out", "/tmp/o"]).is_err());
        assert!(p(&["--render", "javascript:1", "--out", "/tmp/o"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--clip", "0,0,1,1", "--selector", "x"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--every", "1"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--width", "0"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--scale", "9"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--portal"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "https://c.d"]).is_err());
        assert!(p(&["--out", "/tmp/o", "https://c.d"]).is_err());
        assert!(p(&["--render", "https://a.b", "--out", "/tmp/o", "--reader"]).is_err());
    }

    #[test]
    fn reading_mode_from_the_start() {
        let a = run(&["--reader", "https://example.com/story"]);
        assert!(a.reader && a.mode == Mode::Browse);
        assert!(!run(&["https://example.com"]).reader);
        assert!(run(&["--quicklook", "--reader", "a.html"]).reader);
        assert!(p(&["--reader"]).is_err());
        assert!(p(&["--reader=yes", "https://a"]).is_err());
        assert!(p(&["--reader", "--portal", "https://a"]).is_err());
    }

    #[test]
    fn files_and_typed_addresses() {
        let cwd = Path::new("/home/me");
        let yes = |_: &Path| true;
        let no = |_: &Path| false;
        assert_eq!(start_uri("notes.html", cwd, yes).as_deref(), Some("file:///home/me/notes.html"));
        assert_eq!(start_uri("/tmp/a b.html", cwd, yes).as_deref(), Some("file:///tmp/a%20b.html"));
        assert_eq!(start_uri("example.com", cwd, no).as_deref(), Some("https://example.com"));
        assert_eq!(start_uri("https://example.com/x", cwd, yes).as_deref(), Some("https://example.com/x"));
        assert_eq!(start_uri("file:///tmp/a%20b.html", cwd, no).as_deref(), Some("file:///tmp/a%20b.html"));
        assert_eq!(start_uri("  ", cwd, yes), None);
    }
}
