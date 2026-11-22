# Installing Tessera

The short version is in the [README](../README.md#install). This page has the details.

## Requirements

- **Arch Linux**, or an Arch-based system with pacman (CachyOS, EndeavourOS). A fresh install or your current one.
- **Hyprland 0.56 or 0.57.** The installer installs Hyprland from the official repositories; [compat.json](../compat.json) holds the tested range, and Software Update holds back a Hyprland that hasn't been tested yet.
- **Quickshell 0.3.2 or newer.** When yours is older or missing, the installer builds its own (`tools/build-quickshell.sh`), so Arch's quickshell updates don't affect it.
- A GPU render device, sudo, an internet connection and some free disk space. The installer checks each.

## Two ways in

### Try it next to your desktop

```sh
bash <(curl -fsSL https://tessera.monishbl.in/get)
```

This clones Tessera into `~/tessera` and sets it up to run on its own console. Your `~/.config` and your usual session aren't touched. Asked in a terminal, it offers the full install instead. Options: a folder name (instead of `~/tessera`), `--with-settings` to start from the shared look, `--yes` to not ask before installing packages.

Then:

1. Switch to a free console: press Ctrl+Alt+F3 and log in.
2. Run `cd ~/tessera && ./demo.sh --check && ./dev.sh tty`. The check says what's missing, if anything.
3. Super+Shift+Escape (pressed twice) ends it; Ctrl+Alt+F1 or F2 takes you back.

If the session hangs, switch to another console (Ctrl+Alt+F4) and run `cd ~/tessera && ./dev.sh tty-stop` (`--force` to kill it outright, `--dry-run` to see what it would stop). It only touches the test session. A watchdog restarts the shell when it stops responding; Super+Escape closes any stuck panel.

To remove it, delete the `~/tessera` folder.

### Install it as your desktop

```sh
bash <(curl -fsSL https://tessera.monishbl.in/get) --install
```

or straight from GitHub:

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/monishbl/tessera/main/setup.sh)
```

It downloads from GitHub, and only when that fails from the other sources in `tools/sources.txt` (`TESSERA_MIRRORS` overrides it). Software Update fetches new code from the official GitHub repository only. Before installing packages it checks your pacman mirror lists (Arch's and CachyOS's): ones that are short or behind are re-ranked first (the old lists kept as `*.tessera-backup`), and when the AUR is down, AUR packages come from Arch's GitHub mirror of it.

## What the installer does

It asks which way you'd like: **interactive** (full screen, arrow keys and space to tick, with live progress and a summary; it needs `python-rich` and offers to install it) or **classic** (one question at a time). Either way every question comes first, then the whole plan (each package, file and service), and nothing changes until you start. Then it:

1. checks your system (Arch, sudo, internet, disk space, a GPU render device, virtual machine)
2. takes a snapshot first when snapper or Timeshift is set up
3. updates the system and installs Hyprland, Quickshell and everything Tessera uses (from the official repositories)
4. optionally installs the Bibata pointer from the AUR (it sets up `yay` if you have no AUR helper), extra emoji styles (default no), and icon themes (Colloid, Tela circle, Numix Circle, Flat Remix, Papirus; default none: Tessera styles the apps' own icons)
5. installs what this computer's hardware is missing, as you choose: firmware (Intel SOF sound, Broadcom Wi-Fi), video decoding and Vulkan for each GPU, NVIDIA's driver (only when you tick it), the fingerprint reader's and face camera's drivers (each asked), and a display scale for a sharp screen
6. compiles the shaders and builds the glass plugin against your Hyprland, installing it as `~/.local/share/tessera/tessera-glass.so` (written to a new file and swapped in, so a running session isn't disturbed)
7. installs `tessera-ui` and `tessera-events`, the interface and system watcher in Rust: ready-made and signed by default, built here when there's none (`--ui-source=build` to always build)
8. installs the shell (`~/.config/quickshell/tessera`) and its Hyprland config, backing up yours once (`~/.config/hypr.pre-tessera`)
9. saves the look, light or dark and login style you picked
10. turns on NetworkManager, Bluetooth, power profiles and, in a VM, the guest tools
11. sets up the Tessera login screen (greetd), or starts Hyprland when you log in on tty1
12. checks the result and tells you how to fix anything that's off

Nothing of yours is removed or turned off unless you chose it: PulseAudio, another network service or another login screen is only replaced when you say so (with `--yes`, never). An optional step that fails is reported and skipped. The installer can be run again at any time (it updates), and everything is logged to `~/.cache/tessera/install.log`.

### Installer options

From a clone: `./install.sh`. The full list is in `./install.sh --help`.

| Option | |
|---|---|
| `--tui` / `--cli` | which installer, without asking |
| `--yes` | no questions: the essentials only, never optional features such as face or fingerprint unlock or the login screen unless you name them (Software Update uses it) |
| `--plan` | show the whole plan (packages, files, services), change nothing |
| `--dry-run` | the plan and each step's commands, change nothing |
| `--dev` | link the shell to the clone instead of copying it |
| `--no-deps`, `--no-greeter`, `--no-extras`, `--no-snapshot` | skip that part |
| `--verify` | only check an existing install (same as `tessera doctor`) |
| `--only=login` | only switch to the Tessera login screen |

## The `tessera` command

| Command | |
|---|---|
| `tessera doctor` | check everything (in a session, also that the shell and glass plugin are running) |
| `tessera update` | get the latest Tessera and install it |
| `tessera undo-update` | go back to before the last update (works from a text console too) |
| `tessera rebuild` | rebuild the glass plugin, shaders and native helpers, e.g. after a Hyprland update |
| `tessera repair` | restart the shell (duplicated bars, a frozen shell); apps keep running |
| `tessera logs` | the shell's log |
| `tessera uninstall` | remove Tessera, put your Hyprland config and login screen back |

`tessera uninstall` (or `./uninstall.sh`, with `--dry-run` to see what it would do) leaves packages installed and keeps your settings in `~/.config/tessera`.

## If Tessera can't log you in

Press Ctrl+Alt+F2, log in on the console, and run `sudo /usr/lib/tessera/tessera-root rescue`. Then go back to the login screen with Ctrl+Alt+F1 and log in with your password. [RECOVERY.md](RECOVERY.md) has the rest: what this undoes, how to put it back, and what protects the login.

## In a virtual machine

For trying it or a demo: give it 4 GB of memory, 2+ cores and 3D acceleration, or Hyprland can't start.

- virt-manager: *Video: Virtio* with *3D acceleration*, and *Display Spice: OpenGL* on.
- VirtualBox: *VMSVGA* with *Enable 3D Acceleration*.

Install Arch (for example `archinstall` with a user that has sudo, NetworkManager, and no desktop), log in, run the install line and restart. The installer turns on the software cursor and the guest tools for the VM by itself.

## Optional extras

- `songrec` for Identify Song
- `mangohud` for the FPS limit
- `ollama` for local AI
- `easyeffects`, `cloudflare-warp-bin` for their toggles
- hyprbars for title bars: Settings → Appearance → Decorations → Title bars → Install… builds it with `hyprpm`

To preview the login screen from your session: `TESSERA_GREETER_MOCK=1 qs -p shell/greeter.qml` (the password is "tessera").
