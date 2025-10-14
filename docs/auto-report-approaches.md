# Auto-Report Implementation Approaches

Reveal already exposes auto-open and auto-accept through a shared configuration
model and a set of focused backend handlers. Extending that architecture for
auto-reporting can follow two viable paths depending on how much you want to
reshape the existing control flow.

## Option A – Incremental Alignment

This approach mirrors the existing toggles with minimal churn:

- Reuse the gameflow dispatcher to trigger a post-game worker exactly once per
  match. The current implementation does this from `handle_client_state`,
  spawning a task whenever the phase enters an end-of-game value so that the
  heavy lifting runs off the websocket hot path.【F:src-tauri/src/state.rs†L20-L70】
- Centralise the post-game behaviour inside `handle_end_of_game_phase`, which
  collects the scoreboard, filters out the local summoner and friends, and sends
  one report per remaining player. Error paths log concise status messages so
  they read like the rest of the backend modules.【F:src-tauri/src/end_of_game.rs†L7-L130】
- Persist simple "already handled" state alongside the other managed mutexes in
  `main.rs`, preventing duplicate submissions if Riot emits multiple end-game
  phases for the same match.【F:src-tauri/src/main.rs†L49-L85】

The result fits neatly beside auto-open/auto-accept without additional surface
area: configuration stays in `AppConfig`, UI toggles flow through `updateConfig`,
while all Riot REST calls remain isolated in the dedicated helper module.

## Option B – Full Modular Parity

If you want perfect symmetry with the other automation toggles, you can push the
separation a bit further:

1. Introduce a `post_game` module that mirrors the layout of `champ_select`—one
   public entry point that emits UI events, runs analytics, and (optionally)
   fires reports. The `champ_select` module shows the pattern to follow: it
   fetches lobby participants, emits a front-end update, and conditionally opens
   a browser based on the live configuration.【F:src-tauri/src/champ_select.rs†L1-L104】
2. Extend the UI event surface with a `post_game_reported` payload so the Svelte
   layer can render feedback (e.g., which players were reported) the same way it
   already reflects champ-select updates.【F:src/lib/components/tool.svelte†L21-L130】
3. Split configuration concerns by adding any new post-game delays or filters to
   `Config`, persisting them through the same `set_config` command that handles
   the existing toggles.【F:src-tauri/src/commands.rs†L1-L64】
4. If analytics or history views are desired, chain them from the new module so
   they share the fetched `EogStatsBlock` payload, keeping outbound requests
   coalesced just like `handle_champ_select_start` bundles URL launches and
   analytics submissions.【F:src-tauri/src/champ_select.rs†L72-L104】

Option B demands a bit more plumbing (extra events and optional UI) but yields a
layout identical to the other automation flows, which may help future
contributors discover and extend the feature set.
