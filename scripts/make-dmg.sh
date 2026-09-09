#!/usr/bin/env bash
# Build a styled TinyTerm disk image: dark background artwork, the app icon on
# the left, an /Applications alias on the right.
#
#   scripts/make-dmg.sh <TinyTerm.app> <out.dmg> [volume name]
#
# The layout is applied by driving Finder over AppleScript, which is the only
# way to write the .DS_Store that carries the icon positions and the background
# picture. If Apple events to Finder are unavailable (locked-down CI, a headless
# session, an unanswered automation prompt) the DMG is still produced, just
# without the artwork - the step reports that instead of failing the build.
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
MOUNT="$WORK/mnt"
RW="$WORK/$VOL.rw.dmg"
AS="$WORK/layout.applescript"
cleanup() {
  hdiutil detach "$MOUNT" >/dev/null 2>&1 || hdiutil detach "$MOUNT" -force >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# ── stage the volume contents ────────────────────────────────────────────────
mkdir -p "$STAGE/.background" "$MOUNT"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp "$BG" "$STAGE/.background/background.png"
[[ -f "$BG2X" ]] && cp "$BG2X" "$STAGE/.background/background@2x.png"

# ── write the layout script ──────────────────────────────────────────────────
# Coordinates are in the background artwork's own space: the icon band sits at
# y = 165, between the drag hint and the Gatekeeper cards.
cat > "$AS" <<'APPLESCRIPT'
on run argv
	set volName to item 1 of argv
	set appName to item 2 of argv
	tell application "Finder"
		tell disk volName
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
	end tell
	delay 1
end run
APPLESCRIPT

# ── create the read-write image ──────────────────────────────────────────────
echo "==> creating read-write image"
hdiutil create -srcfolder "$STAGE" -volname "$VOL" -fs HFS+ \
  -format UDRW -ov "$RW" >/dev/null

echo "==> mounting"
hdiutil attach "$RW" -readwrite -noverify -noautoopen -mountpoint "$MOUNT" >/dev/null

echo "==> applying Finder layout"
if osascript "$AS" "$VOL" "$APP_NAME"; then
  echo "    background + icon positions written"
else
  echo "    warning: Finder automation unavailable - the DMG will be unstyled" >&2
fi

# Finder writes the .DS_Store lazily; give it a moment before detaching.
sync
sleep 1
hdiutil detach "$MOUNT" >/dev/null 2>&1 || hdiutil detach "$MOUNT" -force >/dev/null

# ── compress ─────────────────────────────────────────────────────────────────
echo "==> compressing"
rm -f "$OUT"
hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -ov -o "$OUT" >/dev/null

# ── verify the layout survived compression ───────────────────────────────────
VERIFY="$WORK/verify"
mkdir -p "$VERIFY"
if hdiutil attach "$OUT" -readonly -nobrowse -mountpoint "$VERIFY" >/dev/null 2>&1; then
  if [[ -f "$VERIFY/.DS_Store" ]] && strings "$VERIFY/.DS_Store" | grep -q background; then
    echo "    layout verified: background picture + icon positions stored"
  else
    echo "    warning: .DS_Store carries no background reference - image is unstyled" >&2
  fi
  hdiutil detach "$VERIFY" >/dev/null 2>&1 || hdiutil detach "$VERIFY" -force >/dev/null 2>&1 || true
fi

echo "==> done"
echo "    dmg: $OUT ($(du -h "$OUT" | cut -f1))"
