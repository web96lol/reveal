# Auto Multi, Auto Accept, and Auto Report Data Flow Reference

This document captures the precise runtime paths for the Auto Open Multi ("auto multi"), Auto Accept, and Auto Report features, organized by source file so each `use` statement and event hop is clear.

## `src-tauri/src/commands.rs`

### Imports
- `use crate::{ champ_select::ChampSelectSession, lobby::get_lobby_info, region::RegionInfo, utils::display_champ_select, AppConfig, Config, ManagedDodgeState, LCU };`
  - Pulls the managed state singletons (`LCU`, `AppConfig`, `ManagedDodgeState`) that Tauri shares with commands, the configuration data model (`Config`), and the helpers that load lobby/team data and open a multi-search provider. Removing these breaks both toggles because the commands could not read or persist settings, construct lobby payloads, or track dodge state.
- `use shaco::rest::{LCUClientInfo, RESTClient};`
  - Required for rehydrating REST clients tied to the League Client Update (LCU). Auto multi and auto accept depend on these clients to fetch lobby snapshots and post accept/dodge calls; they cannot function without the `shaco` REST types.
- `use tauri::{AppHandle, Manager};`
  - Enables access to shared state and event emission back to the Svelte front end. Without these traits, none of the commands could touch the managed mutexes or emit UI updates.

### Auto Multi data path
1. The Svelte switches call `set_config` via Tauri, which locks the `AppConfig` mutex, writes the JSON file, and keeps the new `auto_open` flag in memory so later handlers see the change. (`set_config` also relies on `AppHandle` to resolve the config directory.)
2. When a lobby needs a manual trigger, `open_opgg_link` reconstructs an `RESTClient` from the `LCU` state, reads the `AppConfig`, pulls live lobby details with `get_lobby_info`, normalizes the region code, and calls `display_champ_select`, which launches the configured provider.
3. Dodge management (`enable_dodge`) deserializes the current `ChampSelectSession` via the `ChampSelectSession` type brought into scope by the `use crate` statement. That keeps the dodge guard in sync with champ select state but does not directly influence the auto-multi toggle.

### Auto Accept data path
- Auto accept does not run inside `commands.rs`, but `set_config` still needs to persist `auto_accept`, `accept_delay`, and the new `auto_report` toggle. The `use` imports above are what allow that persistence and the subsequent `state.rs` logic to read the updated values.

### Auto Report data path
- Auto report also depends on `set_config` to push the `auto_report` flag down to Rust. The command shares the same imports and disk write as the other toggles, so no additional `use` lines are needed here.

## `src-tauri/src/state.rs`

- `use crate::{ champ_select::handle_champ_select_start, end_game::handle_end_game, AppConfig, ManagedDodgeState };`
  - Adds the auto report runtime pieces (`handle_end_game` and the shared `ManagedDodgeState`) to the existing champ select imports. Without them the new match arm for `PreEndOfGame`/`EndOfGame` would not compile.
- `use shaco::rest::RESTClient;`
  - Needed to hit the `/lol-matchmaking/v1/ready-check/accept` endpoint for auto accept and to read `/lol-gameflow/v1/gameflow-phase` to detect champ select transitions.
- `use tauri::{AppHandle, Manager};`
  - Required for state access and emitting the `client_state_update` event used by the UI.

### Auto Multi data path
1. When `handle_client_state` sees `ChampSelect`, it spawns a delayed task. Once the five second delay completes, it clones the current `Config` snapshot (using the `AppConfig` mutex) and immediately releases the lock before calling `handle_champ_select_start`. That call is only possible because `handle_champ_select_start` was imported from `crate::champ_select`.
2. If the switch is off, `handle_champ_select_start` reads `config.auto_open` from the cloned snapshot and short-circuits before launching a browser window.

### Auto Accept data path
1. The same function listens for `ReadyCheck`. When encountered, it locks `AppConfig`, copies `auto_accept`/`accept_delay`, and releases the mutex immediately.
2. If the flag is true, it waits for `accept_delay - 1000` milliseconds and uses the `RESTClient` import to send a POST request that accepts the ready check automatically.
3. With the toggle off, the branch is skipped entirely and the user must click accept manually. Either way, the `client_state_update` event propagates to the UI.

### Auto Report data path
1. When `handle_client_state` sees `PreEndOfGame` or `EndOfGame`, it spawns a short-lived async task.
2. The task waits 500ms, clones the current `Config`, and exits early if `auto_report` is disabled.
3. It then reads `ManagedDodgeState::last_reported_game`, releases the lock, and calls `handle_end_game`. If the handler returns a new game ID, the task reacquires the dodge mutex and stores the updated value.

