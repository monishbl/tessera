# Update system rework (plan, 22 Nov 2026; steps 3–6 written, not shipped)

## Why

Updates make laptops stutter because every update compiles the shell on the
laptop (655 crates and a link, about 10 minutes on every core). Ready-made
builds are meant to make that unnecessary, but publishing stopped after
16 Nov (beta) and 17 Nov (stable). Since then every update falls back to a
local build.

Done already (beta 47a4fdfe): local builds yield to the desktop. They run
with CPUWeight=idle and IOWeight=1, MemoryHigh at 55% of RAM, half the cores
and nice 19. `TESSERA_BUILD_GENTLE=0` gives full speed.

A GPU can't help. Compiling and linking only run on the CPU, wherever the
build happens.

## Decisions (user, 22 Nov)

- **Scope:** Tessera plus its system packages. One bundle per version,
  together with a list of tested package versions. Not a whole OS image.
- **No ready-made build yet:** ask the user. Wait (the default), or build now
  (gently, about 10 minutes).
- **Builder:** server1, fixed and watched. server1 builds, the main laptop
  signs and publishes. A health check makes sure it can't stall silently.
- **Download:** the whole bundle each time, no patches.

## What updating becomes

1. Download one signed bundle for the new version.
2. Verify its signature.
3. Put system packages at the tested versions.
4. Swap it in atomically, keeping the previous version for undo.

Nothing is compiled on a user's laptop. Settings and files in the home folder
are never part of the bundle. Hyprland config edits keep the three-way merge
(tools/config-sync.py).

## Steps

1. **Restart publishing and watch it (from the main laptop).**
   - Find why the server1 builder (tools/release-builder) and the laptop
     publisher (tessera-publish.timer) stopped after 17 Nov. Fix them and
     publish the current beta.
   - Add a health check: notify when the newest release is more than a few
     hours behind origin/beta or origin/main.
   - Every push to beta or main pokes the builder (tools/push-release).
     Reaching server1 from system1 needs Tailscale approval, which is pending.
2. **Faster publishing.**
   - Beta uses the "local" profile (thin LTO, minutes). Stable keeps full LTO
     (about 45 minutes).
   - Install mold on server1.
   - Build the x86-64-v3 and generic variants in parallel if memory allows.
3. **The bundle.** server1 builds everything into one file per version:
   - tessera-ui and tessera-events,
   - the glass plugin, built against the pinned Hyprland,
   - shaders and native helpers,
   - shell/ and the scripts,
   - a package lock with the tested versions.

   It is signed with minisign on the main laptop, as now.
4. **Updater switch.**
   - Each version goes into its own folder, for example
     ~/.local/share/tessera/versions/<ver>. A `current` pointer is swapped
     atomically, and undo swaps it back.
   - The git checkout stays for developers only. Normal installs need no git.
   - Today's "code" conflicts (user edits to Tessera files) only apply in the
     dev mode.
5. **Tested package versions.**
   - The root step installs the exact versions of the packages that break
     Tessera when mismatched: the Hyprland family (the plugin ABI) and
     Quickshell. These come from the Arch Linux Archive when needed and are
     held (IgnorePkg) until the next Tessera version.
   - Everything else is a minimum version only, so security updates keep
     flowing.
   - Confirm this split with the user before building: they chose "Tessera +
     its system packages".
6. **No download yet.** Software Update says the version is still being
   published and offers Wait (the default) or Build now (gently).

## Status (22 Nov, branch worktree-agent-a4f2b2cb7bd6d7baf)

Steps 3–6 are written and tested locally; nothing is published, nothing ran
on server1, nothing is pushed. docs/releases.md ("Bundles") describes it.

- **3. The bundle: done (code).** `tools/tessera-bundle.py pack` makes one
  bundle per variant from a built checkout; `tessera-release` makes one per
  commit (shaders, native helpers, the glass plugin against server1's
  Hyprland, both programs from `built/<tree>`, the package lock from
  server1's pacman) as a `READY` set of type `bundle`; `publish-release`
  checks, signs (and signs the rolling release's pointer,
  `tessera-latest.bundle.json`) and publishes it. Also in the builder (step
  2): beta with the `local` profile, stable and tags `release`; mold when
  installed; both variants at once only with MemTotal ≥ 24 GB.
- **4. Updater switch: done (code).** `versions/<id>`, `current` and
  `previous` links, everything outside (bin/, tessera-glass.so,
  ~/.config/quickshell/tessera, `source`) a link through `current`; undo
  and rollback are the rename back. Modes: `bundle`, `git` (setup.sh's
  checkout with nothing of its own: moves to bundles on its next update,
  what was there kept in backups/before-bundles), `dev` (git, as before).
  tessera-update.py check/stage/finish/peek/info, update-run.sh,
  launch-shell.sh, update-source.py, `tessera update`, install.sh run from
  a bundle all handle it.
- **5. Tested package versions: done (code).** The lock's held family
  (Hyprland's and Quickshell) exact, from the repositories or the Arch
  Linux Archive, held with IgnorePkg (/etc/pacman.d/tessera-hold.conf,
  included from pacman.conf's [options]); the rest minimums (a system update
  first when one isn't met). `tessera-root update --lock`, through
  `sysupdate.py run --lock`, one password, during the background stage.
- **6. No download yet: done (code).** Software Update: "Tessera X has no
  download yet" with Wait for the download (default) and Build now (a
  gentle build here into an unsigned bundle; a git install builds through
  git as before). `tessera update` asks on a terminal, waits unattended.

Left to do:

- Build the Rust side for real (only unit tests ran) and try the whole flow
  in the sandbox, then on system1: a published beta bundle is needed first.
- server1: install mold, Hyprland headers, qt6-shadertools, cmake; copy the
  new tessera-release (`install.sh`); check its bundles' locks name Arch's
  versions (server1 should be plain Arch, or the archive won't have them).
- First publish from the laptop (publish-release with the bundle support).
- `uninstall.sh` should take out the Include line and the hold file.
- The System row still counts held packages among its updates (pacman
  skips them); hide them there.
- A fresh install straight to a bundle (setup.sh downloading the bundle
  instead of cloning) isn't written: a fresh install is still git, and
  moves to bundles with its first update.

## Size and shipping

- Steps 1–2 are small and remove the stutter for everyone.
- Steps 3–6 are a larger round.
- Ship to system1 first, then to beta after the user says yes.
