#!/usr/bin/env bash
# Which of Tessera's packages (tools/packages.sh) this machine is missing, and
# which ones Tessera dropped are still installed. Read-only: installing and
# removing is done by the shell (sysupdate.py's root part, after Tessera's
# password dialog), from Settings → Software Update and before a Tessera update.
#
#   tessera-packages.sh status [git ref]
#     prints one JSON line: {"missing":[…],"unavailable":[…],"dropped":[…],
#     "reasons":{"pkg":"why"}} (why: the comment above its group in packages.sh)
#     With a ref (e.g. origin/main), the list is the one in that version, so
#     an update's new packages can be installed before it's applied. A folder
#     instead of a ref: the version in it (a bundle's, tools/tessera-bundle.py).
#   missing      in PACKAGES, not installed (nor provided by something
#                installed), and in the repositories
#   unavailable  in PACKAGES, not installed, not in the repositories (left
#                out, like install.sh does)
#   dropped      in DROPPED and still installed
#   outdated     installed, but older than that version's MINIMUM or
#                compat.json allows, and the repositories have one that fits:
#                also in missing (pacman -S --needed upgrades it), so whatever
#                installs the missing ones upgrades these in the same run
#   blocked      needed, and the repositories have no version that fits:
#                tools/tessera_deps.py says why, and the update doesn't apply
set -uo pipefail
ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
cmd="${1:-status}" ref="${2:-}"
[[ "$cmd" == status ]] || { echo "usage: tessera-packages.sh status [git ref]" >&2; exit 2; }
command -v pacman >/dev/null || { echo '{"missing":[],"unavailable":[],"dropped":[],"error":"pacman isn'"'"'t installed"}'; exit 0; }

list="$(mktemp)" compat="$(mktemp)"
trap 'rm -f "$list" "$compat"' EXIT
if [[ -n "$ref" && -d "$ref" && -f "$ref/tools/packages.sh" ]]; then
    cp "$ref/tools/packages.sh" "$list"
    cp "$ref/compat.json" "$compat" 2>/dev/null
elif [[ -n "$ref" && ! -d "$ref" ]] && git -C "$ROOT" show "$ref:tools/packages.sh" >"$list" 2>/dev/null; then
    git -C "$ROOT" show "$ref:compat.json" >"$compat" 2>/dev/null
else
    cp "$ROOT/tools/packages.sh" "$list"
    cp "$ROOT/compat.json" "$compat" 2>/dev/null
fi
# The lists are read as text, never run: the file may come from a version
# that isn't installed yet (an update's), and running it would execute
# whatever it says before you've agreed to anything. A NAME=( … ) block, one
# or more names per line, # comments; only well-formed package names (a
# letter, digit or @_+ first, so nothing can pass as a pacman option).
names() { # array-name file
    awk -v want="$1" '
        !on && $0 ~ "^[[:space:]]*" want "=\\(" {
            on = 1
            sub("^[[:space:]]*" want "=\\(", "")
        }
        on {
            line = $0
            sub(/#.*/, "", line)
            done = (line ~ /\)/)
            sub(/\).*/, "", line)
            n = split(line, f, /[[:space:]]+/)
            for (i = 1; i <= n; i++) if (f[i] != "") print f[i]
            if (done) on = 0
        }' "$2" | grep -E '^[a-z0-9@_+][a-z0-9@._+-]*$'
}
mapfile -t PACKAGES < <(names PACKAGES "$list")
mapfile -t DROPPED < <(names DROPPED "$list")

json() { # name values...
    local out="" p
    for p in "${@:2}"; do out+="${out:+,}\"$p\""; done
    printf '"%s":[%s]' "$1" "$out"
}

# pacman -T: the ones not satisfied by anything installed (names or provides)
mapfile -t notmet < <([[ ${#PACKAGES[@]} -gt 0 ]] && pacman -T -- "${PACKAGES[@]}" 2>/dev/null)
missing=() unavailable=()
for p in "${notmet[@]}"; do
    [[ -z "$p" ]] && continue
    if pacman -Si -- "$p" >/dev/null 2>&1 || pacman -Sg -- "$p" >/dev/null 2>&1; then missing+=("$p"); else unavailable+=("$p"); fi
done
# Not power-profiles-daemon next to another power manager (TLP, auto-cpufreq,
# tuned): they'd fight over the same settings, and TLP's package conflicts
# with it (installing it would remove TLP). tuned-ppd stands in for it.
if [[ " ${missing[*]} " == *" power-profiles-daemon "* ]]; then
    for p in tlp auto-cpufreq tuned-ppd tuned; do
        if pacman -Qq -- "$p" >/dev/null 2>&1 || systemctl is-active --quiet "$p" 2>/dev/null; then
            kept=()
            for m in "${missing[@]}"; do [[ "$m" == power-profiles-daemon ]] || kept+=("$m"); done
            missing=("${kept[@]}")
            break
        fi
    done
fi
# Installed but too old for that version (MINIMUM, compat.json): upgraded
# with the missing ones; and the ones no version in the repositories fits
outdated=() blocked=()
if command -v python3 >/dev/null && [[ -f "$ROOT/tools/tessera_deps.py" ]]; then
    req="$(python3 -I "$ROOT/tools/tessera_deps.py" requirements --list "$list" --compat "$compat" 2>/dev/null)"
    mapfile -t outdated < <(python3 -I -c 'import json, sys; [print(n) for n in json.loads(sys.argv[1] or "{}").get("outdated", [])]' "$req" 2>/dev/null)
    mapfile -t blocked < <(python3 -I -c 'import json, sys; [print(n) for n in json.loads(sys.argv[1] or "{}").get("blocked", [])]' "$req" 2>/dev/null)
    for p in "${outdated[@]}"; do
        [[ -n "$p" && " ${missing[*]} " != *" $p "* ]] && missing+=("$p")
    done
fi
dropped=()
for p in "${DROPPED[@]}"; do
    pacman -Qq -- "$p" >/dev/null 2>&1 && dropped+=("$p")
done
# Why each missing one is needed: the "# comment" line above its group
reasons=""
if [[ ${#missing[@]} -gt 0 ]]; then
    want=" ${missing[*]} "
    while IFS=$'\t' read -r p why; do
        # (as JSON text: control characters as spaces, then \ and " escaped:
        # a comment with a path or a tab never breaks the whole answer)
        why=${why//[[:cntrl:]]/ }
        why=${why//\\/\\\\}
        why=${why//\"/\\\"}
        [[ "$want" == *" $p "* ]] && reasons+="${reasons:+,}\"$p\":\"$why\""
    done < <(awk '/^PACKAGES=\(/ { on = 1; next } on && /^\)/ { on = 0 }
        on && /^[[:space:]]*#/ { c = $0; sub(/^[[:space:]]*#[[:space:]]*/, "", c); next }
        on { for (i = 1; i <= NF; i++) { if ($i ~ /^#/) break; printf "%s\t%s\n", $i, c } }' "$list")
fi
printf '{%s,%s,%s,%s,%s,"reasons":{%s}}\n' "$(json missing "${missing[@]}")" "$(json unavailable "${unavailable[@]}")" "$(json dropped "${dropped[@]}")" \
    "$(json outdated "${outdated[@]}")" "$(json blocked "${blocked[@]}")" "$reasons"
