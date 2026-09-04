# IrohMD UI test report

Date: 2026-09-04  
Test surface: packaged macOS app at `target/release/bundle/macos/IrohMD.app`, exercised via computer use  
Result: native board/card UI exercised against disposable boards in `/Users/fletcher/Documents/IrohMD UI Test`.

Additional launch check: added a valid macOS `.icns` asset and the app/DMG bundles built successfully.

| Feature | Status | Evidence / notes |
|---|---|---|
| Initial empty-board screen | Pass | “Open a board to get started” screen rendered with the expected open-board action. |
| Smart “＋ Open board” action | Fixed (automated) | The title-hover board switcher contains one open action that opens existing boards or initializes directories without a board. |
| Empty-state “Open board” action | Fixed (automated) | Uses the same smart open-or-create flow without requiring users to choose a mode. |
| Open sync settings | Pass | Gear action changed the main view to “Sync settings”. |
| Offline/no-peer status | Pass | Settings rendered `Offline`, `offline`, and `No devices paired`; the local endpoint also populated. |
| Sync current board disabled with no board | Pass | Sync button was visibly disabled as expected. |
| Pair device disabled with no board | Pass | Pair button was visibly disabled as expected. |
| Copy endpoint when endpoint is unavailable | Pass | Empty-state settings surfaced `Local endpoint address is not ready.` |
| Copy populated endpoint | Pass | Native settings had a populated endpoint; clicking Copy endpoint produced no visible error. Clipboard contents were not independently read. |
| Return from sync settings to board | Fixed (automated) | The settings control now toggles back to the board and exposes a context-specific accessible label. |
| Board switching | Fixed (automated) | Persistent tabs were removed; hovering or focusing the board title now animates out the open-board switcher. |
| New card / add card to column | Pass | New card created in Backlog; column-specific add created a card directly in Doing. |
| Edit/save card | Pass | Edited title, description, labels, and column; saved values rendered on the board. |
| Card column selector | Pass | Editor dropdown exposed Backlog, Doing, and Done; selecting Doing persisted and updated counts. |
| Delete card | Fixed (automated) | Delete actions now open an accessible in-app confirmation dialog before invoking deletion. |
| Drag-and-drop card movement | Fixed (automated) | Added pointer-based dragging for packaged WebKit reliability while retaining keyboard movement. |
| Alt + Left/Right card movement | Pass | Focused card moved from Doing to Done with `Alt+Right`; counts and placement updated. |
| Rename board | Fixed (automated) | Clicking the board title now edits it inline and persists through the existing backend command. |
| Rename column | Fixed (automated) | Clicking a column title now edits it inline and persists through the existing backend command. |
| Add and reorder columns | Fixed (automated) | Add column uses an accessible in-app form; columns can be dragged by their headers and their new order persists in board metadata. |
| Manual sync validation | Pass | Sync with no peer surfaced `Pair a peer address before syncing.` |
| Manual sync with peer | Blocked | No peer was available for a real network sync. |
| Pair device | Fixed (automated) | Pairing now collects and submits serialized endpoint addresses through an accessible in-app form. |
| Conflict-resolution actions | Blocked | Requires a backend-generated conflict with two parent revisions. |

## Supporting checks

The frontend regression suite now passes: **21/21 tests**. The production frontend build also passes. The tests cover smart board opening, settings return navigation, in-app rename/add/delete/pair dialogs, and pointer-based card dragging. These checks cover mocked command wiring and rendering logic, but do not replace end-to-end interaction with a native Tauri window.

The Rust release build and macOS app/DMG bundling completed successfully after adding the `.icns` asset.

## Follow-up

The actionable UI failures above were remediated with in-app dialogs, pointer-based drag-and-drop, and explicit return navigation. A packaged-app native retest should confirm the rows marked “Fixed (automated)”. A real peer is still needed for network sync and a backend-generated conflict is still needed for conflict-resolution testing.
