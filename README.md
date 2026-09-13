# Knot

Knot is a local-first Markdown Kanban app built with Rust, Tauri, and TypeScript. Boards remain ordinary Markdown files with YAML frontmatter. Cards, immutable revisions, and deterministic merges work locally; peer-to-peer synchronization uses Iroh.

> **Status:** v0.1 active development. Desktop development, iOS simulator packaging, physical iPhone installation, sandbox persistence, and foreground relay connectivity are validated. Bidirectional board-sync acceptance, background execution, and packaged macOS/Windows releases still need broader testing.

## Features

- Markdown boards stored directly on the filesystem.
- Drag-and-drop and keyboard card reordering.
- Card archiving and restore.
- Obsidian-flavored Markdown rendering and live-preview editing.
- Safe rendering of links, HTML, attachments, code, math, and Mermaid diagrams.
- Quick Add through optional embedded GGUF inference, Ollama, or OpenAI-compatible APIs.
- Immutable revisions, tombstones, deterministic three-way merges, and explicit conflicts.
- Manual peer pairing with board authorization and trusted-device state.
- Iroh direct/relay transport plus an isolated relay diagnostic protocol.
- Desktop and iOS targets from one Tauri application.

## Architecture

The workspace is split into dependency-inward Rust crates and a thin Tauri adapter:

- `knot-core` — board/card domain types and Markdown/YAML rules.
- `knot-store` — filesystem-backed board storage and atomic writes.
- `knot-revisions` — immutable revision DAGs and tombstones.
- `knot-merge` — deterministic three-way merge and conflicts.
- `knot-sync` — versioned, transport-independent sync engine.
- `knot-iroh` — Iroh transport and isolated diagnostics.
- `apps/desktop` — TypeScript/Vite UI and Tauri boundary.

The intended dependency flow is `UI → Tauri → sync/store/core`. Core, revisions, and merge do not depend on Tauri or Iroh.

## Requirements

### All platforms

