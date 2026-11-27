//! Search by picture: the picture goes to Google Lens from this window's own
//! WebKit session, so the results open here. Lens keeps an upload's
//! results with the session that sent it (shell/assets/scripts/lens-upload.py:
//! uploaded from elsewhere, the results page says the image has expired), so
//! the upload is this window's: a page loaded from memory that sends the
//! picture as a form, the way lens.google.com's own page does (the field
//! `encoded_image`, multipart), and Lens answers with its results here.
//! The consent ("The circled picture will be sent to Google") is asked by
//! the shell before it starts this.

use std::path::Path;

pub const UPLOAD: &str = "https://lens.google.com/v3/upload";
/// The largest picture sent (as lens-upload.py)
pub const LIMIT: u64 = 20 * 1024 * 1024;

/// The picture's bytes, or what to say instead
pub fn read(path: &Path) -> Result<Vec<u8>, &'static str> {
    let len = std::fs::metadata(path).map_err(|_| "The picture is gone. Circle it again to search it.")?.len();
    if len > LIMIT {
        return Err("The picture is too big for Lens. Circle a smaller area.");
    }
    let data = std::fs::read(path).map_err(|_| "The picture is gone. Circle it again to search it.")?;
    if data.is_empty() {
        return Err("The picture is empty. Circle it again to search it.");
    }
    Ok(data)
}

/// The picture's type, from its first bytes (the extension as a fallback)
pub fn mime(data: &[u8], path: &Path) -> &'static str {
    if data.starts_with(b"\x89PNG") {
        "image/png"
    } else if data.starts_with(b"\xFF\xD8") {
        "image/jpeg"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.len() > 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("png") => "image/png",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        }
    }
}

fn ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        _ => ".jpg",
    }
}

/// Where the form goes: Lens's upload, as its own page sends it
/// (ep=ccm, s empty, st the time in ms)
pub fn action(now_ms: u128) -> String {
    format!("{UPLOAD}?ep=ccm&s=&st={now_ms}")
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// &, <, >, " and ' as HTML entities
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// The page that sends the picture: it builds the file from the bytes in
/// it and submits the form at once. `dark`: its few words in the look's
/// colours while Lens answers.
pub fn page(data: &[u8], mime: &str, now_ms: u128, dark: bool) -> String {
    let (bg, fg) = if dark { ("#111113", "#D4D4D8") } else { ("#FAFAFA", "#27272A") };
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="referrer" content="no-referrer">
<meta name="viewport" content="width=device-width">
<title>Search by picture</title>
<style>
body {{ margin: 0; height: 100vh; display: grid; place-items: center; font: 15px/1.4 system-ui, sans-serif; background: {bg}; color: {fg}; }}
p {{ margin: 0; }}
</style></head>
<body><p id="m">Sending the picture to Google Lens…</p>
<form id="f" method="post" enctype="multipart/form-data" action="{action}">
<input type="file" name="encoded_image" id="i" hidden></form>
<script>
try {{
  const s = atob("{b64}");
  const a = new Uint8Array(s.length);
  for (let k = 0; k < s.length; k++) a[k] = s.charCodeAt(k);
  const t = new DataTransfer();
  t.items.add(new File([a], "picture{ext}", {{ type: "{mime}" }}));
  document.getElementById("i").files = t.files;
  document.getElementById("f").submit();
}} catch (e) {{
  document.getElementById("m").textContent = "The picture couldn't be sent. Open lens.google.com and paste it with Ctrl+V.";
}}
</script>
</body></html>"#,
        action = escape(&action(now_ms)),
        b64 = base64(data),
        ext = ext(mime),
        mime = mime,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_as_atob_reads_it() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn the_form_is_lenss_own() {
        let png = b"\x89PNG\r\n\x1a\nrest";
        let html = page(png, mime(png, Path::new("x.bin")), 1234, true);
        assert!(html.contains(r#"action="https://lens.google.com/v3/upload?ep=ccm&amp;s=&amp;st=1234""#));
        assert!(html.contains(r#"method="post" enctype="multipart/form-data""#));
        assert!(html.contains(r#"name="encoded_image""#));
        assert!(html.contains(&format!("atob(\"{}\")", base64(png))));
        assert!(html.contains(r#"new File([a], "picture.png", { type: "image/png" })"#));
        assert!(html.contains(r#"<meta name="referrer" content="no-referrer">"#));
        assert!(html.contains("#111113"));
        assert!(page(png, "image/png", 1, false).contains("#FAFAFA"));
    }

    #[test]
    fn the_pictures_type() {
        assert_eq!(mime(b"\xFF\xD8\xFFrest", Path::new("a.png")), "image/jpeg");
        assert_eq!(mime(b"GIF89a", Path::new("a")), "image/gif");
        assert_eq!(mime(b"RIFF\0\0\0\0WEBPVP8 ", Path::new("a")), "image/webp");
        assert_eq!(mime(b"????", Path::new("shot.PNG")), "image/png");
        assert_eq!(mime(b"????", Path::new("shot")), "image/jpeg");
        assert_eq!(escape(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }

    #[test]
    fn a_missing_or_huge_picture_says_so() {
        assert!(read(Path::new("/nonexistent/picture.png")).is_err());
        let dir = std::env::temp_dir().join(format!("tessera-web-lens-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.png");
        std::fs::write(&p, b"\x89PNG").unwrap();
        assert_eq!(read(&p).unwrap(), b"\x89PNG");
        std::fs::write(&p, b"").unwrap();
        assert!(read(&p).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
