# Knot

Knot is a local-first Markdown Kanban desktop app. Boards remain ordinary Markdown files with YAML frontmatter on disk. Cards, immutable revisions, and deterministic merges work locally; peer-to-peer synchronization is provided through the Iroh transport layer.

> **Status:** v0.1 active development. The desktop application is usable for development and local testing, but packaged releases and cross-network synchronization still need real-device validation.

## Features

- Markdown boards stored directly on the filesystem.
- Drag-and-drop and keyboard card reordering.
- Card archiving and restore.
- Obsidian-flavored Markdown rendering and live-preview editing.
- Safe rendering of links, HTML, attachments, code, math, and Mermaid diagrams.
- Local Quick Add with downloadable models, Ollama, or OpenAI-compatible APIs.
- Immutable revisions, tombstones, deterministic three-way merges, and explicit conflicts.
- Manual peer pairing with board authorization and trusted-device state.

## Architecture

The workspace is split into dependency-inward Rust crates and a thin Tauri adapter:

- `kanban-core` — board/card domain types and Markdown/YAML rules.
- `kanban-store` — filesystem-backed board storage and atomic writes.
- `kanban-revisions` — immutable revision DAGs and tombstones.
- `kanban-merge` — deterministic three-way merge and conflicts.
- `kanban-sync` — versioned, transport-independent sync engine.
- `kanban-iroh` — Iroh transport integration.
- `apps/desktop` — TypeScript/Vite UI and Tauri boundary.

The intended flow is `UI → Tauri → sync/store/core`. Core, revisions, and merge do not depend on Tauri or the network transport.

## Requirements

- Rust stable, including `rustfmt` and `clippy`.
- Node.js 20+.
- pnpm 9+.
- Tauri 2 platform prerequisites. See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

For packaged desktop builds, install the platform toolchains required by Tauri. Cross-device testing additionally requires two machines, network access, and a board copy on each machine.

## Development

From the repository root:

```sh
pnpm --dir apps/desktop install
pnpm --dir apps/desktop dev
```

This starts the Vite frontend. To run the complete Tauri shell, install the Tauri CLI and run:

```sh
cd apps/desktop
cargo tauri dev
```

There is no web or server replacement application.

## Checks

Rust checks, excluding the Iroh transport crate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --exclude kanban-iroh -- -D warnings
cargo test --workspace --exclude kanban-iroh
```

Frontend checks:

```sh
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
```

The frontend build runs TypeScript checking before Vite. CI runs frontend checks on macOS and Windows. These checks do not prove packaged-app behavior or cross-network synchronization.

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

Column membership remains the `column` frontmatter field; card ordering remains numeric `position`. Filename segments retain ASCII letters, digits, and underscores, replace other characters with collapsed hyphens, and are limited to 60 characters. Empty segments use `card`. Original titles and IDs remain unchanged in YAML.

Saving a changed title or moving a card updates its filename. Renaming only a column display name does not. Legacy filenames remain readable and migrate when the card is saved. External paths copied before a rename can become stale; links containing the ULID suffix continue to resolve in the app.

Malformed YAML is rejected without rewriting the source file. Unknown frontmatter is preserved. Every mutation creates an immutable revision; deletes are represented by tombstones.

## Markdown and attachments

Cards use an Obsidian-flavored renderer with CommonMark/GFM tables and lists, task lists, highlights, comments, wiki links, heading and block references, embeds, footnotes, callouts, syntax-highlighted code, LaTeX math, and Mermaid diagrams.

Wiki links resolve card IDs or titles in the current board. HTTPS media URLs and board-relative attachments are supported. Local image, audio, video, and PDF attachments are confined to the board folder, including symlink checks, and limited to 50 MB per file. Raw HTML is sanitized; scripts, arbitrary frames, and executable links are removed.

Obsidian vault discovery, plugins such as Dataview/Bases, and attachment synchronization are not provided.

## Pairing and security

Per-install identity is stored outside the board: device ID/name, Iroh EndpointId, trusted peers, and authorized boards. Pairing is manual: exchange an EndpointId and explicitly trust the peer. LAN discovery and QR pairing are not implemented.

The sync handshake validates versioned messages, accepts data only from trusted peers authorized for the board, and confines writes beneath the selected board root. Never share private identity state or blindly trust an EndpointId.

## Real-device validation

Before calling the app production-ready, test packaged builds on macOS and Windows:

1. Create or copy the same test board to both machines. Keep a backup.
2. Pair the devices and authorize the board on both sides.
3. Test card creation, editing, deletion, repeat sync, and reconnect behavior.
4. Test local-network and cross-network connections, recording direct, relay, or failed status.
5. Make independent edits while disconnected and verify deterministic merge or an explicit conflict.
6. Test sleep/wake, disconnect/reconnect, idempotence, and convergence.

## Documentation

See the [TDD build plan](docs/TDD%20Build%20Plan%20%E2%80%94%20P2P%20Markdown%20Kanban.md) for the specification and phase-by-phase test requirements.

## License

MIT. See [LICENSE](LICENSE).
