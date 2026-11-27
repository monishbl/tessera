# Tessera: the complete overview

Tessera (formerly **iRice**) is a desktop shell for **Hyprland** on Arch Linux. It looks like macOS, with a second, Android-like **Material** look. It started on 26 October 2026 as a Quickshell (QML) rice. It is now a **native Rust shell** (`tessera-ui`, Slint + FemtoVG on wlr-layer-shell), with the QML shell kept frozen as a crash fallback.

- Repository: `monishbl/tessera` on GitHub, the one source Software Update fetches code from. A GitLab mirror (`gitlab.com/monishbl/tessera`) is listed second in `tools/sources.txt`, for a fresh install's clone and for ready-made builds, and is passed over quietly while it has nothing; it is only filled once its one-time setup is done (docs/releases.md, "Mirrors" and "One-time setup: the GitLab mirror"). Site: tessera.monishbl.in
- Licence: GPL-3.0-or-later. Bundled fonts and icons keep their own licences (THIRD_PARTY_NOTICES.md).
- Channels: `main` is Stable, `beta` is Beta. What is done, pending or still to be checked is not repeated here: see [native/tessera-ui/RUST-ONLY.md](../native/tessera-ui/RUST-ONLY.md) (every Rust-only change and its status), [PENDING.md](../PENDING.md) (what waits on something else) and [ROADMAP.md](../ROADMAP.md) (the QML shell's history).

---

## 1. What the codebase is made of

| Language | What it is |
|---|---|
| Rust | `native/tessera-ui` (the shell), `native/tessera-events` (event supervisor) |
| Slint | the Rust shell's UI markup (`core/ui`, each surface's `.slint`) |
| QML | the original shell (`shell/`, `material/`), frozen as a fallback |
| Python | system helpers, installer, updater, Quick Share, root helper, tests |
| Shell | install, dev sandbox, session and greeter scripts |
| C++ | the `tessera-glass` Hyprland plugin |
| GLSL | glass, lens and frost shaders |
| Lua | Hyprland config (Hyprland 0.56+ Lua config) |

Tests: Rust unit tests in each crate (`cargo test`), Python tests in `tests/test_*.py` (`tools/run-python-tests.sh`, which also runs the Node tests, `tests/*.test.cjs`: polkit rules and the website). The release builder runs all of them for a commit before anything built from it is offered (docs/releases.md, "The tests pass first").

---

## 2. Tech stack

### Compositor and session
- **Hyprland** (0.56/0.57, Lua config) started with `start-hyprland`. The config lives in `hypr/tessera/*.lua`: execs, keybinds, gestures, rules, findpointer and others.
- **greetd** with Tessera's own login screen: the Rust `tessera-ui --greeter` running inside a minimal Hyprland (`greeter.lua`), started by `greeter-session.sh`.
- **hypridle** for idle handling (it honours systemd idle inhibitors). The lock screen is Tessera's own.
- **PAM** stacks:
  - `pam_fprintd` for the fingerprint;
  - Howdy for face unlock;
  - `pam_gnome_keyring` so the login password unlocks the keyring;
  - polkit rules in `49-tessera.rules`.
- **systemd user units**: the release builder and publisher, battery guard, login check and fallback.

### The Rust shell, `native/tessera-ui`
- **Slint 1.18.1** (pinned), rendered with **FemtoVG** (OpenGL ES through EGL). The FemtoVG renderer is vendored and patched with partial rendering and culling. A Skia backend exists behind a feature flag.
- **smithay-client-toolkit 0.20**, `wayland-client 0.31`, wlr-layer-shell, `wayland-egl`, `khronos-egl` (dynamic).
- **calloop** event loop (with signals), plus a fixed 4-thread work pool. The UI thread never blocks: writes go through an ordered lane, and Hyprland dispatches go through another lane.
- **zbus 5** (blocking API) for D-Bus: NetworkManager, BlueZ, UPower, logind, fprintd, MPRIS, polkit, notifications and the portals.
- **serde / serde_json** (order kept when settings are written back), `image`, `resvg` (SVG icons), `memmap2`, `fontique`.
- **material-colors** for Material You colour schemes from the wallpaper.
- **accesskit / accesskit_unix** for screen-reader accessibility.
- **ureq 3 + rustls** for HTTPS (weather, calendars, AI, updates).
- Workspace layout:
  - `core`: theme, services, environment, health, panic containment, frame pacing, wallpaper, icons.
  - `surfaces/*`: 40+ UI parts.
  - `host/*`: `shell`, `system`, `desktop`, `sharing`, `tools`.
