# IrohMD

IrohMD is a local-first, peer-to-peer Markdown Kanban desktop application. The board remains ordinary Markdown with YAML frontmatter on disk; immutable revisions and deterministic merges synchronize through Iroh. No central server or database is required.

> **Status:** active development (v0.1). The desktop shell and protocol are not yet a release artifact. Treat the real-device checklist below as a test plan, not a compatibility guarantee.

## Architecture

The workspace is split into dependency-inward Rust crates and a thin Tauri adapter:

- kanban-core — board/card domain types and Markdown/YAML rules.
- kanban-store — filesystem-backed board storage and safe atomic writes.
- kanban-revisions — immutable revision DAGs and tombstones.
- kanban-merge — deterministic three-way merge and explicit conflicts.
- kanban-sync — versioned sync protocol and transport-independent engine.
- kanban-iroh — Iroh transport integration.
- apps/desktop — TypeScript/Vite UI; src-tauri is the thin Tauri boundary.

The intended flow is UI → Tauri → sync/store/core; core, revisions, and merge stay independent of Tauri and Iroh. Markdown files are the live state, so local editing continues to work without networking.

## Prerequisites

- Rust stable (with rustfmt and clippy components), Cargo, and a compiler toolchain supported by Tauri 2.
- Node.js 20+ and pnpm 9+ (the frontend lockfile is apps/desktop/pnpm-lock.yaml).
- Tauri 2 platform prerequisites for your OS: Xcode Command Line Tools and WebKit on macOS; Microsoft C++ Build Tools, WebView2, and the Windows SDK on Windows. Follow the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for current details.
- A real-device sync test additionally needs one macOS and one Windows machine, Iroh network access, and a board copied to each machine.

## Development

From the repository root:

    pnpm --dir apps/desktop install
    pnpm --dir apps/desktop dev

The last command starts the Vite frontend. The complete desktop shell requires the Tauri CLI and platform prerequisites; run cargo tauri dev from apps/desktop when those are installed. There is deliberately no replacement web/server application.

## Test and build

Rust workspace checks:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

Desktop frontend checks:

    pnpm --dir apps/desktop test
    pnpm --dir apps/desktop build

The frontend build runs TypeScript checking before Vite. CI runs these commands on both macOS and Windows; platform-specific packaging is not implied by the frontend build.

## Board format

A board is a directory containing board.md and a cards/ directory. Cards are files such as cards/fix-auth.md; do **not** use column directories. Column membership is the column frontmatter field and ordering is numeric position.

    board/
      board.md
      cards/
        fix-auth.md

Every card has a stable ULID, deterministic YAML serialization, and sync metadata (revision, parents, content hash, and update time). Unknown frontmatter is preserved. Malformed YAML is rejected without rewriting the source file. Every mutation creates an immutable revision; deletes are represented by tombstones.

## Pairing and security model

Per-install identity is stored outside the board: device ULID/name, Iroh EndpointId, trusted peers, and authorized boards. V1 pairing is intentionally manual: copy an EndpointId, paste it on the other device, and explicitly trust the peer. LAN discovery and QR pairing are not implemented.

The sync handshake validates versioned messages, accepts data only from trusted peers authorized for that board, and confines all writes beneath the selected board root. Untrusted peers and unapproved boards are rejected. Transport may be direct or relayed; relay status does not change authorization. Never share private identity state or blindly trust an EndpointId.

## Real-device test checklist (honest status)

This is a manual integration test, not something CI can perform. Use packaged development builds only after the Tauri shell is available, and record the app commit, OS versions, EndpointIds (redacted), and connection result (direct, relay, or failed).

1. On macOS and Windows, install the prerequisites and build/run the desktop shell.
2. Create or copy the same test board to both machines; keep a backup because this test mutates files.
3. Pair manually by exchanging EndpointIds and approving the peer and board on each side.
4. On the same personal Wi-Fi, create a card on macOS and verify it arrives on Windows; edit, delete, and repeat sync to check idempotence.
5. Repeat across different networks, including a Windows work network where inbound connectivity may be restricted. Record direct/relay/failure.
6. Disconnect both peers, edit the same card independently, reconnect, and verify deterministic merge or an explicit conflict—never silent overwrite.
7. Exercise disconnect/reconnect and sleep/wake, then verify no duplicate revisions and convergence on both devices.

A successful local frontend build or in-memory Rust test does **not** prove cross-network, relay, firewall, sleep/wake, or packaged-app behaviour. Report failures with network topology and connection status.

## Build plan

See [the TDD build plan](docs/TDD%20Build%20Plan%20%E2%80%94%20P2P%20Markdown%20Kanban.md) for the specification and phase-by-phase test requirements.

## License

MIT; see [LICENSE](LICENSE).

### Moving cards

Drag a card above or below another card; neighboring cards slide aside to open a
gap for the pulsing drop box. The gap stays stable while you move within it.
Reduced-motion settings disable the pulse and card animation. Hold near the board's top or bottom edge to
scroll while dragging. Release outside the board, or press Escape, to cancel.

With a card focused, use **Alt+↑/↓** to reorder within its column or **Alt+←/→** to
move between columns. Enter or Space opens the card. Moving retains keyboard focus;
opening or editing never changes board scroll or card order.

### Obsidian Markdown in cards

Cards and the editor share an Obsidian-flavored renderer. The editor is a single editable live-preview surface: click rendered text to
edit its Markdown in place. Syntax appears in the active block and renders again
when the cursor moves away. Undo, selection, and clipboard operations preserve
the underlying Markdown. Reference links, heading anchors, and footnote numbering
resolve across the whole editor document, not separately within each visual block.
Unsaved drafts survive UI redraws and board refreshes in memory; close the editor
to save them to disk. Drafts do not survive an application restart.
Reading styles follow the Baseline theme's restrained Inter typography, rounded
callouts and code blocks, neutral surfaces, and light and dark syntax colors.

Supported syntax includes CommonMark/GFM tables and lists, task lists (including
Baseline alternate task markers), highlights, comments, wiki links and aliases,
heading and block references, note/section/block embeds, reference and inline
footnotes, collapsible callouts, syntax-highlighted code, LaTeX math (KaTeX), and
Mermaid diagrams. Code spans and fences preserve literal Markdown syntax.

Wiki links resolve card IDs or titles within the current board. Media embeds can
use HTTPS URLs or board-relative files, for example `![[assets/sketch.png|320]]`.
Local image, audio, video, and PDF attachments are restricted to the board folder
(including symlink resolution) and 50 MB per file. Missing notes/files remain
visible as unresolved references. Obsidian vault discovery, plugins such as
Dataview/Bases, and attachment synchronization are not provided by this renderer.
Raw HTML is sanitized; scripts, arbitrary frames, and executable links are removed.

References: [Obsidian syntax](https://obsidian.md/help/obsidian-flavored-markdown),
[Baseline theme](https://github.com/aaaaalexis/obsidian-baseline).
