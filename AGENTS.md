# Repository Agent Guide

> **Scope:** Entire repository.
>
> **Instruction:** Keep analytics removed from the backend and consult the detailed operational notes below when working on any auto/open/dodge functionality. This document is reference material—there are no additional style constraints beyond what is written here.

## Auto Accept Button Flow

1. **Frontend toggle** (`src/lib/components/tool.svelte`)
   - Switch updates `config.autoAccept` and calls `updateConfig(config)`.
2. **Config update helper** (`src/lib/config.ts`)
   - Invokes the Tauri command `set_config` with the serialized config.
3. **Backend command** (`src-tauri/src/commands.rs::set_config`)
   - Logs the new config.
   - Locks `AppConfig` mutex, replaces in-memory config, serializes to JSON, and writes to `config.json`.
   - Uses `unwrap()` for path resolution and file I/O, so failures panic.
4. **Ready check handler** (`src-tauri/src/state.rs::handle_client_state`)
   - On `"ReadyCheck"`, locks config, checks `auto_accept`, sleeps for `accept_delay - 1000` ms, and POSTs to `/lol-matchmaking/v1/ready-check/accept` using `remoting_client`.
   - Response is ignored; failures are silent.

## Auto Open Multi Flow

1. **Frontend toggle** (`tool.svelte`)
   - Switch updates `config.autoOpen` then persists via `updateConfig`.
2. **Gameflow listener** (`state.rs::handle_client_state`)
   - On `"ChampSelect"`, clones handles/clients and spawns a task that waits 5 seconds before reading config and calling `handle_champ_select_start`.
3. **Champ select start handler** (`src-tauri/src/champ_select.rs::handle_champ_select_start`)
   - Fetches lobby via `/chat/v5/participants`.
   - Fetches region via `/riotclient/region-locale`.
   - Emits `champ_select_started` with lobby payload.
   - If `config.auto_open` is true:
     - Normalizes region (`SG2` → `SG`).
     - Calls `display_champ_select` with lobby, region, and `config.multi_provider`.
   - **Analytics hook has been removed**; do not reintroduce telemetry.
4. **Link generation & browser launch** (`src-tauri/src/utils.rs`)
   - `display_champ_select` builds a readable team string, prints it, selects provider-specific builder (`create_opgg_link`, `create_deeplol_link`, `create_ugg_link`, or `create_tracker_link`), and opens default browser via `open::that`.
   - On failure to open, logs "Failed to open link in browser".

## Manual Dodge Button Flow

1. **Frontend button** (`tool.svelte`)
   - `invoke("dodge")` on click.
2. **Backend command** (`commands.rs::dodge`)
   - Locks `LCU` state, builds `RESTClient` with remoting auth, logs "Attempting to quit champ select...".
   - POSTs to `/lol-login/v1/session/invoke?destination=lcdsServiceProxy&method=call&args=["","teambuilder-draft","quitV2",""]`.
   - Uses `unwrap()` on client creation and response.

## Manual Open Multi Link Flow

1. **Frontend button** (`tool.svelte`)
   - `invoke("open_opgg_link")` on click.
2. **Backend command** (`commands.rs::open_opgg_link`)
   - Locks `LCU` state, creates app-level REST client, locks config.
   - Fetches lobby (`/chat/v5/participants`) and region (`/riotclient/region-locale`).
   - Normalizes region (`SG2` → `SG`).
   - Calls `display_champ_select` with lobby and selected provider.

## Last-Second Dodge Flow

1. **Toggle command** (`commands.rs::enable_dodge`)
   - Locks `LCU` and `ManagedDodgeState`.
   - If already armed, clears `enabled` and returns.
   - Otherwise fetches `/lol-champ-select/v1/session`, stores `game_id` in `enabled`.
2. **WebSocket handler** (`main.rs::handle_ws_message`)
   - Parses champ select session events; on `FINALIZATION` phase:
     - Locks `ManagedDodgeState`, skips if already dodged or not armed for current game.
     - Records `last_dodge`, drops lock, and spawns async task sleeping for `adjusted_time_left_in_phase` before POSTing the dodge endpoint.
     - Logs spawn and execution messages; `unwrap()` on POST.

## Managed State Overview

- `LCU(Mutex<LCUState>)`: connection status and client info.
- `AppConfig(Mutex<Config>)`: persisted configuration (auto flags, delay, multi provider).
- `ManagedDodgeState(Mutex<DodgeState>)`: last-second dodge bookkeeping.
- All state locks are acquired with `.lock().await`; lock order is consistent (LCU → Config) to avoid deadlocks.

## WebSocket Lifecycle (`main.rs`)

- Connects to League Client, subscribes to `/lol-gameflow/v1/gameflow-phase` and `/lol-champ-select/v1/session`.
- On connection, sets `LCU` state, emits `lcu_state_update`.
- Processes each message via `handle_ws_message`, which routes to `state::handle_client_state` or last-second dodge logic, logging unhandled messages.

## Error Handling Notes

- Extensive use of `unwrap()` in commands and state handlers will panic on failures (I/O, HTTP errors, serialization).
- Browser launch failures are logged, not bubbled.
- Auto-accept POST ignores the response entirely.

## Frontend Signals & State

- Listens for `lcu_state_update`, `client_state_update`, and `champ_select_started` events to update UI.
- `RevealCount` component polls `https://hyperboost.gg/api/reveal/stats` every minute. Failure renders "Failed to fetch reveal stats" and logs to console.
- UI exposes toggles for auto open/accept, provider selection, manual open, manual dodge, and last-second dodge.

## Network Touchpoints (Post-Analytics Removal)

- League Client REST endpoints (local).
- Multi-search providers: `op.gg`, `deeplol.gg`, `u.gg`, `tracker.gg` (via system browser).
- Reveal stats endpoint: `https://hyperboost.gg/api/reveal/stats` (frontend polling).

## Concurrency Considerations

- Auto-open spawn delay may outlive champ select sessions, potentially hitting stale lobbies.
- Last-second dodge task cannot be cancelled once spawned; ensure UX communicates this.
- Config and LCU locks are held briefly to avoid contention.

## Logging Summary

- Config updates, client state transitions, dodge attempts, team compositions, connection status, and certain errors emit `println!` statements for debugging.

