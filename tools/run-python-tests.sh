#!/usr/bin/env bash
# All Python tests, each file on its own, in a throwaway home: no test can
# reach a real install (17 Nov: one ran native-install for real and started a
# full release build into server1's own ~/.local/share/tessera/bin). Then the
# Node tests (tests/*.test.cjs: the polkit rules, the bug Worker…).
#   tools/run-python-tests.sh [tests/test_x.py …]
#   TESSERA_TEST_QS        Tessera's own Quickshell (default: yours), for the QML load tests
#   TESSERA_UI_TEST_BINARY a tessera-ui build, for the tests of the real binary
set -uo pipefail
cd "$(dirname "$0")/.."
H=$(mktemp -d "${TMPDIR:-/tmp}/tessera-pytests.XXXXXX") || { echo "no temporary home" >&2; exit 2; }
[[ -n $H && -d $H ]] || { echo "no temporary home" >&2; exit 2; }
trap 'rm -rf "$H"' EXIT
mkdir -p "$H/.config" "$H/.local/share" "$H/.local/state" "$H/.cache" "$H/run" "$H/root" || exit 2
chmod 700 "$H/run"
# (the QML load tests need Tessera's Quickshell, which lives in the real home)
QSDIR=${TESSERA_TEST_QS:-${XDG_DATA_HOME:-$HOME/.local/share}/tessera/quickshell}
files=("$@"); [[ ${#files[@]} -gt 0 ]] || files=(tests/test_*.py)
fail=0 ran=0 skipped=0
declare -A why=()
for f in "${files[@]}"; do
    # Never the live session (conftest.py does this under pytest only; audit
    # #64): no display, compositor or session bus, a runtime folder and a
    # system root of its own, Qt offscreen
    out=$(env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u DISPLAY -u HYPRLAND_INSTANCE_SIGNATURE -u DBUS_SESSION_BUS_ADDRESS \
        -u TESSERA_ROOT -u TESSERA_TTY -u TESSERA_SANDBOX -u TESSERA_USER_DIR -u TESSERA_SHELL -u TESSERA_QS_DEBUG \
        HOME="$H" XDG_CONFIG_HOME="$H/.config" XDG_DATA_HOME="$H/.local/share" XDG_STATE_HOME="$H/.local/state" \
        XDG_CACHE_HOME="$H/.cache" XDG_RUNTIME_DIR="$H/run" TESSERA_SYSTEM_ROOT="$H/root" QT_QPA_PLATFORM=offscreen \
        TESSERA_TEST_QS="$QSDIR" TESSERA_NATIVE_BUILDER=/bin/false timeout 900 python3 -m unittest -v "$f" 2>&1)
    n=$(grep -oE '^Ran [0-9]+' <<<"$out" | grep -oE '[0-9]+'); ran=$((ran + ${n:-0}))
    # (skips counted and their reasons kept: a green run with the signature,
    # PAM or QML checks skipped said nothing; audit #62)
    while IFS= read -r reason; do
        [[ -n $reason ]] || continue
        skipped=$((skipped + 1))
        why["$reason"]=$(( ${why["$reason"]:-0} + 1 ))
    done < <(sed -nE "s/.* \.\.\. skipped '(.*)'$/\1/p" <<<"$out")
    # (unittest's verdict line; warnings may print after it)
    if grep -q '^FAILED' <<<"$out" || ! grep -qE '^OK( \(.*\))?$' <<<"$out"; then fail=$((fail + 1)); echo "FAIL $f"; grep -E '^(FAIL|ERROR):' <<<"$out" | sed 's/^/  /'; fi
done
[[ -z "$(ls -A "$H/.local/share/tessera/bin" 2>/dev/null)" ]] || echo "note: a test installed into the throwaway home's tessera/bin"
echo "python tests: $ran ran, $skipped skipped, $fail file(s) failing"
if (( skipped )); then
    for r in "${!why[@]}"; do printf '  skipped %d: %s\n' "${why[$r]}" "$r"; done | sort
fi
# The Node tests, when a whole run was asked for (audit #58)
if (( $# == 0 )); then
    if command -v node >/dev/null; then
        if node --test tests/*.test.cjs >"$H/node.log" 2>&1; then
            echo "node tests: $(grep -oE '^(ℹ|#) pass [0-9]+' "$H/node.log" | grep -oE '[0-9]+' || echo ?) passed"
        else
            fail=$((fail + 1))
            echo "FAIL node tests (node --test tests/*.test.cjs):"
            grep -E '^not ok|^✖|(ℹ|#) fail' "$H/node.log" | sed 's/^/  /'
        fi
    else
        echo "NOTE: node isn't installed: the Node tests (tests/*.test.cjs: polkit rules, bug Worker) did NOT run"
    fi
fi
exit $(( fail > 0 ))
