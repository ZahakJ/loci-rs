#!/usr/bin/env bash
# Check that a macOS app bundle carries a complete, valid ad-hoc signature. Runs in the macOS
# release job and fails the build if the bundle is unsigned, partially signed or modified after
# signing (the cause of "is damaged and can't be opened").
#
#   scripts/verify-macos-signature.sh <App.app | image.dmg>...
#
# A .dmg is mounted read-only and every .app at its top level is checked: that copy is what
# users download. spctl is printed for the log only: without a Developer ID and notarization it
# always says "rejected", which is expected for an ad-hoc signature (docs/macos-gatekeeper.md).
set -euo pipefail

[ "$#" -gt 0 ] || { echo "usage: verify-macos-signature.sh <App.app | image.dmg>..." >&2; exit 2; }

check_app() {
  local a="$1"
  echo "==> codesign --verify --deep --strict --verbose=2 $a"
  codesign --verify --deep --strict --verbose=2 "$a"
  echo "==> codesign -dv --verbose=4 $a"
  local info
  info="$(codesign -dv --verbose=4 "$a" 2>&1)"
  echo "$info"
  grep -q '^Signature=adhoc' <<<"$info" || { echo "::error::$a is not ad-hoc signed"; exit 1; }
  grep -q '^Sealed Resources' <<<"$info" || { echo "::error::$a has no sealed resources"; exit 1; }
  test -f "$a/Contents/_CodeSignature/CodeResources" \
    || { echo "::error::$a has no Contents/_CodeSignature/CodeResources"; exit 1; }
  echo "==> spctl -a -vvv -t install $a (informational: ad-hoc is rejected without notarization)"
  spctl -a -vvv -t install "$a" 2>&1 || true
}

check_dmg() {
  local dmg="$1" mnt found=0
  mnt="$(mktemp -d)"
  echo "==> mounting $dmg"
  hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$dmg" >/dev/null
  for a in "$mnt"/*.app; do
    [ -d "$a" ] || continue
    found=1
    check_app "$a" || { hdiutil detach "$mnt" -force >/dev/null 2>&1 || true; exit 1; }
  done
  hdiutil detach "$mnt" -force >/dev/null 2>&1 || true
  [ "$found" = 1 ] || { echo "::error::no .app at the top level of $dmg"; exit 1; }
  echo "==> codesign -dv $dmg"
  codesign -dv "$dmg" 2>&1 || echo "(the dmg itself carries no signature)"
}

for target in "$@"; do
  case "$target" in
    *.dmg) check_dmg "$target" ;;
    *.app) check_app "$target" ;;
    *) echo "::error::not a .app or .dmg: $target"; exit 2 ;;
  esac
done

echo "signature OK"