- Surfaces:
  - appstore, captions, cheatsheet, clipboard, controlcenter, decorations, desktop;
  - dock, emoji, greeter, hotcorners, launchpad, lock, lookcover, menubar;
  - missioncontrol, network, notes, notifications, osd, osk, overlay, overview;
  - polkit, resources, screenfx, screentools, session, settings, share, shelf;
  - sidebar, spotlight, stagemanager, statusmenus, switcher, tabs, tiling, today;
  - updatescreen, wallpaper, welcome.

### The QML shell (fallback)
- **Quickshell 0.2+** (QML / Qt 6). `shell/` is the Glass look and `material/` is the Material look.
- Frozen: it gets compatibility fixes only. It starts automatically if the Rust shell crashes twice within 5 minutes. A watermark then reads "Rust shell stopped · basic fallback", with **Try again**.

### Helpers
- **tessera-events** (Rust): the event supervisor. It turns system events into JSON lines for the shell: lid, sleep and wake, USB, displays, screen share and camera use, inhibitors.
- **Python helpers**, 87 scripts in `shell/assets/scripts`. Among them:
  - `fingerprint.py` (fprintd over Gio D-Bus);
  - `fastpair.py`, the Quick Share daemon (the protocol itself, BLE and Wi-Fi Direct);
  - `sysupdate.py`, `desktop-look.py`, `features.py`;
  - `tessera-root.py`, the single root helper, run through pkexec or sudo;
  - `repair-shell.sh`, `greeter-start.sh`, `launch-shell.sh`.
- **The glass plugin `tessera-glass`** (C++, Hyprland plugin API) does the real refraction and lensing over windows, drawn by Hyprland itself. It is built against the running Hyprland's headers.

### Build, release and test infrastructure
- **Builds and tests** run on a build machine (`<builder>`), never on the machines people use: `~/bin/tbuild` there gives each build a memory budget, a FIFO queue and sccache.
- **Release builder** on `<builder>` (`tools/release-builder/tessera-release` + timer) runs the commit's tests, then builds both CPU variants, **x86-64-v3** and **generic**, tests each in a sandbox, and leaves the set unsigned for the publisher.
- **Publisher** (`tools/release-builder/publish-release`, `tessera-publish.timer`, every 2 minutes) runs on the machine that holds the signing key (`<key file>`, never on the builder), signs with **minisign** and publishes the GitHub prereleases `tessera-ui-beta` and `tessera-ui-latest`.
- **Sandbox**: `./dev.sh`, a nested Hyprland inside distrobox with its own home, runtime directory and D-Bus. Shims stop tests from touching the real Wi-Fi, Bluetooth or power mode.
- **Arena**: headless sandboxes on `<builder>` for visual checks, latency and speed benchmarks, and the "gate" that compares CPU, memory and stalls against a baseline (docs/gate.md).
- **Website**: deployed with Cloudflare wrangler.

---

## 3. Features

### Desktop
- **Menu bar** (transparent glass):
  - system, app and Window menus, plus the window title;
  - Spaces with app icons;
  - tray items with glass menus;
  - status items for battery and energy mode, Wi-Fi, Bluetooth, sound, CPU and memory, weather and keyboard layout;
  - optional buttons, a notification bell, scroll gestures for brightness and volume, auto-hide, per-screen settings.
- **The island**: an iPhone-style black pill in the bar.
  - Space dots and live activities: music, timer, stopwatch, recording, downloads and copies, calls and meetings.
  - Alerts with springs for Bluetooth devices with a battery ring, USB, displays, charging, sound output changes, low battery, Focus, and heat ("Running hot" with red ripples).
  - A mini player, a pin, iOS-style notification cards, and the volume and brightness HUD.
- **Overview** (tap Super), **Mission Control / All Windows**, **App Windows**, **Stage Manager**.
- **Dock**: recent apps, minimized windows, a Downloads stack, Trash, badges, magnification, auto-hide.
- **Desktop widgets** on a snapping grid, in three sizes:
  - Clock, World Clock, Weather, Calendar, Reminders, Batteries, Battery History, Now Playing;
  - System, Screen Time, Stocks, Shortcuts, Photos, Note;
  - Widget Stack.
- **Desktop files** on the same grid, with Stacks, Quick Preview, Share and Trash with Undo.
- **Today** greeting panel; its backdrop is baked from your wallpaper.
- Rounded screen corners; squircle window corners.

