#!/bin/sh
# Regenerates the app icon (AppIcon) and the app's own copy of it (Logo, for the home and About
# screens) in apps/ios/KeyboardApp/Assets.xcassets, from tools/icon/render.swift.
#
# Downloads Space Grotesk (SIL OFL 1.1) from Google Fonts' repository, pinned to a commit and
# checked by digest, into build/icon/. The font is only drawn with; it is not committed.
set -eu

cd "$(dirname "$0")/../.."
FONT_COMMIT=2861cb7b12f90c0a294a12ed666e381e2211872f
FONT_SHA256=acad6de1fc93436f5c0f1f4137751ef04f1aea3063e7036535970ffcfbd79f72
FONT=build/icon/SpaceGrotesk.ttf
ASSETS=apps/ios/KeyboardApp/Assets.xcassets

mkdir -p build/icon
if ! echo "$FONT_SHA256  $FONT" | shasum -a 256 -c -s 2>/dev/null; then
    curl -sSLf -o "$FONT" \
        "https://raw.githubusercontent.com/google/fonts/$FONT_COMMIT/ofl/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf"
    echo "$FONT_SHA256  $FONT" | shasum -a 256 -c -s || { echo "Space Grotesk: unexpected digest" >&2; exit 1; }
fi

render() { swift tools/icon/render.swift "$FONT" "$1" "$2" "$3"; }

render light 1024 "$ASSETS/AppIcon.appiconset/AppIcon.png"
render dark 1024 "$ASSETS/AppIcon.appiconset/AppIcon-Dark.png"
render tinted 1024 "$ASSETS/AppIcon.appiconset/AppIcon-Tinted.png"
# The in-app copy: up to 64 pt at 3x.
render light 192 "$ASSETS/Logo.imageset/Logo.png"
render dark 192 "$ASSETS/Logo.imageset/Logo-Dark.png"
echo "icon: rendered into $ASSETS"
