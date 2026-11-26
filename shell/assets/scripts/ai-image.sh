#!/usr/bin/env bash
# Pictures for the AI sidebar: each one turned upright and at most 1568 px on
# its long side (what models read; more only costs upload), as a JPEG in the
# runtime folder, owner-only. Prints one line per picture:
#   <file>\t<base64>     or     !\t<name>   (not a picture, or too big)
#   ai-image.sh FILE…         ai-image.sh --clipboard   (exit 3: no picture there)
set -uo pipefail
umask 077
dir="${XDG_RUNTIME_DIR:-/tmp}/tessera-ai"
mkdir -p "$dir"
im=$(command -v magick || command -v convert) || { echo "!	ImageMagick isn't installed" ; exit 2; }
# (copies from earlier days: the chat that showed them is long sent; a
# detached picture or a deleted chat's go at once, from the sidebar)
find "$dir" -maxdepth 1 \( -name 'pic-*.jpg' -o -name 'paste-*' \) -mmin +1440 -delete 2>/dev/null

# The decoder from the file's first bytes, from a short list of picture
# formats: ImageMagick would otherwise follow the content into SVG, MVG,
# PostScript or PDF (and their delegates), which can pull in local files
coder() {
    local sig
    sig=$(head -c 12 -- "$1" 2>/dev/null | od -An -tx1 | tr -d ' \n')
    case $sig in
        ffd8ff*) echo jpeg ;;
        89504e470d0a1a0a*) echo png ;;
        47494638*) echo gif ;;
        52494646????????57454250*) echo webp ;;
        424d*) echo bmp ;;
        49492a00*|4d4d002a*) echo tiff ;;
        ????????66747970*)
            case ${sig:16:8} in
                61766966|61766973) echo avif ;;                          # avif avis
                68656963|68656978|68657663|6d696631|6d736631) echo heic ;; # heic heix hevc mif1 msf1
                *) return 1 ;;
            esac ;;
        *) return 1 ;;
    esac
}

one() { # source, name shown when it fails
    local out="$dir/pic-$(date +%s%N).jpg"
    if [[ ! -f "$1" || $(stat -c %s -- "$1") -gt 67108864 ]]; then
        printf '!\t%s\n' "$2"
        return
    fi
    local c
    if ! c=$(coder "$1"); then
        printf '!\t%s\n' "$2"
        return
    fi
    # ([0]: the first frame of a GIF or a multi-page file)
    if "$im" "$c:$1[0]" -auto-orient -resize '1568x1568>' -strip -quality 85 "jpg:$out" 2>/dev/null; then
        printf '%s\t%s\n' "$out" "$(base64 -w0 -- "$out")"
    else
        rm -f -- "$out"
        printf '!\t%s\n' "$2"
    fi
}

if [[ "${1:-}" == --clipboard ]]; then
    type=$(wl-paste --list-types 2>/dev/null | grep -m1 '^image/') || exit 3
    tmp="$dir/paste-$(date +%s%N)"
    wl-paste --type "$type" >"$tmp" 2>/dev/null || exit 3
    one "$tmp" "the pasted picture"
    rm -f -- "$tmp"
    exit 0
fi
for f in "$@"; do
    one "$f" "$(basename -- "$f")"
done
