#!/usr/bin/env bash
# Check that a macOS app bundle (and the copy inside its dmg) carries a complete, valid
# ad-hoc signature. Runs in the macOS release job; fails the build if the bundle is unsigned,
# partially signed or modified after signing (the cause of "is damaged and can't be opened").
#
#   scripts/verify-macos-signature.sh <App.app> [<image.dmg>]
#
# spctl is printed for the log only: without a Developer ID and notarization it always says
# "rejected", which is expected for an ad-hoc signature (see docs/macos-gatekeeper.md).
set -euo pipefail

app="${1:?usage: verify-macos-signature.sh <App.app> [<image.dmg>]}"
dmg="${2:-}"

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

check_app "$app"

if [ -n "$dmg" ]; then
  echo "==> the copy inside $dmg"
  mnt="$(mktemp -d)"
  hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$mnt" "$dmg" >/dev/null
  trap 'hdiutil detach "$mnt" -force >/dev/null 2>&1 || true' EXIT
  check_app "$mnt/$(basename "$app")"
  echo "==> codesign -dv $dmg"
  codesign -dv "$dmg" 2>&1 || echo "(the dmg itself carries no signature)"
fi

echo "signature OK"