### Windows
- **Tiling**: halves, quarters, thirds, fill and centre. Use the tile grid on the green button, drag to an edge or corner for a glass preview, or Super+Ctrl+Arrows.
- **Window tabs** (Super+T), **Picture in Picture**, minimize to the dock.
- Window decorations: none, a controls pill (drawn by the plugin), or hyprbars title bars.

### Apps and panels
- **App Drawer / Launchpad**, with category folders or A–Z.
- **Spotlight search**:
  - apps, calculator, unit and currency conversion, definitions, events;
  - prefixes: `;` clipboard, `:` emoji, `/` files by name and by content, `>` commands;
  - web search, Ask AI, and reminders written in plain words.
- **Control Center / Quick Settings**: glass chips you can edit and resize, with detail pages for Wi-Fi, Bluetooth, Focus, sound with a per-app mixer, Caffeine, Sharing and battery saver.
- **Notification Center**:
  - history grouped by app, with iOS-style stacked cards;
  - calendar, to-dos, Pomodoro and stopwatch;
  - live alert entries, one per event, updated in place.
- **Clipboard** (Super+V) with image previews and pins; **emoji picker**; **Quick Preview**.
- **AI sidebar**: your own models, from Google AI Studio, Vertex AI, Anthropic, OpenAI, OpenRouter, Ollama or a custom endpoint. Also a translator and an image browser.
- **Settings** app (Super+,): every page in Rust, including Software Update, Health and Features.

### Focus and calendar
- **Focus modes**: DND, Work, Personal, Sleep and your own. Each sets which apps get through, a wallpaper, light or dark, and a schedule.
- **Calendars**: .ics subscriptions, plus Google Calendar and Tasks via Online Accounts, and iCloud and Microsoft 365. Events show in Today, Notification Center, widgets, the lock screen and Search.

### Screen tools
- Screenshots of the screen, a window or an area, with a macOS-style toolbar (Super+Shift+5) and Swappy for markup. The screen freezes while you select.
- **Circle to Search**: an edge light (dot ripple, glow, pool or bands). Circle an area, then pick a chip (What is this? / Search the web via Google Lens / Copy text / Translate) and get a streamed answer.
- **Ask about the screen**, **OCR copy text**, **translate the screen**, **screen recording** with a mic meter, **live captions**, dictation.
- Overlay widgets (Super+G): notes, resources, recorder, mixer, floating image, crosshair, MangoHud FPS limit.
- On-screen keyboard, colour picker, keyboard-shortcut cheatsheet (read live from Hyprland), periodic table.

### Sharing and devices
- **Quick Share** (Android Nearby Share), implemented natively:
  - discovery over mDNS and BLE;
  - transfer over Wi-Fi LAN or **Wi-Fi Direct** (one password prompt), falling back to Bluetooth;
  - Accept in the island, with a PIN.
- **LocalSend**, send and receive.
- **Google Fast Pair** for headphones (battery levels and alerts), with a light 4 s scan every 30 s.
- Cast to Chromecast; screen mirroring; phone link through the QR link for iPhone.
- **Firewall-aware**: ports open only while you're visible or sharing, then close. This works with ufw, firewalld, nftables and iptables.

### Lock, login and security
- **Lock screen** in a Widgets or Classic layout, with glass, frost or solid cards. **Bedside Mode** turns a charging, idle laptop into a big clock.
- **Lock island "Halo"**: the password field grows into a glass island while the fingerprint reader or camera is on.
  - Fingerprint: ridges trace in, then a tick on a match or a shake on a miss.
  - Face: brackets close in as a scan line sweeps.
- **Fingerprint robustness**, the user's design:
  - a fresh scan on every wake, screen-on, lid-open or input;
  - 3+3 tries;
  - no scanning after 2 minutes idle;
  - a sleep inhibitor that releases the reader before suspend;
  - a silent `fprintd` restart through polkit when the reader is stuck.
- **Fingerprint enrollment** in Settings:
  - a finger picker with all 10 fingers;
  - every stage a match-on-chip reader asks for (11 on some);
  - a "Saving your fingerprint" spinner during the long chip commit, then a check mark, and the sheet closes itself.