- Rust stable with `rustfmt` and `clippy`.
- Node.js 20+.
- pnpm 9+.
- Tauri CLI 2.10.1 or compatible Tauri 2 release.
- Platform prerequisites from the [Tauri documentation](https://v2.tauri.app/start/prerequisites/).

### iOS

- macOS and Xcode with an iOS simulator runtime.
- Rust targets `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and optionally `x86_64-apple-ios`.
- XcodeGen and CocoaPods as required by Tauri.
- An Apple development team and provisioning profile for physical-device installation.

No development team, certificate, private identity, API key, or provisioning profile is committed.

## Install frontend dependencies

```sh
pnpm --dir apps/desktop install
```

## Desktop development

Run the frontend only:

```sh
pnpm --dir apps/desktop dev
```

Run the complete Tauri app with embedded Local AI disabled:

```sh
cd apps/desktop
cargo tauri dev
```

Embedded GGUF inference is deliberately opt-in because it increases compile time, binary size, and native toolchain requirements:

```sh
cd apps/desktop
cargo tauri dev --features local-ai
```

Ollama and OpenAI-compatible providers remain available when `local-ai` is disabled. Hugging Face search, download, model management, and embedded inference return an actionable disabled-feature error in that build.

## iOS development

The generated Xcode project is committed at:

```text
apps/desktop/src-tauri/gen/apple/desktop.xcodeproj
```

Do not open that project directly for a Tauri development session. The generated Rust build phase expects the Tauri CLI coordinator. Start it from the app directory and leave the command running:

```sh
cd apps/desktop
cargo tauri ios dev --open --host <MAC_LAN_IP>
```

Then select the `desktop_iOS` target, choose a development team under **Signing & Capabilities**, select the connected device, and run from Xcode. The Vite configuration binds to `TAURI_DEV_HOST` only for mobile development; normal desktop development remains localhost-only.

A development build depends on the Mac dev server and can show a blank page after a cellular cold start. Build a standalone signed package for persistence and cellular testing:

```sh
cd apps/desktop
cargo tauri ios build --debug --target aarch64 --ci --export-method debugging
```

Tauri prints the exported package path. With Tauri CLI 2.10, the usual output is:

```text
apps/desktop/src-tauri/gen/apple/build/arm64/Knot.ipa
```

Generated `build/`, `Externals/`, Xcode user data, and signing state are ignored and must not be committed. See [iOS Development](docs/iOS%20Development.md) for simulator, device, signing, and acceptance procedures.

## Local AI build modes

Default/no-feature checks:

```sh
cargo check -p desktop --no-default-features --all-targets
cargo test -p desktop --no-default-features
```

Opt-in Local AI check:

```sh
cargo check -p desktop --features local-ai --all-targets
```

The `quick_add_bench` example requires `local-ai` and is excluded from default builds.

## Checks

Rust:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Frontend:

```sh
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
```

iOS Rust targets:

```sh
cargo check -p desktop --target aarch64-apple-ios-sim --no-default-features
cargo check -p desktop --target aarch64-apple-ios --no-default-features
```

CI also links a complete unsigned iOS simulator application. External relay tests remain opt-in so ordinary CI does not depend on relay availability.

## Board format

A board is a directory containing `board.md` and `cards/`:

```text
board/
├── board.md
└── cards/
    └── backlog-Fix-auth-01ARZ3NDEKTSV4RRFFQ69G5FAV.md
```

Card filenames use:

```text
{column_id}-{sanitized_title}-{ULID}.md
```

Column membership remains the `column` frontmatter field; ordering remains numeric `position`. Filename segments retain ASCII letters, digits, and underscores, replace other characters with collapsed hyphens, and are limited to 60 characters. Empty segments use `card`. Original titles and IDs remain unchanged in YAML.

Saving a changed title or moving a card updates its filename. Renaming only a column display name does not. Legacy filenames remain readable and migrate when the card is saved. Malformed YAML is rejected without rewriting the source file. Unknown frontmatter is preserved. Card mutations create immutable revisions; deletes become tombstones.

Revision metadata lives in `.knot/revisions/` alongside `board.md` and `cards/`.

## Storage

Desktop defaults:

- Unix-like systems: `$HOME/.config/knot`.
- Windows: `Knot` under `%LOCALAPPDATA%` or `%APPDATA%`.
- `KNOT_CONFIG_PATH` can override the desktop install configuration path.

iOS uses sandbox paths resolved through Tauri before commands run:

- Private install identity and model settings: application data directory.
- Default board: `Documents/Default Board`.

The default board is created automatically on mobile. Native watcher failure does not prevent board use; iOS uses a polling watcher fallback. Install configuration contains secret identity material and must remain private.

## Markdown and attachments

Cards use an Obsidian-flavored renderer with CommonMark/GFM tables and lists, task lists, highlights, comments, wiki links, heading and block references, embeds, footnotes, callouts, syntax-highlighted code, LaTeX math, and Mermaid diagrams.

Wiki links resolve card IDs or titles in the current board. HTTPS media URLs and board-relative attachments are supported. Local image, audio, video, and PDF attachments are confined to the board folder, including symlink checks, and limited to 50 MB per file. Raw HTML is sanitized; scripts, arbitrary frames, and executable links are removed.

Obsidian vault discovery, Dataview/Bases plugins, and attachment synchronization are not provided.

## Pairing, sync, and diagnostics

Per-install identity stores device ID/name, Iroh EndpointId, trusted peers, and authorized boards. Pairing is manual: exchange a full endpoint address, trust the peer, and authorize the same stable board ID. LAN discovery and QR pairing are not implemented.

The sync handshake validates versioned messages, accepts data only from trusted peers authorized for the board, and confines writes beneath the selected board root. Never share private identity state or blindly trust an EndpointId.

Settings exposes the full local endpoint address JSON and a remote diagnostic dial input. Diagnostics use the separate `knot-diagnostic/1` ALPN, an ephemeral identity, a nonce-bound HELLO/ACK, bounded frames, and a forced relay-only client. A successful result must report both `hello_acknowledged: true` and `path: relay`.

Run a long-lived diagnostic host:

```sh
cargo run -p knot-iroh --bin knot-peer-echo
```

Dial its printed address from another machine or the app:

```sh
cargo run -p knot-iroh --bin knot-peer-echo -- --dial '<ENDPOINT_ADDRESS_JSON>'
```

See [Remote Diagnostic Task](REMOTE_DIAGNOSTIC_TASK.md) for the full procedure.

## Verified scope and remaining acceptance

Verified during iOS implementation:

- No-feature and Local AI Rust builds.
- Frontend tests and production build.
- Workspace tests and strict Clippy.
- iOS simulator/device Rust compilation.
- Complete iOS simulator linking, installation, launch, and sandbox persistence.
- Signed standalone installation on a physical iPhone.
- Default-board persistence through force-quit and cellular cold start.
- Forced relay-only HELLO/ACK on physical iPhone over cellular.

Still required before production status:

1. Pair desktop and iPhone against a matching board ID and verify bidirectional card create/edit/delete sync.
2. Test disconnect/reconnect, independent edits, deterministic merge, and explicit conflict resolution on devices.
3. Define and test background/resume lifecycle behavior; current validation is foreground-only.
4. Package and test supported macOS and Windows releases.
5. Validate the declared minimum iOS version on older hardware/runtime; current physical and simulator validation used current iOS versions.

## Documentation

- [iOS Development](docs/iOS%20Development.md)
- [TDD Build Plan](docs/TDD%20Build%20Plan%20%E2%80%94%20P2P%20Markdown%20Kanban.md)
- [Remote Diagnostic Task](REMOTE_DIAGNOSTIC_TASK.md)

## License

MIT. See [LICENSE](LICENSE).
