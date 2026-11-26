<div align="center">

<img src="site/public/media/hero.jpg" alt="Tessera: the Badge, a wavy glass seal, over a soft flower" width="720">

# Tessera

**Glass. Or Material.** A desktop for Hyprland on Arch Linux.

[![Licence: GPL-3.0-or-later](https://img.shields.io/badge/licence-GPL--3.0--or--later-blue)](LICENSE)
[![Arch Linux](https://img.shields.io/badge/Arch_Linux-1793D1?logo=archlinux&logoColor=white)](https://archlinux.org)
[![Hyprland 0.56–0.57](https://img.shields.io/badge/Hyprland-0.56%E2%80%930.57-58E1FF)](compat.json)
[![Website](https://img.shields.io/badge/website-tessera.monishbl.in-4F46E5)](https://tessera.monishbl.in)
[![GitHub stars](https://img.shields.io/github/stars/monishbl/tessera?style=flat)](https://github.com/monishbl/tessera/stargazers)

[Website](https://tessera.monishbl.in) · [Install](#install) · [Features](docs/FEATURES.md) · [Docs](docs/README.md) · [Report a bug](https://tessera.monishbl.in/bugs/)

</div>

![The Tessera desktop over green leaves: the island at the top shows a pair of earbuds connected, with left, right and case batteries](site/public/media/shots/hero.webp)

## Why Tessera

A desktop for Hyprland in two looks: glass that bends what's behind it, or Material in your wallpaper's colours. An island in the menu bar, your Android phone and earbuds close at hand, and an assistant that's yours. On Arch Linux, next to the desktop you already have.

- **Complete.** Menu bar, dock, island, Quick Settings, Notification Center, Search, lock and login screens, Settings and Software, all built to match. Everything end4's illogical-impulse dots offer, and more.
- **Yours to keep.** Updates never overwrite a setting you've changed, and an update that breaks something can be undone.
- **Light.** The interface is written in Rust ([Slint](https://slint.dev), `native/tessera-ui`), with the original [Quickshell](https://quickshell.org) shell as an automatic fallback. Tuned for integrated GPUs; it idles at a fraction of a percent of CPU.
- **Safe to try.** One line runs it on its own console. Your `~/.config` and your usual session aren't touched.

## Highlights

### Two looks, one desktop

Glass, after macOS. Or Material, after Android: colours from your wallpaper, bold shapes and springy motion. Every feature works in both. Switch in Settings → Appearance → Look.

<p>
<img src="site/public/media/shots/looks-g.webp" alt="The Glass look: Quick Settings in clear glass that bends the pastel tiles behind it" width="49%">
<img src="site/public/media/shots/looks-m.webp" alt="The Material look: the same Quick Settings in tones taken from the tiles, with wide sliders and round tiles" width="49%">
</p>

### The island

A black island in the menu bar. It shows your spaces, keeps music, timers and recordings live, and springs open for earbuds connecting, drives, the charger, low battery and a CPU running hot.

<img src="site/public/media/shots/island-g.webp" alt="An island card: earbuds connected, with left, right and case batteries" width="480">

### Quick Share and Fast Pair

Send photos and files to and from Android phones, over Wi-Fi Direct at full speed without dropping your Wi-Fi, or over Bluetooth when there's no network at all. Open your earbuds' case and they're there, with their picture; one click connects them and shows each battery. LocalSend works too, for other computers and phones.

<img src="site/public/media/shots/share-g.webp" alt="A Quick Share card: receiving 2 photos from a phone, 46%" width="480">

### Point at it. Ask about it.

- **Search** answers sums, conversions and dates as you type: "18% of 2450", "days until christmas", "time in Tokyo", "remind me to call mum at 6".
- **Circle anything** on the screen and look it up. The picture goes to the web only when you ask.
- **Copy text from pictures**, read on this computer, nothing uploaded.
- **The AI sidebar** runs the model you choose: local with Ollama, or any provider you have a key for. Nothing is sent anywhere until you set one up, and chats stay on this computer.

<img src="site/public/media/shots/spotlight-flower.webp" alt="Search over a white flower: 18% of 2450 answered as 441 in a large card" width="640">

### Everything in its place

Alerts, your calendar, to-dos and focus in one sheet. Something that keeps changing stays one entry, with every update kept under it.

<img src="site/public/media/shots/nc-live.webp" alt="Notification Center: one entry for a low battery with 3 updates folded under it, earbuds connected, and the month's calendar" width="480">

### And everything you'd expect

Window tiling, snapping and Stages · All Windows and the App Drawer · clipboard history and an emoji picker · screenshots with markup and screen recording · Focus modes · desktop widgets and Widget Stacks · dynamic and live wallpapers · a matching lock and login screen with Bedside Mode · live captions and offline dictation · night light · per-app volume · Software for the Arch repositories and Flathub.

The full list, with keys and gestures: **[docs/FEATURES.md](docs/FEATURES.md)**.

## Install

**Requirements:** Arch Linux or an Arch-based system (CachyOS, EndeavourOS), Hyprland 0.56–0.57 (the installer sets it up), and a GPU with 3D acceleration (in a VM too).

**Try it** next to your desktop:

```sh
bash <(curl -fsSL https://tessera.monishbl.in/get)
```

It clones Tessera into `~/tessera` and asks before installing packages. Then switch to a free console (Ctrl+Alt+F3), log in, and run:

```sh
cd ~/tessera && ./demo.sh --check && ./dev.sh tty
```

Super+Shift+Escape (pressed twice) ends it. To remove it, delete `~/tessera`.

**Install it as your desktop:**

```sh
bash <(curl -fsSL https://tessera.monishbl.in/get) --install
```

The installer asks every question first, shows the whole plan (each package, file and service), and changes nothing until you start. What it touches:

- packages from the official repositories (plus optional AUR extras you tick);
- your Hyprland config, backed up once to `~/.config/hypr.pre-tessera`;
- the shell in `~/.config/quickshell/tessera`, settings in `~/.config/tessera`;
- optionally the Tessera login screen (greetd); another login screen is only replaced if you say so.

It takes a snapshot first when snapper or Timeshift is set up, and logs to `~/.cache/tessera/install.log`.

**Uninstall:** `tessera uninstall` puts your Hyprland config and login screen back. Packages stay installed and your settings are kept.

Mirrors, every installer option, virtual machines and the `tessera` command: **[docs/INSTALL.md](docs/INSTALL.md)**.

## Update and undo

Tessera checks for updates every few hours. Open **Settings → Software Update** to see what's new, or run `tessera update`.

- **Your changes win.** If an update touches a setting, shortcut or file you've changed, you decide before anything installs: keep yours, or take the new one. Settings and shortcuts are backed up first.
- **Undo.** Something broke after a system update? Go back to the exact package versions you had, from the package cache: Settings → Software Update, or `tessera undo-update` (it works from a text console too).
- **Held back when untested.** A Hyprland newer than the tested range ([compat.json](compat.json)) waits until Tessera supports it.
- **Channels.** Stable follows the `main` branch; Beta gets changes first.

<img src="site/public/media/shots/settings-update.webp" alt="Settings, Software Update: go back to before the last update, listing the packages it changed" width="640">

## The two looks

| | Glass | Material |
|---|---|---|
| After | macOS | Android (Material 3 Expressive) |
| Surfaces | Glass, Frost or Solid, or your own mix | Tones from your wallpaper |
| Tuning | Lens, frost, tint and light split, each on its own slider | Colours follow the wallpaper |
| Status | The default | Experimental: if it doesn't start, Tessera switches back to Glass and tells you |

Real glass over windows comes from **tessera-glass**, a Hyprland plugin the installer builds. Without it, Glass bends the wallpaper only and uses Frost over windows. Switching looks restarts the shell in a few seconds; your apps stay open. More in [docs/material-look.md](docs/material-look.md).

## Documentation

- [Features](docs/FEATURES.md): everything, area by area, with keys and gestures
- [Install](docs/INSTALL.md): requirements, mirrors, installer options, VMs, the `tessera` command
- [The Material look](docs/material-look.md)
- [Google Calendar and Tasks](docs/google-integration.md)
- [Battery saver](docs/battery-saver.md)
- [Changelog](CHANGELOG.md), and what's still open: [PENDING.md](PENDING.md)
- [All docs](docs/README.md)

## Contributing

Bug reports, ideas and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md): it covers the sandbox (`./dev.sh`), the tests and the commit style. Please follow the [code of conduct](CODE_OF_CONDUCT.md), and report security issues privately as described in [SECURITY.md](SECURITY.md).

Found a bug? Use **Report a bug** in Tessera's Settings, the [web form](https://tessera.monishbl.in/bugs/), or [open an issue](https://github.com/monishbl/tessera/issues/new/choose) with the output of `tessera doctor`.

## Licence

Tessera is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version (`GPL-3.0-or-later`). It comes with no warranty. The full text is in [LICENSE](LICENSE).

The fonts, icons, protocol definitions and data Tessera bundles from others keep their own licences; [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) lists each one, and the licence texts are in [LICENSES/](LICENSES).

## Credits

- [Hyprland](https://hyprland.org), [Quickshell](https://quickshell.org) and [Slint](https://slint.dev), which Tessera is built on.
- [end4's illogical-impulse](https://github.com/end-4/dots-hyprland): Tessera's default keys and gestures follow its layout ([ROADMAP.md](ROADMAP.md) compares the two).
- [RQuickShare](https://github.com/Martichou/rquickshare), credited for Quick Share interoperability.
- Phosphor icons, Google Sans Flex and Material Symbols (bundled as Tessera Sans Flex and Tessera Symbols), Inter. Photos from Unsplash.

**Trademarks.** macOS, AirPlay, iCloud and iPhone are trademarks of Apple Inc.; Android, Quick Share, Fast Pair, Chromecast and Material You are trademarks of Google LLC. They are named here only to say what Tessera works with or resembles. Tessera is not affiliated with or endorsed by Apple or Google.

**An earlier name.** Tessera had a working name until November 2026. If you used it then, your settings and folders move over by themselves; [docs/rename-to-tessera.md](docs/rename-to-tessera.md) lists what's left to run once.
