# Launch checklist

Outward steps that only the maintainer can do, each with a text ready to paste. Tick them off as you go. The repo's description, homepage and topics are already set.

Before posting anywhere: make sure the site is deployed with the new SEO tags (`site/public/robots.txt`, `sitemap.xml`, the tags in `index.html`) and the README renders well on GitHub.

## 1. Pin the repo and turn on private reports

- [ ] On your GitHub profile, **Customize your pins** → tick `tessera`.
- [ ] Repo **Settings → Security → Private vulnerability reporting → Enable** (SECURITY.md and the issue form point there).
- [ ] Repo **Settings → General → Features**: Issues on; Discussions on if you want questions kept out of Issues.

## 2. Upload the social preview

- [ ] Repo **Settings → General → Social preview → Edit → Upload an image** → `site/public/media/social-preview.png` (1280×640, under 1 MB).

This is the card that shows when the repo link is shared on Reddit, X, Discord, Slack and Hacker News. The website already uses the same picture (`og:image`).

## 3. r/unixporn

- [ ] Post a video or a gallery: [site/public/media/hero.mp4](../site/public/media/hero.mp4), or the shots in `site/public/media/shots/` (the Glass and Material pair works best). Mark it OC.
- [ ] Add the details comment straight away: the subreddit requires one.

**Title**

```
[Hyprland] Tessera: glass that bends what's behind it, or Material in your wallpaper's colours
```

**Details comment**

```
Details:
- OS: Arch Linux
- WM: Hyprland 0.56
- Shell: Tessera, written from scratch (interface in Rust/Slint, a Quickshell fallback); the glass is a Hyprland plugin that refracts the windows behind it
- Font: Inter
- Icons: each app's own logo, restyled to match the look
- Cursor: Bibata
- Wallpaper: Unsplash

What's in it: two looks (Glass, or Material in your wallpaper's colours), an island in the menu bar, Quick Share and Fast Pair with Android, circle-to-search and copy text from pictures, an AI sidebar with the model you choose, and updates you can undo from the package cache.

Try it next to your current desktop (runs on its own console, doesn't touch ~/.config):
bash <(curl -fsSL https://tessera.monishbl.in/get)

Source (GPL-3.0): https://github.com/monishbl/tessera
Site: https://tessera.monishbl.in
```

## 4. r/hyprland

- [ ] Post with the hero video or the Glass/Material pair. Flair: Showcase (or whatever showcase flair the sub has at the time).

**Title**

```
Tessera: a full desktop shell for Hyprland, in Glass or Material
```

**Body**

```
I've been building Tessera, a complete desktop for Hyprland: menu bar with an island, dock, Quick Settings, Notification Center, Search, lock and login screens, Settings and a software centre, in two looks.

A few things that might interest this sub:
- The glass is a Hyprland plugin (tessera-glass) that refracts the windows behind it, drawn by Hyprland only where it repaints anyway, so static glass costs nothing.
- Drag a window to an edge or corner for a glass preview of where it'll tile; Stages and window tabs too.
- Config is Lua, with ~/.config/hypr/custom.lua loaded last for your own changes.
- Software Update holds back a Hyprland version it hasn't been tested with (0.56–0.57 today) and can undo an update from the package cache.

You can try it on a spare console without touching your current setup:
bash <(curl -fsSL https://tessera.monishbl.in/get)

Source (GPL-3.0): https://github.com/monishbl/tessera
Feedback and bug reports very welcome.
```

## 5. awesome-hyprland

- [ ] Fork [hyprland-community/awesome-hyprland](https://github.com/hyprland-community/awesome-hyprland), add the line under the section for shells or complete desktops (check the current section names and their order, usually alphabetical), and open a pull request.

**The line**

```
- [Tessera](https://github.com/monishbl/tessera) - A complete desktop shell for Hyprland in two looks, Glass and Material, with an island in the menu bar, Quick Share and an AI sidebar.
```

**Pull request title**

```
Add Tessera
```

**Pull request body**

```
Adds Tessera, a GPL-3.0 desktop shell for Hyprland (menu bar, island, dock, Quick Settings, lock and login screens) in two looks, Glass and Material. Site: https://tessera.monishbl.in
```

## 6. Show HN

- [ ] Post on a weekday morning (US Eastern), and stay around for the first couple of hours to answer.

**Title** (80 characters at most)

```
Show HN: Tessera – A desktop shell for Hyprland in two looks, Glass or Material
```

**URL**

```
https://github.com/monishbl/tessera
```

**Text** (first comment)

```
Tessera is a complete desktop for Hyprland on Arch Linux: menu bar, island, dock, Quick Settings, notifications, Search, lock and login screens, Settings and a software centre. It comes in two looks: glass that refracts what's behind it, or Material in your wallpaper's colours.

Some parts that were interesting to build:
- The glass is a Hyprland plugin that draws refraction inside the compositor, only where it repaints anyway, so glass that isn't moving costs nothing.
- The interface started in Quickshell (QML) and is now written in Rust with Slint; the QML shell stays as an automatic fallback if the Rust one can't start.
- Quick Share with Android phones is implemented from the protocol, over Wi-Fi or Bluetooth LE with a Wi-Fi Direct upgrade for speed.
- Software Update can go back to the exact package versions you had before an update, from pacman's cache, and holds back Hyprland versions it hasn't been tested with.
- The AI sidebar runs whichever model you configure (local or any provider); nothing is sent until you set one up.

You can try it on a spare console without touching your current setup: bash <(curl -fsSL https://tessera.monishbl.in/get)

It's GPL-3.0. I'd love feedback, especially from people on hardware I haven't tested.
```

## 7. Publish the AUR package

The draft is in [package/aur/tessera-git/](../package/aur/tessera-git/). It installs the `tessera-setup` command (setup.sh), which downloads Tessera and runs its installer, because Tessera installs per user and updates itself.

- [ ] Add your SSH public key in your [AUR account](https://aur.archlinux.org) (My Account → SSH Public Key).
- [ ] Build and check it in a clean folder:

```sh
cp -r package/aur/tessera-git /tmp/tessera-git && cd /tmp/tessera-git
makepkg -si                       # builds, installs tessera-setup
namcap PKGBUILD *.pkg.tar.zst     # lint
makepkg --printsrcinfo > .SRCINFO # refresh pkgver in .SRCINFO
```

- [ ] Publish:

```sh
git clone ssh://aur@aur.archlinux.org/tessera-git.git aur-tessera-git
cp /tmp/tessera-git/{PKGBUILD,.SRCINFO,tessera-git.install} aur-tessera-git/
cd aur-tessera-git
git add PKGBUILD .SRCINFO tessera-git.install
git commit -m "Initial import: tessera-git"
git push
```

- [ ] Add the AUR line to the README's Install section once it's live: `yay -S tessera-git && tessera-setup`.

## 8. Afterwards

- [ ] Answer questions in the threads for the first day; turn good ones into FAQ entries on the site.
- [ ] Label a few small issues `good first issue` so new contributors have somewhere to start.
