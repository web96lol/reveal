# End-of-game auto-report integration outline

## 1. Lessons from the C# `end_game` sample
- The console app blocks on two tasks: phase polling and League-client liveness. Each phase check pulls `/lol-gameflow/v1/session`, then sleeps with calm, sparse console dots until `PreEndOfGame`/`EndOfGame` arrive.
- `using System.Text;` plus `Console.OutputEncoding = Encoding.UTF8;` ensures champion and summoner names render cleanly—our Rust logs should maintain UTF-8 output (which they already do) so the tone matches without extra ceremony.
- When those phases hit, it fetches `/lol-end-of-game/v1/eog-stats-block`, dedupes by `gameId`, populates `currentPlayerId`, and enumerates every player, skipping the local summoner and anyone in the cached friend ID list from `/lol-chat/v1/friends` before POSTing `/lol-player-report-sender/v1/end-of-game-reports`.
- Output is intentionally subdued—"has been reported" or "is a friend, ignoring"—and errors simply abort the current attempt. This cadence, together with the bias filters, is the behavioral contract we mirror inside Tauri.

## 2. How the existing Rust modules already collaborate
- `src-tauri/src/main.rs` is the conductor: it sets up the managed mutex state (`LCU`, `ManagedDodgeState`, `AppConfig`), seeds config defaults, and runs the reconnect loop that builds paired Shaco `RESTClient`s plus the websocket client. The same serene `println!`s—"Waiting for League Client to open...", "Connected to League Client!", and the FINALIZATION dodge notices—set the baseline we must not deviate from.【F:src-tauri/src/main.rs†L34-L248】
- `state.rs` is the phase dispatcher. It clones both REST clients, reads the config mutex, and chooses actions for each phase (`ReadyCheck` auto-accept, champ-select handoff). Its async spawns and dedupe logic are the model for new `PreEndOfGame`/`EndOfGame` handling.【F:src-tauri/src/state.rs†L1-L86】
- `champ_select.rs` showcases the desired modularity: `handle_champ_select_start` gathers lobby + region + summoner data, emits a Tauri event, conditionally calls `utils::display_champ_select`, and forwards analytics—all with a single entry point fed by `state.rs`.【F:src-tauri/src/champ_select.rs†L93-L159】
- `lobby.rs`, `region.rs`, and `summoner.rs` define the typed data snapshots that champ select and analytics consume. Reusing these ensures our end-of-game feature speaks the same structured language when reporting teammates or deriving friend bias.【F:src-tauri/src/lobby.rs†L4-L91】【F:src-tauri/src/region.rs†L3-L28】【F:src-tauri/src/summoner.rs†L4-L73】
- `analytics.rs` wraps the async fire-and-forget telemetry hook, giving us a place to record that auto-report executed without blocking the UX.【F:src-tauri/src/analytics.rs†L4-L38】
- `commands.rs` exposes Tauri commands for config I/O, dodge toggles, and utility actions. Extending it keeps `auto_report` user-controlled exactly like `auto_open`/`auto_accept`.【F:src-tauri/src/commands.rs†L8-L158】
- `utils.rs` centralizes helper output (multi-search URL building, calm printlns). Its style guides the concise status messages our end-of-game handler should emit.【F:src-tauri/src/utils.rs†L60-L121】

Together, these modules already provide the pattern the user celebrates: guarded state in `main.rs`, phase routing in `state.rs`, single-purpose handlers (champ-select today, end-of-game next), and UI/analytics hooks that stay blissfully decoupled.

## 3. Incremental implementation plan for auto-report

### 3.1 Shared state and config (mirroring `auto_open` / `auto_accept`)
1. Extend `Config` in `main.rs` with `auto_report: bool` (default `false`) and optionally `report_categories: Vec<String>`; regenerate the bootstrap JSON file accordingly.【F:src-tauri/src/main.rs†L47-L94】
2. Add a `ManagedReportState(Mutex<ReportState>)` alongside `ManagedDodgeState`. `ReportState` caches `last_reported_game: Option<u64>`, `friends: Vec<u64>`, and `local_summoner: Option<u64>` so repeated phases don’t spam requests.
3. Update `commands.rs` so `get_config`/`set_config` round-trip the new fields, and introduce a `toggle_auto_report(game_id: Option<u64>)` command if UI needs immediate enable/disable feedback (mirrors `enable_dodge`).【F:src-tauri/src/commands.rs†L34-L132】
4. Surface the switch in the Svelte UI next to the existing automation toggles. When disabled, the backend should emit the same single-line style already used elsewhere, e.g. `println!("Auto-report disabled; skipping end-of-game handler")`, so logs continue to blend with the core messages.【F:src-tauri/src/main.rs†L105-L247】

### 3.2 Phase wiring in `state.rs` and websocket handling
1. In `state::handle_client_state`, add matches for `"PreEndOfGame"` and `"EndOfGame"`. If `config.auto_report` is true, clone the Shaco clients plus the `ManagedReportState`, then spawn `end_of_game::handle_end_of_game_start` just like champ-select’s delayed task. Otherwise emit the single calm skip log.【F:src-tauri/src/state.rs†L24-L86】
2. Subscribe to `/lol-end-of-game/v1/eog-stats-block` in `main.rs::handle_ws_message`. Whenever the websocket pushes new payloads, forward them to the same handler, but guard with the cached `last_reported_game` so both pathways dedupe gracefully.【F:src-tauri/src/main.rs†L180-L248】