- **Face unlock**: Howdy with an infrared camera, or a regular camera with a blink liveness check (OpenCV landmarks). A regular camera only unlocks the lock screen; admin prompts need infrared, fingerprint or password.
- **Login screen** (greetd): it matches the lock screen in its Glass, Material, Plain and Text styles. Settings can switch from SDDM, GDM, LightDM or ly with **automatic fallback** if it fails to start.
- **Polkit agent** with Tessera's own password prompt, registered for the desktop's session.
- **Keyring**: unlocked by the login password. Electron and Chromium apps are given `--password-store=gnome-libsecret` so they stay signed in.

### System
- **Software Update** in two rows, both updated in the background with the desktop running:
  - **Tessera**: staged beside the installed version, incremental (only the parts whose sources changed), with signed zstd **binary patches**, a build cache, and rollback to the previous build if it crashes.
  - **System packages**: pacman plus AUR. Package replacements are handled; for example, `libfprint-tod` replaces `libfprint` and is restored if the AUR install fails.
  - Channels: **Stable** (`main`) and **Beta** (`beta`).
  - Restart when you choose, or at the next lock. A sleep inhibitor keeps the screen on during updates.
- **Health**: a local health record (`~/.local/state/tessera/health.jsonl`) of memory, CPU time, slow loop turns, missed frames and part crashes. Settings → Health lists setup items still to do.
- **Features step in the installer**: every optional part in one list, detected for the hardware and pre-ticked, and installed together with one password. Settings shows a setup row only when a feature isn't set up.
  - Fingerprint and face drivers, Bluetooth extras, battery guard, energy modes, Quick Share with its fast port;
  - cast, mirroring, phone link, login screen, boot animation, desktop look, glass plugin;
  - recorder, OCR, cloud drives, clipboard history, captions, emoji, AUR helper.
- **Battery saver**: below 20% by default, or on battery. Caps the frame rate at 60 Hz and reduces motion.
- **Energy modes** through power-profiles-daemon, with a grace period on each change.
- **GPU choice**: the integrated or the discrete GPU. Protected against loops: a GPU that can't start EGL falls back to the desktop's GPU once and resets the choice to "auto".
- **Wallpapers**:
  - dynamic by time of day (real sunrise and sunset), light and dark pairs;
  - live video wallpapers that pause when covered;
  - Auto appearance.
- **Match apps**: GTK, Qt/KDE, terminal and browser colours from the accent or wallpaper, via matugen.
- **Tessera's desktop look**:
  - Tessera Sans Flex as the app font, grayscale font smoothing;
  - Darkly and Breeze Plus for Qt apps;
  - the starship prompt and kitty in Tessera's colours, no terminal greeting.
- **Accessibility**: Reduce transparency, Increase contrast, Reduce motion, screen-reader names, full keyboard control.
- Sound effects: a login chime, the screenshot shutter, the Empty Trash sound.
- Boot splashes for Plymouth: Breathe, Orbit, Frost and Bloom.

### Looks
- **Glass** (macOS-like): materials Glass, Frost or Solid, or a custom mix of lens, frost and opacity. The lens is Thin, Depth or Auto.
- **Material** (Android-like): Material You colours from the wallpaper, Google Sans Flex, Material Symbols, and Expressive motion.
- Motion: Subtle or Expressive. Popovers and buttons give like rubber when pressed.

---

## 4. Optimisations (what makes it fast and light)

### Rendering
- **Partial rendering** in the vendored FemtoVG renderer: items outside the damaged region (from EGL buffer age) are skipped before FemtoVG sees them. **Culling** is on by default; `TESSERA_UI_NO_CULL=1` turns it off.
- **One shared set of shader programs** for every window. Mesa had kept 15 identical programs per window, which cost 75–110 MB.
- **Frame pacing follows the display's refresh rate** (60 or 120 Hz), never a fixed 16 ms.
  - Hand-animated parts tick once per frame.
  - The loop sleeps until the next real deadline; with nothing moving, timers wake together on a 250 ms grid.
- **Glass in one layer**: the plugin blurs its own copy of what's behind in two half-size passes, and the lens bends that one picture. It draws only where Hyprland repaints anyway, so static glass costs nothing.
- **Glass is synchronised with the drawn frame** (held at most 100 ms), so panels never show content before their glass.
- **Text** tuned to match Qt: unhinted glyphs, whole-pixel placement, Material weights below bold drawn as Qt draws them.

### CPU
- Idle CPU went from **7–12% to about 1.4%**:
  - the system mirror runs only on a state change;
  - one shared `pactl subscribe` instead of one per feature;
  - one shared second-tick for every service instead of about 15 timers;
  - Hyprland's window list is re-read at most once a second, even while a terminal's title spins.
