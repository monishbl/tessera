// Reading mode: the page's article alone, in a calm column (a small
// readability-style pass: the block with the most paragraph text, cleaned of
// scripts, navigation, forms and ads). Run in its own world by tessera-web
// (src/main.rs), with TESSERA_READER = { css } before it. Gives back
// "ok" when it found an article, "none" when it didn't (the page stays as it
// was); reloading the page leaves reading mode.
(function () {
  const cfg = typeof TESSERA_READER === "object" ? TESSERA_READER : { css: "" };
  const doc = document;
  if (!doc.body) return "none";
  const DROP = "script, style, noscript, iframe, object, embed, form, nav, aside, footer, header, button, input, select, textarea, svg, canvas, [role=navigation], [role=banner], [role=complementary], [aria-hidden=true], .ad, .ads, .advert, .advertisement, .share, .social, .related, .newsletter, .comments, #comments";
  const textLen = (el) => (el.textContent || "").replace(/\s+/g, " ").trim().length;
  const linkLen = (el) => [...el.querySelectorAll("a")].reduce((n, a) => n + textLen(a), 0);
  // Candidates: the article/main elements, then every block holding paragraphs
  let best = null, bestScore = 0;
  const consider = (el, bonus) => {
    const ps = el.querySelectorAll("p");
    if (ps.length < 2 && textLen(el) < 400) return;
    let text = 0;
    ps.forEach((p) => { const n = textLen(p); if (n > 40) text += n; });
    const links = linkLen(el), all = Math.max(textLen(el), 1);
    const score = (text + bonus) * (1 - Math.min(links / all, 0.9));
    if (score > bestScore) { best = el; bestScore = score; }
  };
  doc.querySelectorAll("article, main, [role=main], [itemprop=articleBody]").forEach((el) => consider(el, 400));
  doc.querySelectorAll("div, section, td").forEach((el) => consider(el, 0));
  if (!best || bestScore < 300) return "none";
  const article = best.cloneNode(true);
  article.querySelectorAll(DROP).forEach((el) => el.remove());
  // (pictures keep their real source when the page loads them lazily)
  article.querySelectorAll("img").forEach((img) => {
    const lazy = img.getAttribute("data-src") || img.getAttribute("data-lazy-src") || img.getAttribute("data-original");
    if (lazy) img.setAttribute("src", lazy);
    img.removeAttribute("srcset");
    img.removeAttribute("width");
    img.removeAttribute("height");
    img.removeAttribute("style");
  });
  article.querySelectorAll("*").forEach((el) => { el.removeAttribute("style"); el.removeAttribute("class"); el.removeAttribute("id"); });
  const h1 = doc.querySelector("h1");
  const title = ((h1 && textLen(h1) > 0 ? h1.textContent : doc.title) || "").trim();
  const host = location.hostname.replace(/^www\./, "");
  const html = doc.documentElement;
  [...html.attributes].forEach((a) => html.removeAttribute(a.name));
  const head = doc.createElement("head");
  const meta = doc.createElement("meta");
  meta.setAttribute("name", "viewport");
  meta.setAttribute("content", "width=device-width");
  const style = doc.createElement("style");
  style.textContent = cfg.css;
  const t = doc.createElement("title");
  t.textContent = doc.title;
  head.append(meta, t, style);
  const body = doc.createElement("body");
  const main = doc.createElement("main");
  const site = doc.createElement("p");
  site.className = "tessera-site";
  site.textContent = host;
  const heading = doc.createElement("h1");
  heading.textContent = title;
  // (the article's own first heading, when it's the title, isn't shown twice)
  const first = article.querySelector("h1");
  if (first && first.textContent.trim() === title) first.remove();
  main.append(site, heading, article);
  body.append(main);
  html.replaceChildren(head, body);
  window.scrollTo(0, 0);
  return "ok";
})();