## `src-tauri/src/champ_select.rs`

### Imports
- `use crate::{analytics, lobby, region::RegionInfo, summoner, utils::display_champ_select, Config};`
  - Provides access to lobby fetching, region parsing, summoner lookup, analytics reporting, the UI emitter, and the `Config` structure. Each piece is vital: without `display_champ_select` the auto multi launch cannot occur; without `Config` the code cannot check `auto_open`.
- `use serde::{Deserialize, Serialize};`
  - Enables the `ChampSelectSession` data structures to deserialize the LCU websocket payloads. Removing this would stop the dodge guard and UI from understanding champ select state.
- `use shaco::rest::RESTClient;`
  - Used for both lobby (`app_client`) and remoting client calls inside `handle_champ_select_start`.
- `use tauri::{AppHandle, Manager};`
  - Allows the function to emit the `champ_select_started` event back to the front end.

### Auto Multi data path
1. `handle_champ_select_start` pulls the live lobby from the app REST client and resolves the region via `/riotclient/region-locale`.
2. It emits `champ_select_started` so the Svelte component can render participant details.
3. If `config.auto_open` is true, it normalizes the region (`SG2 → SG`) and calls `display_champ_select`, which builds and opens the provider URL. The `use` imports above are all necessary: remove any, and either the function would not compile or it would fail to launch the link.
4. It also fetches the current summoner and fires analytics, which do not depend on the toggles but share the same imports.

### Auto Accept data path
- Auto accept does not run here; this module only supplies champ select context and events. It still matters because the `ChampSelectSession` type (defined here) is the one referenced by `use crate::champ_select::ChampSelectSession` in `commands.rs` and `main.rs`.

### Auto Report data path
- Auto report does not use `champ_select.rs`, but the `ChampSelectSession` struct remains necessary for dodge management and other champ select flows that share the same managed state.

## `src-tauri/src/utils.rs`

### Imports
- `use crate::lobby::{Lobby, Participant};`
  - Gives the helper functions access to player data. All multi-search URL builders depend on this struct shape.
- `use urlencoding::encode;`
  - Used to encode summoner tags safely into URLs.

### Auto Multi data path
1. `display_champ_select` acts on the lobby participants produced by `get_lobby_info` or the websocket event and chooses the proper URL builder (OP.GG, DeepLoL, U.GG, Tracker).
2. The helper returns early if the lobby is empty, prints the participants for logging, constructs the URL, and finally calls `open::that` to launch the browser tab. Auto multi relies on this function—when the toggle is on, `handle_champ_select_start` invokes it automatically; when off, it is only triggered by the `open_opgg_link` command.
3. Because the helpers are pure functions that only depend on `Lobby`/`Participant` and `encode`, they are reusable regardless of the toggle state.

## `src-tauri/src/main.rs`

### Imports
- The module declarations (`mod analytics;`, etc.) and `use` statements bring the Tauri-managed state types (`LCU`, `ManagedDodgeState`, `AppConfig`) plus helper modules into scope. Notable `use crate` lines include:
  - `use crate::champ_select::ChampSelectSession;`
  - `use crate::lobby::Lobby;`
  - `use crate::region::RegionInfo;`
  - `use crate::utils::display_champ_select;`
  These ensure the websocket handler and dodge logic can deserialize champ select events, send lobby snapshots to the UI, and reuse the multi-link helper for analytics or logging.
- `use commands::{ app_ready, dodge, enable_dodge, get_config, get_lcu_info, get_lcu_state, open_opgg_link, set_config };`
  - Registers every command—including `set_config` and `open_opgg_link`, which are the execution entry points for both toggles.
- The additional `use` statements for `shaco`, `tauri`, `tokio`, `serde`, and `futures_util` wire up websocket subscriptions, timers, and event emission.

### Auto Multi data path
1. `main.rs` sets up the managed `LCU`, `ManagedDodgeState`, and `AppConfig` mutexes that `commands.rs` and `state.rs` share.
2. It spins an async task that discovers the League client, builds REST clients, and then launches a websocket listener.
3. When `/lol-gameflow/v1/gameflow-phase` emits `ChampSelect`, the handler routes the payload to `state::handle_client_state`, which eventually calls `handle_champ_select_start` and (if enabled) `display_champ_select`.

### Auto Accept data path
1. The same websocket loop forwards `ReadyCheck` phases to `state::handle_client_state`.
2. Because `set_config` saved `auto_accept` and `accept_delay`, `handle_client_state` can honor them and call the accept endpoint.

