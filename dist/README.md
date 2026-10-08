# Packaging

## Linux (implemented)

```sh
scripts/package-linux.sh            # release build (thin LTO, stripped)
PROFILE=debug scripts/package-linux.sh   # fast smoke package
```

Produces `target/package/zeren-<version>-linux-<arch>.tar.gz` containing:

- `zeren` — the binary (headed by default; `zeren headless` runs the engine alone)
- `zeren.desktop` — XDG desktop entry template (`Exec=zeren` for packagers;
  the installers rewrite `Exec`, `TryExec`, and `Icon` to absolute paths under
  `~/.zeren/app/current`, since `~/.local/bin` is often not on a desktop
  session's `PATH`)
- `zeren.png` — 1024×1024 Zeren app icon
- `install.sh` — installs into `~/.zeren/app/<version>` behind a `current`
  symlink (the curl installer's layout, which the in-app updater manages),
  links `~/.local/bin/zeren` to it, and writes the desktop entry and icon under
  `$XDG_DATA_HOME` (default `~/.local/share`). The curl installer does the same
  from the extracted tarball; `scripts/test-linux-desktop-entry.sh` checks both

The release profile in the root `Cargo.toml` sets `lto = "thin"` and
`strip = "symbols"` for distribution builds.

## macOS

```sh
scripts/package-macos.sh    # → target/package/zeren-<version>-macos-<arch>.dmg
```

Builds the release binary, assembles `Zeren.app` (Info.plist + icns), ad-hoc
signs it (set `CODESIGN_IDENTITY` for a real Developer ID), and wraps it in a
dmg. The auto-update tarball retains an internal `Zeren.app` path so older
installed builds can update into Zeren. CI runs this on tags
(`.github/workflows/release.yml`). The manual steps it automates, for reference
(run on a macOS host — gpui needs Metal; no cross-build from Linux):

1. Build the universal (or per-arch) binary:
   ```sh
   cargo build --release -p zeren --target aarch64-apple-darwin
   cargo build --release -p zeren --target x86_64-apple-darwin
   lipo -create -output zeren \
     target/aarch64-apple-darwin/release/zeren \
     target/x86_64-apple-darwin/release/zeren
   ```
2. Assemble the bundle:
   ```sh
   mkdir -p Zeren.app/Contents/{MacOS,Resources}
   cp zeren Zeren.app/Contents/MacOS/zeren
   sed "s/__VERSION__/$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')/" \
     dist/macos/Info.plist > Zeren.app/Contents/Info.plist
   ```
3. Icon: generate `zeren.icns` from `dist/macos/icon-1024.png` (the macOS-shaped
   variant of the artwork — squircle mask, margins, and shadow pre-baked, since
   `sips` can't apply an alpha mask) and place it at
   `Zeren.app/Contents/Resources/zeren.icns`:
   ```sh
   mkdir zeren.iconset && sips -z 256 256 dist/macos/icon-1024.png --out zeren.iconset/icon_256x256.png
   iconutil -c icns zeren.iconset -o Zeren.app/Contents/Resources/zeren.icns
   ```
4. Sign + notarize (required for distribution):
   ```sh
   codesign --deep --force --options runtime --sign "Developer ID Application: …" Zeren.app
   xcrun notarytool submit Zeren.zip --keychain-profile … --wait
   xcrun stapler staple Zeren.app
   ```
5. Ship as a `.dmg` (`hdiutil create -volname Zeren -srcfolder Zeren.app -ov -format UDZO Zeren.dmg`).

## Windows

```powershell
./scripts/package-windows.ps1 -ReleasesUrl https://github.com/femboypuppy/zeren/releases/latest/download
```

Produces, under `target/package/`:

- `zeren-<version>-windows-<arch>-setup.exe` — the per-user installer built
  from `dist/windows/zeren.iss` with Inno Setup 6
- `zeren-<version>-windows-<arch>.zip` — the portable package
- `zeren-<version>-windows-<arch>.exe` — the bare executable the in-app
  updater downloads

The installer and the zip both carry `zeren-update.json`, the marker that lets
the app update itself in place. CI runs `scripts/test-windows-installer.ps1`
against the setup on every Windows build.
