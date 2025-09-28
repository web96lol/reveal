# Auto-Report Implementation Status

This note compares the current Rust implementation to the legacy C# tool and explains what would happen if the auto-report feature were removed again.

## Does the current code work?

The backend only runs the automation when the stored configuration enables `auto_report`, matching the guard the C# script performed manually. It fetches the post-game stats, skips the current player and any friends, and submits the same seven report categories that the C# prototype hard-coded, all through Riot's official LCU REST endpoints.【F:src-tauri/src/end_of_game.rs†L7-L128】 When the report succeeds the log mirrors the legacy console output, and any failures are surfaced without crashing the Tauri process.【F:src-tauri/src/end_of_game.rs†L105-L127】

The workflow is wired into the same gameflow dispatcher that already handles auto-open and auto-accept. As soon as the client enters a post-game phase, the handler clones the latest configuration and REST client and offloads the reporting logic onto the async runtime so it does not block websocket processing.【F:src-tauri/src/state.rs†L20-L70】 Because the configuration is managed state in `main.rs`, the toggle in the UI and the backend logic stay in sync just like the existing booleans.【F:src-tauri/src/main.rs†L49-L109】【F:src/lib/components/tool.svelte†L72-L107】【F:src/lib/config.ts†L1-L15】

## What if we revert to the old code?

Removing the Rust auto-report module would simply restore the previous behaviour—auto-open and auto-accept would keep working because their data flow and configuration fields are untouched. The only regression would be the loss of end-of-game automation (no more reports or friend/current-player filtering), effectively matching the state before the feature landed. Since the new code does not patch the shared `shaco` client or alter the ready-check/champ-select handlers, reverting it would not introduce instability beyond disabling this single toggle.

## Parity with the C# version

The Rust implementation follows the same milestones as the C# sample:

- Watches for the end-of-game phase before attempting to pull the stats payload.【F:src-tauri/src/state.rs†L62-L70】
- Reads `/lol-end-of-game/v1/eog-stats-block` to discover the players in the match, along with their IDs, names, and champion picks.【F:src-tauri/src/end_of_game.rs†L60-L124】
- Filters out the local account and any summoners on the friend list, just like the `foundFriends` short-circuit in the original console app.【F:src-tauri/src/end_of_game.rs†L69-L127】
- Posts the hard-coded category array to `/lol-player-report-sender/v1/end-of-game-reports` so every teammate receives the full set of reasons the prototype sent.【F:src-tauri/src/end_of_game.rs†L7-L115】

Because it reuses the same managed configuration, REST wrapper, and websocket session that power the other toggles, the Rust port remains stable and ready to extend—no additional library changes were required.
