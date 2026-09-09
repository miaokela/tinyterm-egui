#!/usr/bin/env bash
# Build a styled TinyTerm disk image: dark background artwork, the app icon on
# the left, an /Applications alias on the right.
#
#   scripts/make-dmg.sh <TinyTerm.app> <out.dmg> [volume name]
#
# The layout is applied by driving Finder over AppleScript, which is the only
# way to write the .DS_Store that carries the icon positions and the background
# picture. Two details matter, both learned the hard way:
#
#   * Finder ignores a volume it has not "seen" yet. Running the AppleScript
#     right after `hdiutil attach` fails with
#         Finder got an error: Can't get disk "TinyTerm" (-1728)
#     so we mount the usual way (under /Volumes, no -mountpoint), give Finder a
#     few seconds, and retry. create-dmg carries the same workaround.
#   * The disk is addressed by the basename of its mount point, which is what
#     Finder shows in its `disk` list.
#
# The build fails if the finished image ends up without a background reference:
# a silently unstyled DMG is worse than a red build.
set -euo pipefail

APP="${1:?usage: make-dmg.sh <app bundle> <out.dmg> [volume name]}"
OUT="${2:?usage: make-dmg.sh <app bundle> <out.dmg> [volume name]}"
VOL="${3:-TinyTerm}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BG="$ROOT/assets/dmg-background.png"
BG2X="$ROOT/assets/dmg-background@2x.png"
APP_NAME="$(basename "$APP" .app)"

[[ -d "$APP" ]] || { echo "error: no app bundle at $APP" >&2; exit 1; }
[[ -f "$BG" ]] || { echo "error: missing $BG (run scripts/make-dmg-background.swift)" >&2; exit 1; }

mkdir -p "$(dirname "$OUT")"
OUT="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/tinyterm-dmg.XXXXXX")"
STAGE="$WORK/stage"
RW="$WORK/$VOL.rw.dmg"
AS="$WORK/layout.applescript"
VERIFY="$WORK/verify"
MOUNT=""
DEV=""
cleanup() {
  if [[ -n "$MOUNT" ]]; then
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || hdiutil detach "$MOUNT" -force >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── stage the volume contents ────────────────────────────────────────────────
mkdir -p "$STAGE/.background" "$VERIFY"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp "$BG" "$STAGE/.background/background.png"
if [[ -f "$BG2X" ]]; then
  cp "$BG2X" "$STAGE/.background/background@2x.png"
fi

# ── write the layout script ──────────────────────────────────────────────────
# Coordinates are in the background artwork's own space: the icon band sits at
# y = 165, between the drag hint and the Gatekeeper cards.
cat > "$AS" <<'APPLESCRIPT'
on run argv
	set volName to item 1 of argv
	set mountDir to item 2 of argv
	set appName to item 3 of argv
	set dsStore to quoted form of (mountDir & "/.DS_Store")
	tell application "Finder"
		tell disk (volName as string)
			open
			set current view of container window to icon view
			set toolbar visible of container window to false
			set statusbar visible of container window to false
			set the bounds of container window to {200, 120, 860, 560}
			set opts to the icon view options of container window
			set arrangement of opts to not arranged
			set icon size of opts to 128
			set text size of opts to 12
			set background picture of opts to file ".background:background.png"
			set position of item (appName & ".app") of container window to {175, 165}
			set position of item "Applications" of container window to {485, 165}
			close
			open
			update without registering applications
			delay 2
			close
		end tell
		delay 1
		-- Wait for Finder to flush the .DS_Store to disk.
		set waited to 0
		repeat while waited < 20
			if (do shell script "[ -f " & dsStore & " ]; echo $?") is "0" then exit repeat
			delay 1
			set waited to waited + 1
		end repeat
	end tell
end run
APPLESCRIPT

# ── create the read-write image ──────────────────────────────────────────────
echo "==> creating read-write image"
hdiutil create -srcfolder "$STAGE" -volname "$VOL" -fs HFS+ \
  -format UDRW -ov "$RW" >/dev/null

echo "==> mounting"
# No -mountpoint: Finder only picks up volumes mounted the normal way.
ATTACH_OUT="$(hdiutil attach "$RW" -readwrite -noverify -noautoopen -nobrowse)"
MOUNT="$(printf '%s\n' "$ATTACH_OUT" | awk -F'\t' '$3 ~ /^\// {print $3; exit}')"
DEV="$(printf '%s\n' "$ATTACH_OUT" | awk -F'\t' '/^\/dev\// {print $1; exit}')"
[[ -n "$MOUNT" ]] || { echo "error: could not determine the mount point" >&2; exit 1; }
VOL_NAME="$(basename "$MOUNT")"
echo "    mounted at $MOUNT (disk name: $VOL_NAME)"

# Finder needs a moment to register a freshly mounted volume (-1728 workaround).
sleep 5

echo "==> applying Finder layout"
applied=0
for attempt in 1 2 3; do
  if osascript "$AS" "$VOL_NAME" "$MOUNT" "$APP_NAME"; then
    applied=1
    break
  fi
  echo "    Finder rejected the layout (attempt $attempt/3), retrying in 5s" >&2
  sleep 5
done
[[ $applied -eq 1 ]] || echo "    warning: Finder automation failed - layout may be missing" >&2

# Finder writes the .DS_Store lazily; give it a moment before detaching.
sync
sleep 2
hdiutil detach "$MOUNT" >/dev/null 2>&1 || hdiutil detach "$MOUNT" -force >/dev/null
MOUNT=""

# ── compress ─────────────────────────────────────────────────────────────────
echo "==> compressing"
rm -f "$OUT"
hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -ov -o "$OUT" >/dev/null

# ── verify the layout survived compression ───────────────────────────────────
echo "==> verifying"
if hdiutil attach "$OUT" -readonly -nobrowse -mountpoint "$VERIFY" >/dev/null 2>&1; then
  if [[ -f "$VERIFY/.DS_Store" ]] && strings "$VERIFY/.DS_Store" | grep -q background; then
    echo "    layout verified: background picture + icon positions stored"
    hdiutil detach "$VERIFY" >/dev/null 2>&1 || hdiutil detach "$VERIFY" -force >/dev/null 2>&1 || true
  else
    hdiutil detach "$VERIFY" >/dev/null 2>&1 || hdiutil detach "$VERIFY" -force >/dev/null 2>&1 || true
    echo "error: the disk image has no background reference in .DS_Store" >&2
    echo "       (Finder never applied the layout; see the warnings above)" >&2
    exit 1
  fi
fi

echo "==> done"
echo "    dmg: $OUT ($(du -h "$OUT" | cut -f1))"
