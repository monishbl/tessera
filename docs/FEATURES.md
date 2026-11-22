# Tessera features

Everything Tessera does, area by area. For a shorter tour, see the [README](../README.md) or [tessera.monishbl.in](https://tessera.monishbl.in). Installing: [INSTALL.md](INSTALL.md).

Tessera is a desktop shell for Hyprland in two looks: **Glass**, after macOS, and **Material**, after Android. Every feature below works in both unless it says otherwise. It covers what end4's illogical-impulse dots offer and more ([ROADMAP.md](../ROADMAP.md) compares them feature by feature).

- [Desktop](#desktop)
- [Windows](#windows)
- [Apps](#apps)
- [Panels](#panels)
- [Focus and calendar](#focus-and-calendar)
- [Tools](#tools)
- [System](#system)
- [Glass, Frost and Solid](#glass-frost-and-solid)
- [Keys and gestures](#keys-and-gestures)
- [Sharing files](#sharing-files)
- [AI](#ai)
- [Where things are kept](#where-things-are-kept)
- [Not included](#not-included)
- [Fonts and icons](#fonts-and-icons)

## Desktop

### Menu bar

A transparent menu bar with everything end4's bar offers:

- system, app and Window menus, plus the window title
- Spaces with app icons (click or scroll to switch)
- tray icons with glass menus, Now Playing and a recording indicator
- status items with their own menus: battery and energy mode, Wi-Fi, Bluetooth, sound, CPU and memory, weather, keyboard layout
- every item can be hidden in Settings → Menu bar, including the app and Window menus, Search, AI, Quick Settings and the clock
- optional buttons: screenshot, colour picker, mic, keyboard, dark mode, energy mode, recording
- unread notifications as a bell with a dot, a dot, or a count; scroll gestures (left half brightness, right half volume), auto-hide, per-screen
- click a menu to open it, another title to switch, the same title to close

### The island

The island sits in the menu bar, black like the iPhone's.

- **Closed:** dots for your spaces, or a compact live activity (recording, timer, music, stopwatch).
- **Alerts** spring open with their own animation: headphones and other Bluetooth devices connecting (with a battery ring), USB devices and drives (with Open), displays, the charger filling its ring, sound moving to another output, low battery, track changes, Focus, CPU or memory running hot. Alerts that arrive together stack in one island and stagger in, each leaving on its own time.
- **Running hot:** when the CPU package passes a temperature you set (90 °C by default), a "Running hot" alert opens and, until it cools, the closed island turns red with heat ripples from the active space.
- **Live activities** (music, a timer, the stopwatch, recording) sit in the closed island: art, a scrolling title and playing bars, with controls. They replace the space dots, or sit around or beside them (Settings → Menu bar, where every piece can be switched off). Several live at once: scroll over the island to switch, or list them all.
- **More live activities:** downloads and file copies (cp, mv, rsync, dd) with progress and speed; calls and meetings (Discord, Zoom, Teams, Slack, Google Meet and more) with mute, open and leave. Wi-Fi, VPN, hotspot and Ethernet changes are alerts. The status row of the open island shows the CPU temperature next to CPU.
- Point at it for a mini player over the album art with seek, open-the-player and volume. Pin it open with the pin or a right-click. Hover it for details.
- Notifications can appear there as cards instead of banners, and the volume and brightness HUD can show there too.
- Preview any alert with `qs ipc call island demo device` (or `usb`, `display`, `audio`, `charging`, `battery`, `notification`).

### Overview and privacy

- **Overview:** tap Super for this screen's spaces as plain containers with a tile per open window (app icon and name). Drag a tile to another space, or type to search apps.
- **Privacy dots:** orange while an app uses the microphone, green for the camera, purple while the screen is shared; click for which apps.

### Dock

Recent apps, minimized windows, a Downloads stack and Trash, right-click menus, optional magnification and auto-hide (rest at the bottom edge to bring it up). Unread-count badges from apps that report them (Telegram, Discord, Thunderbird…) and a dot when an app wants attention. Click the app in front again for App Windows.

### Desktop widgets and files

- **Widgets** on a snapping grid: Clock, World Clock, Weather, Calendar, Reminders (with due dates), Batteries, Battery History, Now Playing, System, Screen Time, Stocks, Shortcuts (one-click buttons), Photos, Note (each with its own text) and **Widget Stack** (several widgets in one, scroll to flip; it brings forward music, the next event or a low battery), in small, medium and large sizes. Drag to move; right-click for sizes, Remove and the widget's own options; Edit Widgets (right-click the desktop) opens a gallery to add more.
- **Files on the desktop** (`~/Desktop`) on the same grid, from the top right: double-click to open, drag to arrange, right-click for Quick Preview, Share, Move to Trash (with Undo). **Use Stacks** groups them by kind.
- Rounded screen corners.

## Windows

- **Decorations** (Settings → Appearance): none, window controls, or title bars via the hyprbars plugin. Window controls are a small pill at the top of each window that opens into close, minimize and zoom; with the glass plugin, Hyprland draws them so they move with the window.
- **Corners:** adjustable radius with continuous (squircle) corners.
- **Minimize:** windows park in the dock's Minimized section.
- **Tiling:** halves, quarters, thirds, fill and centre from the Window menu, from the tile grid that opens when you rest on the green window button, or with Super+Ctrl+Arrows / F / C. **Drag a window to a screen edge or corner** and a glass preview shows where it will go (needs the glass plugin). Arrange two to four windows side by side; Return to Previous Size puts a window back.
- **App Windows:** one app's windows from every space (three fingers down, or click the app in front in the dock).
- **All Windows:** live window previews and Spaces.
- **Picture in Picture:** browsers' video pop-outs float small in a corner on every space; any window can do it from the Window menu or Super+Shift+P.
- **Window tabs:** merge windows into tabs (Window menu or Super+T, Super+[ / ] to switch). Tabs look like Hyprland's bar restyled, or Tessera's own with icons (Settings → Appearance).
- **Stages:** one group of windows in front, the rest as thumbnails on the left. New apps open in their own stage; Shift-click a thumbnail to merge it into the front one. Turn it on in Quick Settings.

## Apps

- **App Drawer** (Super+Ctrl+Space or the dock's Apps button): every app in category folders or A–Z, with search and keyboard navigation.
- **App icon styles** (Settings → Appearance): Default, Dark, Clear, Tinted, or Auto (dark in dark mode).
- **Software:** apps from the Arch repositories and Flathub in one place, with pictures and descriptions. The AUR is there when you ask for it.

## Panels

- **Quick Settings:** each control its own glass chip (or on one glass panel: Settings → Menu Bar & Dock → Quick Settings → Panel background): Wi-Fi, Bluetooth, Focus, Now Playing, display and sound sliders and more. Choose Edit Controls to add, remove, resize and drag controls, from a gallery that also has game mode, anti-flashbang, Identify Song, keyboard, colour picker, screenshot, recording, battery, weather, CPU and memory, EasyEffects and Cloudflare WARP (hidden when their tool isn't installed). Right-click a control (or click a wide one's label) for its details, which open as a second popover beside Quick Settings: Wi-Fi networks, Bluetooth devices, Focus modes, and sound output with a per-app mixer.
- **Notification Center:** history grouped by app, calendar with your events, to-do list, and a Pomodoro timer with stopwatch. Something that keeps changing stays one entry, with every update kept under it.
- **Notifications:** glass banners with actions. Several stack into one with an expand button, and only a new one slides in.
- **Search:** apps, calculator, unit and currency conversion ("10 km to mi", "100 usd in eur"), definitions ("define serendipity"), actions, calendar events, `;` clipboard history, `:` emoji, `/` files by name and by what's inside, `>` commands, web search. Shift+Space or Ctrl+Y on a file opens Quick Preview. Ask AI right in Search (`?` or the Ask row) for a short answer, then continue in the sidebar. Type "remind me to call mum at 5pm" to add a reminder.
- **Clipboard** (Super+V): history with image previews, search, pins that survive restarts, paste as plain text.
- **Quick Preview:** previews of images, video, audio, PDFs, text and folders without opening an app (Search, the Downloads stack, desktop files).
- **Sidebar:**
  - **AI chat** with the models you add (see [AI](#ai)). Every chat is kept until you delete it, unless you turn chat history off.
  - **Translator**
  - **Anime image browser:** safe-rated results unless you switch on Explicit.

## Focus and calendar

- **Focus modes:** Do Not Disturb, Work, Personal, Sleep, and your own. Each lets chosen apps through, can change the wallpaper and light/dark mode while on, and can run on a schedule, for an hour, or until the evening. The active mode shows in the menu bar.
- **Calendar:** subscribe to Google, iCloud, Outlook or Nextcloud calendars with their .ics link (Settings → Calendar). Events show in Notification Center and Search, with reminder notifications. Add your own reminders in plain words.
- **Google Calendar and Tasks:** Settings → Google connects Calendar and Tasks through desktop Online Accounts; you sign in without creating a Google Cloud project. A custom Desktop OAuth client remains an advanced option. Events appear in Calendar, Today and Search; tasks appear alongside local reminders with completion, rename and deletion. Choose a Google task list for new tasks or keep them local. Keep opens in your browser; native personal Keep sync isn't supported. See [google-integration.md](google-integration.md) for setup, security and limitations.

## Tools

- **Screenshots** of the screen, a window or a selection; Super+Shift+5 opens a toolbar. Area screenshots open in Swappy to crop, draw and annotate (Settings → Keyboard Shortcuts).
- **Ask about the screen** (Super+Shift+A): select part of the screen and ask the AI about it ("what's this error?").
- **Circle to search** (Super+Shift+Space, a 3-finger long press, or the circle button in the AI sidebar): circle, scribble over or box anything on the screen and the AI says what it is. **Search the web** under the answer sends the picture to Google Lens (only when you click it) and opens the results.
- **Copy text from pictures:** select the text in a screenshot or picture, read on this computer (OCR); translate text on screen; record the screen.
- **Overlay widgets** (Super+G): notes, resources, recorder, volume mixer, floating image, crosshair, MangoHud FPS limit. Widgets can be pinned and made click-through.
- On-screen keyboard, colour picker, keyboard-shortcut cheatsheet, periodic table.

## System

- **Lock screen** (Settings → Lock Screen): Widgets layout (large clock, small widgets, Now Playing with controls, notifications) or Classic layout; cards in glass, frost or solid; light, dark or automatic text; notification previews optional.
- **Bedside Mode:** on a charging laptop left alone, the lock screen becomes a large clock, two widgets or your photos (red at night), and the screen stays on.
- **Login screen** that matches the lock screen (greetd), set up by the installer or with `./install.sh --only=login`.
- Power dialog, polkit password prompt, volume and brightness HUD.
- **Sound effects:** a chime at login, the shutter for screenshots, paper for Empty Trash (Settings → Notifications).
- **Match apps** (Settings → Appearance): GTK, Qt/KDE, terminal (foot, kitty, Alacritty) and browser colours from your accent or wallpaper, with matugen. Dolphin's KDE colours and the desktop light/dark preference follow Tessera even when Match apps is off.
- **Accessibility:** Reduce transparency and Increase contrast.
- **Icons** are each app's own logo, restyled to match the look (clear glass, or Material You themed); icon themes such as Colloid, Tela circle or Papirus are optional. GTK apps use Adwaita and the pointer is Bibata.
- **Wallpapers:** dynamic (time of day using sunrise and sunset, or light/dark pictures), live video wallpapers that pause when covered, and an Auto appearance that turns light at sunrise and dark at sunset. **Wallpaper sets** are found in your wallpaper folder automatically: pictures named "Name Light" and "Name Dark" switch with the theme, and a sub-folder of pictures follows the time of day; pick one in Settings → Wallpaper.
- **Software Update** with Stable and Beta channels, conflicts you decide, and undo from the package cache (see the [README](../README.md#update-and-undo)).
- Battery warnings, idle handling (hypridle).
- **Settings** (Super+,) for appearance, wallpaper, menu bar, dock, desktop and weather, notifications, Focus, calendar, AI, sidebar and shortcuts.

## Glass, Frost and Solid

Choose the material in Settings → Appearance: Glass, Frost or Solid, or your own mix of the three with Advanced… (lens, frost and opacity sliders; the 3D highlights fade as it frosts and are gone when solid):

| Material | What it does | Cost |
|---|---|---|
| **Glass** (default) | Refracts and lenses whatever is behind it, windows included | Low |
| Frosted | Blurs what's behind it | Low |
| Solid | No transparency; also makes the menu bar opaque | None |

**Lens** sets how thick the glass looks: Thin (edge lensing only), Depth (a thick lens that bends the whole shape), or Auto (depth on large panels, thin on small controls).

Real glass over windows needs the **tessera-glass** Hyprland plugin, which the installer builds (or Settings → Appearance → Build plugin…). Hyprland then draws the glass itself, and only where it repaints anyway, so static glass costs nothing. Without the plugin, Glass refracts the wallpaper only and falls back to Frosted over windows. Rebuild the plugin after a Hyprland update (`tessera rebuild`); until then Tessera uses Frosted.

The shell idles below 0.1% CPU. A live wallpaper is the exception while it's visible (about 10% of a core for a 720p video in testing); it pauses when windows cover most of the screen.

**Motion** (Settings → Appearance): Subtle, Snappy or Expressive. Press and drag on a popover, menu or button and it gives like rubber, then springs back; nothing moves on hover.

The Material look has its own page: [material-look.md](material-look.md).

## Keys and gestures

Super is ⌘. Press **Super+/** for the full list, which is read live from Hyprland. Put your own Hyprland changes in `~/.config/hypr/custom.lua`: Tessera loads it last, and an error in it won't break the rest of the config.

| Keys | Action |
|---|---|
| Super (tap) | Overview of spaces, with search |
| Super+Space | Search |
| Super+Ctrl+Space | App Drawer |
| Super+V | Clipboard history |
| Super+Tab | All Windows |
| Super+A | AI and tools sidebar |
| Super+C / Super+N | Quick Settings / Notification Center |
| Super+, | Settings |
| Super+Shift+3 / 4 / 5 | Screenshot screen / area / tools |
| Super+Shift+S | Screenshot of the screen (saved straight away) |
| Super+Ctrl+Shift+S | Screenshot of an area |
| Super+Escape | Close everything (if a panel is stuck) |
| Super+Ctrl+Escape | Repair the shell: duplicated bars or a frozen shell. Restarts only the shell; apps and windows keep running (also in the system menu) |
| Super+Shift+T / Y | Copy text from screen / translate screen |
| Super+Shift+A | Ask AI about part of the screen |
| Super+Shift+Space | Circle to search |
| Super+Shift+R | Start or stop recording |
| Super+G / Super+K | Overlay widgets / on-screen keyboard |
| Super+Ctrl+Q | Lock |
| Super+Q, F, D | Close, full screen, zoom window |
| Super+Ctrl+Arrows | Tile to a half of the screen |
| Super+Ctrl+F / C / R | Fill, centre, return to previous size |
| Super+Shift+P | Picture in Picture |
| Super+T, Super+[ / ] | Merge into tabs (or move a tab out), previous / next tab |
| Super+1–9 | Spaces |
| Super+scroll | Next or previous space |

| Gesture | Action |
|---|---|
| 3 fingers left or right | Switch spaces |
| 3 fingers up / down | Overview / App Windows |
| 4 fingers left / right | Sidebar / Notification Center |
| Pinch / spread 4 fingers | App Drawer / Show Desktop |
| 3 fingers, long press | Circle to search (needs Tessera's glass plugin) |
| Three-finger drag (Settings → Trackpad) | Move the window under your fingers; the swipes above move to 4 fingers |
| Touchscreen: pull down from the menu bar | Quick Settings (right third) or Notification Center |
| Touchscreen: swipe up from the bottom edge | Show a hidden dock |

## Sharing files

### Nearby sharing (LocalSend)

Send files to phones and computers running LocalSend, and receive from them after an Accept in the island.

1. Open LocalSend on the receiving phone or computer and connect both devices to the same network.
2. In Tessera, go to **Settings → Sharing → Send files…**, choose one or more files, then click the receiving device. You can also use **Share…** on a desktop file, **Share** in Quick Preview, or "Send Files" in Search.
3. The receiving device must accept the transfer.

Who can send to you: Settings → Sharing: No one, Everyone for 10 minutes, Everyone. Sending works with receiving off.

### Android phones (Quick Share)

Tessera speaks Android's Quick Share protocol itself; nothing else needs installing.

- **Receiving:** in Settings → Sharing → Android phones (Quick Share), set **Visible to phones** to *Everyone for 10 min* or *Everyone*. Phones on the same Wi-Fi then see this computer in their Quick Share sheet, and you're asked in the island, with the PIN both screens show, before anything is saved. Files go to Downloads, or the folder you choose; links and Wi-Fi networks can be opened or copied.
- **Sending:** use Share on a file and pick the phone. It must have Quick Share open and be visible to Everyone.
- **Without a shared network** it works over Bluetooth (turn it on; Settings → Sharing offers a button): the phone and this computer find each other and connect over Bluetooth LE, then the transfer moves to a Wi-Fi Direct link for full speed, which asks for your password once (it needs root for a moment; nothing stays configured afterwards). Without the password, or without Wi-Fi, it carries on over Bluetooth, which is slow.
- **Firewall:** with a firewall on (ufw, firewalld, nftables or iptables), Tessera opens sharing's ports only while you're visible to others or sharing (Quick Share 53318–53319, DHCP and mDNS; LocalSend 53317; 53319–53320 while a QR-code share or a Chromecast plays a file from this computer) and closes them after, without a password for an administrator at the computer. Settings → Privacy & Security → Firewall says what's open now.
- **Troubleshooting:** if they can't find each other, make sure avahi-daemon is running. If phones ask to pair over Bluetooth, Settings → Sharing → **Stop pairing prompts** turns off BlueZ's experimental Ranging profile, which causes it. For testing with a phone, `TESSERA_QS_DEBUG=1` logs what happens to `$XDG_RUNTIME_DIR/tessera-quickshare.log` (no keys, PINs or passwords).

### Earbuds (Fast Pair)

Open the case and your earbuds are there, with their picture (it works with Google Fast Pair). One click connects them, then each earbud's battery and the case's show in the island, with a warning before they run out.

## AI

Nothing is sent anywhere until you set up a model. New installations have no preset model list.

1. Open **Settings → AI** and choose a provider: Google AI Studio, Vertex AI (Gemini), Anthropic, OpenAI, OpenRouter, Ollama, or a custom OpenAI-compatible endpoint.
2. Enter the exact model ID from your account, optionally a display name, and a credential. Custom servers also need their API base URL.
3. Save the model, then pick it from the sidebar's model menu or Settings. Saved models appear in the sidebar and Search.

Models can be edited and removed, and the same model ID can be saved for different providers or connections. Credentials stay in the system keyring or a private file, separate from model settings.

**Vertex AI** supports [Express mode API keys](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/start/express-mode/overview) or an OAuth access token with your Google Cloud project and region (including `global`). Pasted OAuth access tokens are short-lived and must be replaced when they expire. Adaptive thinking is an optional per-model setting for Anthropic models that support it.

## Where things are kept

- `~/.config/tessera/config.json`: settings. Edits apply live.
- `~/.config/tessera/focus.json` and `calendars.json`: Focus modes and calendar subscriptions (also editable in Settings).
- `~/.local/state/tessera/`: layout, to-dos, notes, clipboard pins, your reminders, AI chats, and API keys when no keyring is running. Private files are owner-only.
- `~/.cache/tessera/`: the baked glass backdrop, Quick Preview's PDF pages, the install log.
- With Match apps on: `tessera-colors.*` files next to your GTK, terminal and Qt configs (each config gets one include line); turning it off empties them.

## Not included

- **Calendar editing:** subscribed .ics calendars are read-only; your own reminders are editable. Times with a time zone are read as local time.
- **Global app menus:** Linux apps would need an AppMenu registrar service, which isn't there. The menu bar shows the app and window menus instead.
- **hyprglass "Ultra" tier:** the plugin hooks Hyprland internals and wasn't tested here. Glass, Frosted and Solid cover the looks without it.
- **Vertical or bottom bar:** end4 offers these; the menu bar stays at the top.

## Fonts and icons

Phosphor (MIT), Tessera Sans Flex (Google Sans Flex, OFL-1.1) and Tessera Symbols (Material Symbols, Apache-2.0) are bundled; Inter and Papirus are used when installed. The bundled wallpapers are generated by `tools/gen-wallpapers.sh`. See [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).
