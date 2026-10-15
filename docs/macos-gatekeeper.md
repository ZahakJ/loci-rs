# macOS: opening an unnotarised app

The macOS build is ad-hoc signed but not notarised. The first launch shows "Apple could not
verify 'Loci' is free of malware". To open it:

- macOS 14 and earlier: Control-click Loci in Applications, choose **Open**, then **Open** again.
- macOS 15 and later: open it once, then **System Settings › Privacy & Security › Open Anyway**.
- Or: `xattr -cr /Applications/Loci.app`

Version 0.2.0 was not signed as a bundle and macOS reported it as "damaged". Use 0.2.1 or later.

## Release signing

`apps/loci/src-tauri/tauri.conf.json` sets `bundle.macOS.signingIdentity` to `"-"` (ad-hoc).
`tauri build` signs the executable, then the whole `.app`, then builds the `.dmg`.
`hardenedRuntime` is `false`; it only matters for notarisation.

The release job mounts the `.dmg` and runs `scripts/verify-macos-signature.sh` on the `.app`
inside. It fails unless `codesign --verify --deep --strict` passes, `codesign -dv` reports
`Signature=adhoc` with sealed resources, and `Contents/_CodeSignature/CodeResources` exists.
`spctl -a -t install` reports "rejected" for any app without a Developer ID; it is logged, not
enforced.

## References

- Tauri, macOS code signing: https://v2.tauri.app/distribute/sign/macos/
- Apple, runtime protection in macOS Sequoia: https://developer.apple.com/news/?id=saqachfa
- Apple, Safely open apps on your Mac: https://support.apple.com/en-us/102445
- `man codesign`, `man spctl`
