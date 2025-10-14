# Auto-Open and Auto-Accept Data Flow

This document describes how Reveal's auto-open and auto-accept toggles move from the
Svelte front end through the Tauri backend, and how the resulting state drives
League Client interactions. It also outlines how the same architecture could be
ported to a C# desktop application without modifying the shared `shaco` library.

## Shared Configuration Lifecycle

1. **UI bootstraps configuration** – When `reveal.svelte` mounts it wires up
   listeners for `client_state_update`, `lcu_state_update`, and
   `champ_select_started` before invoking the `app_ready` command. The resolved
   `Config` struct populates local component state so every toggle reflects the
   persisted backend values.【F:src/reveal.svelte†L1-L53】
2. **Config shape parity** – The TypeScript `Config` interface mirrors the Rust
   struct (camelCase vs. snake_case handled by Serde). Both carry the `autoOpen`,
   `autoAccept`, `acceptDelay`, and `multiProvider` fields so the UI can operate
   on a single object graph.【F:src/lib/config.ts†L1-L13】【F:src-tauri/src/main.rs†L41-L55】
3. **Updates flow through `updateConfig`** – Every UI control mutates the shared
   object and immediately calls `updateConfig`, which invokes the `set_config`
   Tauri command with the new struct. The command replaces the managed state and
   persists the JSON back to disk, ensuring that runtime consumers and the next
   launch see the updated values.【F:src/lib/components/tool.svelte†L21-L73】【F:src-tauri/src/commands.rs†L1-L43】
4. **Startup seeding** – During `main` startup the backend ensures a
   `config.json` exists (defaulting `auto_open = true`, `auto_accept = false`,
   `accept_delay = 2000`). It then loads the JSON into an `AppConfig` state so
   every command and event handler receives the same snapshot without repeating
   disk IO.【F:src-tauri/src/main.rs†L57-L105】

Because `Config` lives in the crate root, child modules like `commands` and
`champ_select` can import it without marking the struct `pub`. Rust privacy rules
let children access their parent module's private items, so only the fields need
`pub` to support serde serialization.【F:src-tauri/src/main.rs†L41-L55】【F:src-tauri/src/commands.rs†L1-L30】

## Gameflow Monitoring Pipeline

The backend spawns a long-lived async task that continually searches for the
League lockfile, constructs REST clients, and subscribes a websocket connection
once the client is available. Subscriptions include both `/lol-gameflow/v1/gameflow-phase`
and `/lol-champ-select/v1/session`, feeding all subsequent state transitions into
`handle_client_state`. Initial and subsequent phases are also broadcast back to
Svelte via `client_state_update`, while champ select payloads go through
`champ_select_started` so the UI can render team members.【F:src-tauri/src/main.rs†L69-L193】【F:src-tauri/src/state.rs†L1-L47】

## Auto-Open Execution Path

1. **Phase detection** – When `handle_client_state` sees the `ChampSelect` phase
   it clones the REST handles and schedules a task five seconds in the future.
   The delay lets Riot's champion select APIs stabilize before the next step and
   ensures the code reads the latest configuration snapshot before acting.【F:src-tauri/src/state.rs†L16-L35】
2. **Roster and region fetch** – `handle_champ_select_start` retrieves the lobby
   participants via `/chat/v5/participants`, filters out non champ-select
   entries, and loads region metadata from `/riotclient/region-locale`. The team
   is emitted to the UI, which updates the roster view and manual open button.
   【F:src-tauri/src/champ_select.rs†L1-L71】【F:src-tauri/src/lobby.rs†L1-L40】
3. **Provider launch** – If `config.auto_open` remains true, the helper formats a
   multi-search URL using the selected provider and opens it with the OS
   default handler. Special cases like `SG2` -> `SG` are normalized before the
   URL is built.【F:src-tauri/src/champ_select.rs†L72-L96】【F:src-tauri/src/utils.rs†L1-L73】
4. **Shared analytics** – The same function also fetches the active summoner and
   forwards analytics alongside the roster, demonstrating how auto-open shares
   its data fetching with other champ-select side effects.【F:src-tauri/src/champ_select.rs†L96-L99】

## Auto-Accept Execution Path

1. **Ready check detection** – A transition to `ReadyCheck` prompts
   `handle_client_state` to lock the `AppConfig` and inspect `auto_accept`.
   【F:src-tauri/src/state.rs†L35-L47】
2. **Delay handling** – When enabled, the handler sleeps for `accept_delay - 1000`
   milliseconds, mimicking a human response while giving users a small window to
   cancel by toggling the switch off. Because the config is read immediately
   before the sleep, any UI change propagates to the next ready check.
   【F:src-tauri/src/state.rs†L35-L47】
3. **REST acknowledgement** – After the delay the function posts an empty body to
   `/lol-matchmaking/v1/ready-check/accept`. If auto-accept is disabled (or the
   delay elapses and the user cancels beforehand) the call is skipped, allowing
   manual interaction.【F:src-tauri/src/state.rs†L35-L47】

The front end keeps users informed by showing the live gameflow state, champ
select rosters, and connection status. Manual overrides (e.g., the "Open Multi
Link" button) reuse the same REST and configuration helpers, so manual and
automatic workflows stay consistent.【F:src/lib/components/tool.svelte†L21-L130】【F:src-tauri/src/commands.rs†L45-L79】

## Extending the Flow to Auto-Report

Reveal's architecture makes it easy to bolt on additional automated actions,
like an auto-report flow:

1. **Capture the trigger** – Subscribe to the `/lol-gameflow/v1/gameflow-phase`
   transition into `PostGame` (or use an existing subscription) and branch on the
   new phase inside `handle_client_state`.
2. **Fetch the required data** – Use the same REST clients to retrieve the
   post-game scoreboard and current summoner identity.
3. **Honor shared configuration** – Extend the `Config` struct with
   `auto_report` flags and add matching UI toggles that use `updateConfig`.
4. **Execute the side effect** – POST the desired report payload to Riot's
   endpoint, respecting any delay or confirmation logic encoded in the config.

Because `AppConfig` and the websocket loop already exist, only the trigger and
handler need to be implemented, and the front end instantly gains new controls
through the shared config lifecycle.

## Porting the Pattern to C#

To replicate this behavior in a C# desktop application without modifying the
`shaco` Rust crate, you can:

1. **Expose a thin FFI layer** – Keep the Rust backend (with `shaco`) as a Tauri
   or standalone service that communicates over an IPC channel (e.g., WebSocket,
   HTTP, or a message queue). The C# UI would send the same config payloads and
   listen for the same events.
2. **Mirror the configuration contract** – Define a C# `Config` class matching
   the Rust/TypeScript shape and serialize it when calling the existing `set_config`
   handler. The Rust side retains ownership of disk persistence and Riot API
   calls, so no changes to `shaco` are necessary.
3. **Reuse event semantics** – Have the C# layer subscribe to the emitted
   `client_state_update`, `lcu_state_update`, and `champ_select_started` events
   (transported over the chosen IPC). UI buttons toggle booleans that flow to the
   Rust backend, and the backend keeps posting ready-check accepts or opening
   URLs exactly as before.
4. **Optional native REST/WebSocket** – If you must run everything in C#, use the
   documented REST endpoints and websocket subscriptions as a reference model.
   The same state machine applies: watch for gameflow transitions, use the lockfile
   credentials, delay when necessary, and honour the persisted configuration.

This separation keeps the reliable Rust implementation and `shaco` bindings in
place while allowing a C# shell to present the controls and status indicators in
an environment familiar to .NET teams.