### 3.3 Implementing `end_of_game::handle_end_of_game_start`
1. Create `src-tauri/src/end_of_game.rs` with a public `handle_end_of_game_start(app_handle: &AppHandle, app_client: RESTClient, remoting_client: RESTClient)` that accepts owned clones—matching `champ_select` style.
2. Inside, lock `ManagedReportState`. If the incoming `game_id` matches `last_reported_game`, unlock and return quietly.
3. Ensure friends and local summoner caches are hydrated:
   - If `friends` is empty or stale, GET `/lol-chat/v1/friends` via the non-remoting client and collect `summonerId`s (bias rule from C#).
   - If `local_summoner` is `None`, reuse `summoner::get_current_summoner` (already used by champ select) instead of re-parsing JSON manually.【F:src-tauri/src/summoner.rs†L12-L52】
4. Fetch `/lol-end-of-game/v1/eog-stats-block` via the remoting client. Parse into a new `EndOfGameStats` struct that mirrors the JSON (teams, players, `gameId`). Store `last_reported_game` once parsing succeeds.
5. Iterate players, skip when `player.summoner_id == local` or in `friends`. For each candidate, POST `/lol-player-report-sender/v1/end-of-game-reports` with categories drawn from config (falling back to the C# list). Log one serene line per result, matching the terse dodge messages already in `main.rs`, so console output stays indistinguishable from the existing websocket flow.【F:src-tauri/src/main.rs†L204-L247】
6. Collect the outcomes and emit a Tauri event (e.g., `"auto_report_completed"`) carrying the success/failure map so the frontend button can reflect state without polling.
7. Optionally forward a summary to `analytics::send_analytics_event` (include counts, categories used, game ID) while swallowing errors.

### 3.4 Frontend considerations
- Update the Svelte settings panel to show the new toggle and, if categories become configurable, a checklist component. Disable the "Auto report" button unless `lcu_state_update` says the client is connected, mirroring how multi-search buttons behave.
- On receiving `auto_report_completed`, surface a toast or log entry with the same calm tone: "Auto-reported 3 players; skipped 2 friends." Respect the repository style (no spammy popups).

## 4. Testing & observability checklist
- **Unit-style tests**: Mock the REST client with a local server (or inject a trait) to confirm `handle_end_of_game_start` filters friends/local player and dedupes by `game_id`.
- **Manual smoke**: With `auto_report` off, complete a game—verify only the skip log prints. Enable the toggle, finish another match, and confirm reports fire once, the UI event triggers, and re-entering `EndOfGame` without a new match does not duplicate calls.
- **Telemetry**: Inspect analytics payloads for the new event and ensure they remain optional (errors logged once, no panics).
- **Console tone**: Audit logs for single-line status messages; they should resemble existing champ-select output to keep the "bliss" aesthetic intact.

Following this plan keeps every `.rs` module in harmony, reuses Shaco clients exactly as champ select already does, and extends automation with the same serene, toggle-driven ergonomics that the user expects.

## 5. How this plan leans on Reveal’s strengths
- **No new plumbing required** – the paired Shaco `RESTClient`s that `main.rs` already provisions are reused directly, so friend lookups, end-of-game stats, and report submissions all travel through the same trusted TLS client your app already depends on.【F:src-tauri/src/main.rs†L101-L200】【F:src-tauri/src/champ_select.rs†L108-L159】
- **Managed mutex state everywhere** – introducing `ManagedReportState` mirrors `ManagedDodgeState`, keeping the dedupe-by-game-ID rule and toggle checks under the exact same locking discipline that makes the current automation stable.【F:src-tauri/src/main.rs†L34-L82】
- **Phase orchestration stays centralized** – `state::handle_client_state` remains the serene switchboard; the new `PreEndOfGame`/`EndOfGame` branch simply spawns one more async task alongside the existing champ-select path, preserving the “beautiful connection” between websocket updates and feature modules.【F:src-tauri/src/state.rs†L24-L86】
- **UI & analytics reuse** – the Svelte UI keeps calling `get_config`/`set_config`, and analytics piggybacks on the current helper, so no bespoke IPC or logging patterns creep in and the UX stays familiar.【F:src-tauri/src/commands.rs†L34-L158】【F:src-tauri/src/analytics.rs†L4-L38】
- **Calm logging preserved** – adopting the single-line tone from `utils::display_champ_select` and `main.rs` keeps UTF-8 output tidy, just like the C# sample’s console style that inspired this feature.【F:src-tauri/src/utils.rs†L60-L121】【F:src-tauri/src/main.rs†L134-L200】

Because every step reuses these existing pillars, the auto-report flow is an “alike” extension of champ select—grounded in Reveal’s strengths rather than reinventing them.

## 6. Shaco error handling we inherit
- `RESTClient::new` already returns a `Result`; `main.rs` mirrors the project style by unwrapping during startup and only panicking after multiple reconnect attempts, so our handler keeps leaning on those trusted clients without inventing new TLS or auth code.【F:src-tauri/src/main.rs†L117-L168】
- Every REST call we reuse (for example `app_client.get` and the remoting `post` calls) already bubbles up `reqwest::Error`. Existing modules either `unwrap()` when failure is unrecoverable or print a single calm message before returning. The auto-report module will follow that exact pattern: keep the `unwrap()`s where they already exist and translate recoverable failures into one-line logs, preserving the repo’s tone while still benefitting from Shaco’s error propagation.【F:src-tauri/src/champ_select.rs†L123-L150】【F:src-tauri/src/main.rs†L204-L247】
