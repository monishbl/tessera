# Contributing to Tessera

Thanks for helping. Bug reports, ideas, docs fixes and code are all welcome.

- **A bug:** [open an issue](https://github.com/monishbl/tessera/issues/new/choose) with the output of `tessera doctor`, or use Report a bug in Tessera's Settings.
- **An idea:** open a feature request first, so we can agree on the shape before you write code.
- **A security problem:** don't open an issue; see [SECURITY.md](SECURITY.md).

By taking part you agree to the [code of conduct](CODE_OF_CONDUCT.md).

## Where things are

| Folder | |
|---|---|
| `native/tessera-ui/` | The interface in Rust (Slint): every surface, both looks |
| `native/tessera-events/` | The system watcher in Rust |
| `shell/`, `material/` | The Quickshell (QML) shell, Glass and Material; the fallback when tessera-ui can't start, and the scripts both use (`shell/assets/scripts/`) |
| `hypr/` | Hyprland config (Lua) and the tessera-glass plugin (`hypr/plugin/tessera-glass`) |
| `sandbox/` | The nested compositor used for development |
| `tools/` | Installer, update, build and generator scripts |
| `tests/` | Python tests |
| `site/` | The website, tessera.monishbl.in |
| `docs/` | Documentation; [docs/README.md](docs/README.md) sorts it |

## Develop in the sandbox, never on your live session

All development happens in a nested Hyprland inside a distrobox (`./dev.sh`). It has its own home folder, runtime directory, D-Bus session and a headless 1920×1080 test output.

```sh
./dev.sh create      # one-time: create the container and install the toolchain
./dev.sh up          # start the sandbox (Alt stands in for Super)
./dev.sh reload      # rebuild shaders and restart the shell
./dev.sh ipc panels open controlCenter TESSERA-TEST   # drive the UI
./dev.sh shot shot.png                               # screenshot the test output
./dev.sh hyprctl …   # talk to the sandbox compositor
./dev.sh logs        # the compositor's and shell's logs
./dev.sh down        # stop it
./dev.sh perf [secs] [phases]   # CPU, memory, frames and glass copies per scripted phase
```

To try Tessera with real input on its own console: switch to a text console (Ctrl+Alt+F3) and run `./dev.sh tty`. Your normal desktop keeps running. If it hangs, run `./dev.sh tty-stop` from another console.

What the sandbox shares with your computer, and how it's guarded:

- **Audio:** your PipeWire, so volume changes are real. `TESSERA_HOST_AUDIO=0` turns this off.
- **System D-Bus:** network and Bluetooth toggles can act on real hardware.
- **Guarded:** power actions only notify, the polkit agent isn't registered, and your AI keys are stripped.
- **Processes:** distrobox shares the process list. `./dev.sh down` and `reload` find sandbox processes by their runtime directory, so don't use `pkill`: it can hit your real session.

`TESSERA_NO_BOX=1` runs the same isolated sandbox without distrobox, on a machine that has the packages itself.

## Tests

**Python** (installer, updater, scripts, website and docs checks). Each file runs on its own in a throwaway home, so no test can touch a real install:

```sh
tools/run-python-tests.sh                          # all of them
tools/run-python-tests.sh tests/test_licences.py   # one file
```

Some tests read the README, the docs and the website (`tests/test_trademarks.py`, `tests/test_licences.py`): run them when you change text.

**Rust**, per crate:

```sh
cd native/tessera-ui && cargo test         # the whole workspace
cd native/tessera-ui && cargo test -p tessera-core
cd native/tessera-events && cargo test
```

Test in the default debug profile. Don't use `cargo test --release`: the release profile uses full LTO for every test binary and takes a very long time. For a quick optimised build, `cargo build --profile fast`.

## Pull requests

- Keep a pull request to one change, and say how you checked it (tests run, what you tried in the sandbox).
- Add or update tests for behaviour you change.
- Screenshots help for visual changes. Take them in the sandbox, never of your own desktop, and check they show no Wi-Fi or Bluetooth names, emails or home paths.
- Never commit secrets, API keys or personal data.
- Text people see follows the house style: plain words, short sentences, and features named for what they do (Search, not another product's name; `tests/test_trademarks.py` checks).

## Commit style

One line that says what changed and, in brackets, why, starting with the area:

```
Software Update: the Follow row says this copy's version, not Stable's (it read as Stable's own)
Island spring: half-pixel epsilon, sub-pixel size changes at rest set at once
Site: no "This release" section
```

Write it for someone reading the history a year from now. A longer body is fine when the reason doesn't fit on one line.

## Licence

Tessera is GPL-3.0-or-later. By contributing, you agree your contribution is licensed the same way. Code or assets from elsewhere must have a compatible licence and be listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