- **The UI thread never waits**:
  - settings and state writes (about 30 ms with fsync) go through an ordered background lane;
  - Hyprland dispatches go through another;
  - snapshots run on a fixed 4-thread pool;
  - an 8 ms tripwire in the timing log names any slow step.
- The update page sends only the last 200 log lines, so it no longer lags.

### Memory
- Two malloc arenas and a fixed 128 KiB mmap threshold, set at startup. glibc had kept one arena per thread, 110–130 MB after use.
- After 10 s of quiet the heaps are marked no-huge-pages and trimmed.
- Windows made ahead start soon after the first frame, are hidden, and are retired after 5 minutes unused (clipboard, emoji).

### Startup and responsiveness
- Panels are made ahead on the first quiet turn after the intro, several per turn.
- Reopening the clipboard and emoji picker went from about 84 ms to about 25 ms.
- Close animations use the QML's exact timings (Notification Center 196 ms, power menu 227 ms).
- The island and its HUD use a stepped Qt-style spring, so a held volume key no longer stutters.

### Robustness
- **Panics are contained per part**:
  - a crashed part is rebuilt after 1 s, then after 3 s;
  - after 3 crashes in 2 minutes the part stays off until the next login and one notification says so;
  - two whole-shell crashes start the QML fallback.
- **Settings files**: a broken `config.json` is kept aside, never overwritten. Writes are a locked read-merge-write; symlinks and permissions are kept; odd values are clamped.
- **Hardware variety**: no or several Wi-Fi cards, batteries and backlights; iwd or ConnMan; no EGL (falls back to QML); software GL; portrait and fractional-scale displays.
- **Watchdog** (`tty-shell.sh`) restarts a frozen shell. It skips its crash count while a deliberate restart holds the lock.
- **Login fallback**: if greetd fails or the login screen doesn't appear within 90 s, the previous login manager comes back automatically.

