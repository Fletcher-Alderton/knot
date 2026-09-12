# TDD Build Plan — P2P Markdown Kanban

## Goal

Build a Tauri 2 desktop app for macOS and Windows with:

- TypeScript frontend
- Rust core
- Markdown + YAML frontmatter as source of truth
- local-first operation
- Iroh P2P sync
- custom deterministic conflict resolution
- architecture reusable for future iOS/Android support

No central server or database is required.

---

## Core Rules

1. Write tests before implementation.
2. Keep domain logic independent of Tauri and Iroh.
3. Markdown files are the live board state.
4. Every card has a stable ULID.
5. Every change creates an immutable revision.
6. Sync must never silently discard divergent edits.
7. Local editing must work without networking.
8. Peers receiving the same revisions must converge.

---

# Project Structure

```text
crates/
  knot-core/
  knot-store/
  knot-revisions/
  knot-merge/
  knot-sync/
  knot-iroh/

apps/
  desktop/
    src/          # TypeScript UI
    src-tauri/    # thin Tauri adapter
```

Dependencies flow inward:

```text
UI → Tauri → sync/store/core

iroh → sync
store → core
merge → core
revisions → core
```

`knot-core`, `knot-revisions`, and `knot-merge` must not depend on Tauri or Iroh.

---

# File Format

Board:

```text
board/
  board.md
  cards/
    fix-auth.md
    add-search.md
```

Example card:

```md
---
id: 01ABC...

title: Fix authentication
column: doing
position: 2000
due: 2026-09-10
labels:
  - backend

created_at: 2026-09-04T03:20:00Z

sync:
  revision: 01REV...
  parents:
    - 01PREV...
  content_hash: sha256:...
  updated_at: 2026-09-04T04:20:00Z

activity:
  - id: 01EVENT...
    type: created
    at: 2026-09-04T03:20:00Z
---

Fix refresh token handling.
```

Board metadata belongs in `board.md`.

Do not use list directories. Card column membership comes from:

```yaml
column: doing
```

Ordering comes from:

```yaml
position: 2000
```

---

# Phase 1 — Core Markdown Model

Write tests first for:

- parsing valid cards
- parsing `board.md`
- deterministic YAML serialization
- unknown frontmatter preservation
- malformed YAML rejection without rewriting
- ULID validation
- semantic parse → serialize → parse equality
- canonical content hashing

Implement only enough code to pass them.

Required types:

```rust
Board
Card
CardFrontmatter
SyncMetadata
Activity
```

Done when all parsing/serialization tests pass.

---

# Phase 2 — Local Store

Define:

```rust
trait BoardStore
```

with operations for:

```text
open board
list cards
read card
write card
delete card
read board metadata
write board metadata
```

Implement filesystem-backed storage.

Tests:

- create board
- create/read/update/delete card
- atomic writes
- writes cannot escape board root
- duplicate card IDs detected
- external file changes can be re-read correctly

Do not add Tauri yet.

---

# Phase 3 — Revision DAG

Implement immutable revision snapshots.

Each revision contains:

```rust
revision_id
card_id
parents
content_hash
timestamp
snapshot
```

Tests:

```text
A → B
A → B → C

    B
   /
  A
   \
    C
```

Verify:

- parent lookup
- ancestor detection
- descendant detection
- common ancestor lookup
- merge revisions with two parents
- tombstones for deletion

Do not implement networking yet.

---

# Phase 4 — Merge Engine

API:

```rust
merge(base, local, remote) -> MergeResult
```

Write tests before each merge policy.

Policies:

### Scalars

For:

```text
title
due
```

Rules:

- one side changed → changed side wins
- both changed identically → accept
- both changed differently → conflict

### Column

- one side moved → accept move
- both moved differently → newest move activity wins

### Position

- one side changed → accept
- both changed → deterministic winner
- never block sync over ordering alone

### Labels

Perform ancestry-aware set merge.

Example:

```text
base:    [backend, urgent]
local:   [backend]
remote:  [backend, urgent, bug]

result:
[backend, bug]
```

### Activity

Merge by activity ULID.

### Markdown body

Use three-way line merge.

- non-overlapping changes → merge
- overlapping incompatible changes → conflict

Required tests:

- independent field edits
- independent body edits
- conflicting titles
- conflicting body edits
- simultaneous label add/remove
- card moved while body edited
- merge symmetry where applicable
- deterministic repeated merge result

---

# Phase 5 — Convergence Tests

Before networking, simulate two peers entirely in memory.

Example:

```text
Peer A starts at X
Peer B starts at X

A edits title
B edits due date

sync revisions
merge

assert A == B
```

Create scenarios for:

- fast-forward
- divergent edits
- deletes
- card moves
- simultaneous body edits
- repeated sync
- reconnect after offline edits

