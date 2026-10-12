# macOS: why the app opened as "damaged", and what the release does now

## What went wrong

Up to 0.2.0 the macOS `.app` was not signed as a bundle. On Apple silicon the Rust linker gives
every executable a minimal *linker-signed* ad-hoc signature, but nothing sealed the bundle: there
was no `Contents/_CodeSignature/CodeResources` binding `Info.plist` and the resources to the
executable. A browser marks a download with the `com.apple.quarantine` attribute; when Gatekeeper
assesses a quarantined app whose signature is missing or does not cover the bundle, it reports
"Loci is damaged and can't be opened. You should move it to the Trash." There is no button past
that dialog.

## What the release does now

`apps/loci/src-tauri/tauri.conf.json` sets `bundle.macOS.signingIdentity` to `"-"`, Tauri's
pseudo-identity for an ad-hoc signature. `tauri build` (which `tauri-action` runs) signs the
executable and then the whole `.app` after every resource is in place, and only then builds the
`.dmg` from the signed bundle. `tauri-action` does not re-sign or modify the bundle afterwards;
it uploads what `tauri build` produced. `hardenedRuntime` is set to `false`: the hardened runtime
only matters for notarization, which needs a Developer ID this project does not have.

The macOS release job then runs `scripts/verify-macos-signature.sh` on the `.app` and on the copy inside
the `.dmg`. It fails the build unless `codesign --verify --deep --strict --verbose=2` passes
("valid on disk", "satisfies its Designated Requirement"), `codesign -dv` reports
`Signature=adhoc` with sealed resources, and `Contents/_CodeSignature/CodeResources` exists. It
also prints `spctl -a -vvv -t install`, which says "rejected": that is the expected answer for
any app without a Developer ID and notarization, so it is logged, not enforced.

## What the user sees

An ad-hoc signature does not make the app trusted; there is no Apple Developer ID behind it and
it is not notarized. It turns "damaged" into the ordinary "Apple could not verify 'Loci' is
free of malware" dialog, which the user can get past once:

- macOS 14 and earlier: right-click (Control-click) the app in Applications, choose **Open**,
  then **Open** again.
- macOS 15 Sequoia and later: Control-click no longer bypasses Gatekeeper. Try to open the app
  once, then go to **System Settings > Privacy & Security**, scroll to Security and click
  **Open Anyway** for Loci.
- Or, in Terminal, clear the quarantine attribute: `xattr -cr /Applications/Loci.app`

## Sources

- Tauri, "macOS Code Signing", ad-hoc signing with `"signingIdentity": "-"` under
  `bundle > macOS` (or `APPLE_SIGNING_IDENTITY`), useful on Apple silicon "where code-signing is
  required for all apps from the Internet": https://v2.tauri.app/distribute/sign/macos/
- Tauri bundler source: with a signing identity set, `tauri build` runs `codesign --force -s <id>`
  on the executables and then the `.app`, before the `.dmg` is created
  (`crates/tauri-bundler/src/bundle/macos/sign.rs`, `crates/tauri-macos-sign/src/keychain.rs`):
  https://github.com/tauri-apps/tauri/tree/dev/crates/tauri-macos-sign
- Apple, "Updates to runtime protection in macOS Sequoia" (6 August 2024): Control-click no
  longer overrides Gatekeeper; users approve the app in System Settings > Privacy & Security:
  https://developer.apple.com/news/?id=saqachfa
- Apple, "Safely open apps on your Mac" (Open Anyway in Privacy & Security):
  https://support.apple.com/en-us/102445
- Apple, macOS Big Sur 11 release notes: native arm64 code must be signed to run; the linker
  applies an ad-hoc signature by default:
  https://developer.apple.com/documentation/macos-release-notes/macos-big-sur-11_0_1-universal-apps-release-notes
- `man codesign` (`--sign -` is ad-hoc signing; `--verify --deep --strict`) and `man spctl`.