### Updates and builds
- Builds are named by the **git tree of `native/`**. A commit that changes only QML or Python reuses the existing binary of its channel (the builder keeps each channel's builds apart: the program carries its channel).
- **CPU variant detection** (`/proc/cpuinfo`): x86-64-v3 or generic.
- Every download is checked before install: minisign signature, tree name, size and sha256 of each file, and each program's `--version`. Settings calls a build "signed" only when the installer checked the signature of the very file that's running.
- Signed **zstd binary patches** between versions; the full download is the fallback.
- **Tests before a release**: the commit's Python, Node and Rust tests must pass, then each variant is started in a sandbox and must answer, draw, lock and unlock (`ipc call lock state`) before the set is offered; the publisher refuses a set whose tests didn't pass.
- **Rollback**: the previous build is kept as `.prev` and comes back on a crash.
- Sources (`tools/sources.txt`): code updates come from GitHub only; a fresh install tries GitHub, then the GitLab mirror, then a plain tarball; ready-made builds come from GitHub's releases, then the GitLab mirror's packages when it has them. Every source is checked the same way. The AUR falls back to Arch's GitHub mirror, and slow pacman mirrors are re-ranked.
- Build profiles:
  - `release`: fat LTO, one codegen unit;
  - `fast`: thin LTO, 16 units, incremental (for testing and deploys);
  - `local`: thin LTO, used for builds on your own machine and for Beta's published builds ("fast build" in Settings).

---

## 5. How it is installed

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/monishbl/tessera/main/setup.sh)
```

The installer has an interactive full-screen mode (python-rich) or a classic mode that asks one question at a time. Every question comes first, then the whole plan, and nothing changes until you start. The steps:

1. Checks: Arch, sudo, internet, disk, GPU render device, virtual machine.
2. A snapshot (snapper or Timeshift) when one is set up.
3. Packages: Hyprland and the rest from the official repositories, plus AUR extras when chosen.
4. The **Features** step: hardware drivers, fingerprint, face and the rest, all with one password.
5. The signed, prebuilt `tessera-ui` for this CPU, or a local build as a fallback.
6. The glass plugin, built against the running Hyprland.
7. The shell and Hyprland config. Your own config is backed up once to `~/.config/hypr.pre-tessera`.
8. Services: NetworkManager, Bluetooth, power profiles, and the VM guest tools when in a VM.
9. The greetd login screen.
10. Verify (`install.sh --verify`).

The `tessera` command afterwards:

| Command | What it does |
|---|---|
| `tessera doctor` | checks everything |
| `tessera update` | updates Tessera |
| `tessera rebuild` | rebuilds the plugin and shaders |
| `tessera repair` | restarts the shell |
| `tessera logs` | shows the shell's log |
| `tessera uninstall` | removes Tessera and restores your config |

Your own Hyprland changes go in `~/.config/hypr/custom.lua`, which loads last.

### Files

| Path | What |
|---|---|
| `~/.config/tessera/config.json` | settings; edits apply live |
| `~/.config/tessera/focus.json`, `calendars.json` | Focus modes, calendar subscriptions |
| `~/.local/state/tessera/` | layout, to-dos, notes, pins, reminders, AI chats, health record |
| `~/.local/share/tessera/bin/` | `tessera-ui`, `tessera-events` (+ `.prev` for rollback) |
| `~/.cache/tessera/` | baked glass backdrop, previews, install log |
| `/usr/share/tessera/` | the login screen's own copy: binary, shell, Material, `greeter.lua`, look, wallpaper |
| `/etc/tessera/` | login-switch record, boot-splash record |

---

## 6. Keys and gestures

| Keys | Action |
|---|---|
| Super (tap) | Overview |
| Super+Space | Search |
| Super+Ctrl+Space | App Drawer |
| Super+V | Clipboard |
| Super+Tab | All Windows |
| Super+A | AI sidebar |
| Super+C / Super+N | Quick Settings / Notification Center |
| Super+, | Settings |
| Super+/ | Shortcut cheatsheet |
| Super+Shift+3 / 4 / 5 | Screenshot screen / area / tools |
| Super+Shift+Space | Circle to Search |
| Super+Shift+A | Ask about the screen |
| Super+Shift+T / Y | Copy text / translate the screen |
| Super+Shift+R | Recording |
| Super+G / Super+K | Overlay widgets / on-screen keyboard |
| Super+Ctrl+Q | Lock |
| Super+Ctrl+Escape | Repair the shell |
| Super+Ctrl+Arrows, F, C, R | Tile, fill, centre, restore |
| Super+T, Super+[ / ] | Tabs |
| Super+Shift+P | Picture in Picture |

Gestures:
- 3 fingers left or right switch spaces; up or down opens Overview or App Windows.
- 4 fingers left or right open the Sidebar or Notification Center; pinch or spread opens the App Drawer or shows the desktop.
- A 3-finger long press is Circle to Search.
- Three-finger drag can move windows.

---

## 7. Workflow

- Code is edited on any machine; builds and tests run on `<builder>` over ssh, one heavy run at a time in a capped scope.
- Releases are signed only on the machine that holds the key.
- A second standalone install is kept as a reference: both must behave the same.
- Branches:
  - `beta`: continuous;
  - `main`: stable, promoted only by fast-forward on the owner's word.
- Agent branches are squash-merged and scanned for secrets before pushing.

The hosts, logins and key paths behind the placeholders are in the operator's private notes, not in the repository.

---

## 8. History (short)

| Date | Milestone |
|---|---|
| 26 Oct 2026 | Started as **iRice**, a Quickshell (QML) rice after macOS |
| 1–3 Nov | Glass and Material Expressive rounds, island, lock redesign, login screens, Quick Share |
| 4 Nov | Renamed to **Tessera**; first launch films |
| 7 Nov | The Rust shell (`tessera-ui`) goes live |
| 13–16 Nov | Rust-only features, text tuning, idle CPU work, a release builder on a build machine, QML kept as a fallback |
| 17 Nov | Face unlock, lock island, battery saver, health record, panic containment, close-timing parity |
| 18 Nov | Partial rendering, fingerprint after sleep, GPU loop fix; stable promoted |
| 19 Nov | Background incremental updates, Software Update redesign, login switch, iOS notification stacks, desktop look |
| 20 Nov | Installer Features step, fingerprint enrollment fixes, a sweep on the reference install, quiet login, hyprpm nag removed |
| 20–27 Nov | Two whole-codebase audits and their fixes (each change and its status: RUST-ONLY.md) |

---

## 9. Where things stand

Not kept here, so it can't go stale: each Rust-only change and whether it
has been checked is in [native/tessera-ui/RUST-ONLY.md](../native/tessera-ui/RUST-ONLY.md);
what waits on hardware, upstream or a decision is in [PENDING.md](../PENDING.md);
how releases are built, tested, signed and offered is in [releases.md](releases.md).