Primary invariant:

```text
same revision set
→
same resulting semantic board state
```

Do not proceed until this is reliable.

---

# Phase 6 — Tauri Desktop Shell

Create the Tauri 2 app.

Frontend should initially provide only:

```text
Open Board
Create Board
Kanban columns
Cards
Card editor
Add Card
Delete Card
Drag/move Card
```

Tauri commands call the Rust core.

The frontend must never write Markdown directly.

Tests:

- Rust command tests where practical
- UI smoke tests for creating/editing cards
- reopening the application preserves board state

---

# Phase 7 — File Watching

Watch:

```text
board.md
cards/*.md
```

Tests:

1. external editor changes card → app detects change
2. app writes card → watcher does not create duplicate revision
3. malformed external YAML → original file remains untouched
4. external delete → detected
5. external copy with duplicate ID → surfaced as duplicate

All external edits must enter the same revision pipeline as app edits.

---

# Phase 8 — Iroh Transport

Create a transport abstraction:

```rust
trait PeerTransport
```

Implement:

```text
IrohTransport
```

The sync engine must not directly depend on Iroh APIs.

First integration test:

```text
Peer A
  ↕
Iroh
  ↕
Peer B
```

Exchange a simple `hello`.

Then test:

- same-machine peers
- reconnect
- connection loss
- large message rejection
- malformed message rejection

---

# Phase 9 — Sync Protocol

Use versioned messages.

Minimum protocol:

```rust
Hello
BoardSummary
RevisionAvailable
RevisionRequest
RevisionResponse
Ack
Error
```

Every incoming message must be validated.

Handshake:

```text
HELLO
BOARD_SUMMARY
compare revisions
request missing revisions
merge/apply
ACK
```

Tests must run two real sync engines against in-memory stores before filesystem integration.

---

# Phase 10 — Fast-Forward Sync

Implement only simple ancestry first.

Test:

```text
A → B
```

Cases:

- Mac creates card → Windows receives it
- Windows edits card → Mac receives it
- Mac deletes card → Windows receives tombstone
- repeated sync produces no extra revisions

Do not add conflict UI yet.

---

# Phase 11 — Divergent Sync

Enable:

```text
    B
   /
  A
   \
    C
```

Process:

```text
find base A
merge B + C
create D
parents = [B, C]
send D
```

Tests:

- successful automatic merge
- unresolved conflict
- both peers converge on D
- syncing D again is idempotent

---

# Phase 12 — Conflict UI

Represent unresolved conflicts explicitly.

UI options:

```text
Keep Local
Keep Remote
Manual Merge
```

Never overwrite either side before resolution.

Conflict resolution creates a normal merge revision with both parents.

Test:

```text
conflict
→ resolve
→ sync
→ both peers converge
```

---

# Phase 13 — Device Pairing

Store device identity outside the board.

Per-install state:

```text
device ULID
device name
Iroh EndpointId
trusted peers
```

V1 pairing is manual:

```text
copy EndpointId
paste EndpointId
trust peer
```

No LAN discovery or QR codes yet.

Tests:

- untrusted peer rejected
- trusted peer accepted
- peer cannot access unapproved board
- remote data cannot specify arbitrary filesystem paths

---

# Phase 14 — Real Device Test

Run packaged development builds on:

```text
macOS
Windows
```

Test:

1. same personal Wi-Fi
2. different networks
3. Windows work network
4. disconnect/reconnect
5. sleep/wake
6. both devices edit same card offline
7. reconnect and merge

Record whether Iroh connection is:

```text
direct
relay
failed
```

Sync behaviour must be identical for direct and relayed connections.

---

# V1 Non-Goals

Do not build:

```text
mobile UI
accounts
cloud storage
web app
CRDT editor
real-time cursors
attachments
LAN discovery
QR pairing
plugins
Git integration
AI
calendar
notifications
revision-history UI
```

---

# Definition of Done

V1 is complete when:

```text
✓ macOS and Windows apps run locally
✓ board data is ordinary Markdown/YAML
✓ no database is required to use a board
✓ offline editing works
✓ external Markdown editing works
✓ every mutation creates a revision
✓ two peers sync through Iroh
✓ peers can sync across different networks
✓ fast-forward sync works
✓ divergent edits merge deterministically
✓ unresolved edits produce a conflict
✓ conflict resolution preserves both parents
✓ deletes synchronize safely
✓ repeated synchronization is idempotent
✓ same revision set always converges
✓ no remote peer can write outside the board
```

The implementation priority is:

```text
correct file model
→
correct revision model
→
correct merge behaviour
→
local UI
→
network transport
```

Do not optimize networking before merge correctness is proven.