### Auto Report data path
1. `main.rs` registers the managed state singletons (`LCU`, `ManagedDodgeState`, and `AppConfig`) that every command and handler uses.
2. When the League client disconnects, it clears `ManagedDodgeState::last_reported_game` so the next session starts fresh.
3. Gameflow websocket events are routed to `state::handle_client_state`, which now fetches friend data on demand and updates `last_reported_game` through the shared dodge state mutex.

## `src-tauri/src/end_game.rs`

- `use crate::{summoner::get_friend_ids, Config};`
  - Gives the module access to the auto report toggle and the configuration schema, and reuses the summoner helpers to pull friend data before sending reports.
- `use serde::{Deserialize, Serialize};`
  - Powers deserialization of the `/lol-end-of-game/v1/eog-stats-block` response and serialization of the reporting payloads.
- `use shaco::rest::RESTClient;`
  - Required for both fetching end-of-game statistics and submitting report POST requests.
- `use std::collections::HashSet;`
  - Stores friend IDs for quick lookups so that friends are never auto-reported.
- `use tauri::{AppHandle, Manager};`
  - Allows the module to emit the `end_game_reports_sent` event back to the front end once processing completes.

### Auto Report data path
1. `handle_end_game` exits immediately when `config.auto_report` is `false`. When enabled, it fetches the friend list with `get_friend_ids`, pulls `/lol-end-of-game/v1/eog-stats-block`, deserializes it, and checks `last_game_id` to avoid duplicate submissions.
2. It then iterates every player via `handle_player_report`, skipping the local summoner and anyone in the friend cache. Each remaining player is reported with the seven hard-coded categories via the `RESTClient` POST call.
3. After iterating, it emits `end_game_reports_sent` so the UI can react if desired and returns the processed game ID to the caller.

## Front-end (`src/lib/components/tool.svelte` and `src/lib/config.ts`)

### Imports
- `import { invoke } from "@tauri-apps/api/tauri";`
  - Lets the UI call the Rust commands. Without it, the switches could not trigger `set_config` or `open_opgg_link`.
- `import { updateConfig, type Config } from "$lib/config";`
  - Supplies the TypeScript shape that mirrors the Rust `Config` and the wrapper that invokes `set_config`. The interface now includes `autoReport`, ensuring the toggle's state travels with the other switches.
- Component imports (`Switch`, `Label`, `Button`, `Select`) shape the UI but do not alter data flow.

### Auto Multi data path
1. When the user toggles **Auto Open Multi**, the `onCheckedChange` handler updates the local `config.autoOpen` flag and calls `updateConfig`, which invokes the `set_config` command. With the switch off, it writes `false`, so the backend guard short-circuits.
2. The “Open Multi Link” button always calls `open_opgg_link`, regardless of toggle state.

### Auto Accept data path
1. The **Auto Accept** switch mirrors the same pattern, updating `config.autoAccept` and persisting the change through `set_config`.
2. No front-end component accepts the queue pop directly—the actual accept is handled in Rust once `ReadyCheck` fires.

### Auto Report data path
1. The **Auto Report** switch toggles `config.autoReport` and invokes `updateConfig`, persisting the choice through Tauri.
2. When the client state transitions to `PreEndOfGame` or `EndOfGame`, the UI shows "Processing Reports..." if auto report is enabled; otherwise it simply displays "Game Ended." Any additional feedback (such as listening for `end_game_reports_sent`) can be layered on later.

## Switch on vs. off summary
- **Auto Open Multi ON:** `set_config` stores `auto_open = true` → `handle_client_state` detects `ChampSelect` → `handle_champ_select_start` emits the event and calls `display_champ_select`, which builds and opens the URL automatically.
- **Auto Open Multi OFF:** The same detection path runs, but `handle_champ_select_start` skips `display_champ_select`. Manual multi-link opening via `open_opgg_link` remains available.
- **Auto Accept ON:** `set_config` stores `auto_accept = true` → `handle_client_state` waits `accept_delay - 1000` ms during `ReadyCheck` → REST POST to `/lol-matchmaking/v1/ready-check/accept` occurs automatically.
- **Auto Accept OFF:** `handle_client_state` ignores the branch, so the player must click Accept.
- **Auto Report ON:** `set_config` stores `auto_report = true` → `handle_client_state` spawns the end-game task → `handle_end_game` fetches the stats block, filters friends, and sequentially posts reports → `end_game_reports_sent` fires when the loop finishes.
- **Auto Report OFF:** `handle_end_game` returns immediately after checking the config flag, so no network calls or events fire.

These flows rely directly on the `use` imports listed above; removing any of the critical crate references would either break compilation or disable the automation paths.
