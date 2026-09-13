# Knot iOS development

## Prerequisites

- macOS with Xcode, an iOS simulator runtime, command-line tools, and an accepted Xcode license.
- Rust targets `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and optionally `x86_64-apple-ios`.
- Tauri CLI 2.10.1 or compatible Tauri 2 release, XcodeGen, and CocoaPods.
- An Apple ID configured in Xcode for physical-device signing.

The generated project lives at `apps/desktop/src-tauri/gen/apple/desktop.xcodeproj`. No development team, certificate, provisioning profile, or Xcode user data is committed.

## Initialize or regenerate

From `apps/desktop`:

```sh
cargo tauri ios init
```

Regeneration can replace project settings. Reapply repository changes represented in `gen/apple/project.yml`, including `SystemConfiguration.framework` and the build-phase PATH, then regenerate with XcodeGen when needed.

## Simulator

```sh
cd apps/desktop
cargo tauri ios build --debug --target aarch64-sim --ci
cargo tauri ios dev "iPhone 17"
```

List installed simulators with:

```sh
xcrun simctl list devices available
```

The first command links a complete bundled simulator application; a Rust target-only `cargo check` does not catch missing Apple frameworks.

## Physical-device development

A Tauri development build loads the frontend from the Mac. Keep the Mac and phone on the same network, then start the Tauri CLI coordinator and leave it running:

```sh
cd apps/desktop
cargo tauri ios dev --open --host <MAC_LAN_IP>
```

In Xcode:

1. Select the `desktop_iOS` target.
2. Enable automatic signing and choose a development team.
3. Select the connected iPhone.
4. Run the app.

Do not open Xcode directly for this workflow. The generated Rust build phase communicates with the coordinator started by `cargo tauri ios dev`. The build phase prepends `$HOME/.cargo/bin`, `/opt/homebrew/bin`, and `/usr/local/bin` so Xcode can find Cargo and the Tauri CLI.

The Vite configuration uses `TAURI_DEV_HOST` for LAN binding and HMR. iOS may require **Settings → Privacy & Security → Local Network → Knot**. A development build can show a blank screen after a cellular cold start because the Mac dev server is unreachable.

## Standalone signed package

Use a bundled build for restart, persistence, and cellular testing:

```sh
cd apps/desktop
cargo tauri ios build --debug --target aarch64 --ci --export-method debugging
```

Tauri prints the exported path. With Tauri CLI 2.10, the usual output is:

```text
apps/desktop/src-tauri/gen/apple/build/arm64/Knot.ipa
```

Install through Xcode's Devices and Simulators window, or install the archived application:

```sh
xcrun devicectl device install app \
  --device <COREDEVICE_ID> \
  apps/desktop/src-tauri/gen/apple/build/desktop_iOS.xcarchive/Products/Applications/Knot.app
```

A personal team supports development installs only and has provisioning/device limits. The app may require explicit trust under **Settings → General → VPN & Device Management**.

## Storage

On iOS:

- Private identity and model settings use Tauri's application-data directory.
- The default board uses `Documents/Default Board`.
- The default board is created automatically.
- Native watcher failure is nonfatal; iOS uses a polling watcher.

## Manual acceptance

1. Launch the standalone app and create a card in Default Board.
2. Force-quit and reopen; verify the card remains.
3. Disable Wi-Fi and cold-start on cellular; verify bundled UI and card persistence.
4. Run the relay diagnostic and require `hello_acknowledged: true` plus `path: relay`.
5. Pair desktop and phone against the same stable board ID.
6. Verify bidirectional create/edit/delete sync and repeat sync idempotence.
7. Make independent offline edits and verify merge or explicit conflict behavior.
8. Test reconnect/resume separately. Background synchronization is not currently claimed.
