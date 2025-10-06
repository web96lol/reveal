Use the following prompt when you want a code assistant to analyze the project and outline how to extend the existing Shaco-powered workflow to cover the PreEndOfGame and EndOfGame phases without disrupting the current architecture:

```
You are working with a Tauri desktop application that already orchestrates League Client connectivity through the Shaco library. Please audit how the existing modules—`state`, `champ_select`, `lobby`, `region`, `summoner`, and `utils`—coordinate champ-select handling via `handle_champ_select_start`. Produce a detailed, phase-by-phase plan for adding PreEndOfGame and EndOfGame handling that mirrors the same modular structure, configuration pattern, and logging conventions. The plan must:

1. Show where `state::handle_client_state` should branch for the new phases, how to schedule any async follow-up tasks, and how those tasks reuse the cloned Shaco REST clients just like champ select.
2. Describe the responsibilities of a new `end_of_game` module that queries `/lol-end-of-game/v1/eog-stats-block`, deduplicates by game ID, respects friend-bias rules, and sends reports through `/lol-player-report-sender/v1/end-of-game-reports`, all while matching the repo's calm `println!` style (no spam) and surfacing actionable error handling when calls fail.
3. Identify the shared state (e.g., managed mutex wrappers) that must be reused or extended to track last-processed matches and feature toggles without introducing race conditions; highlight how an "auto report" toggle should live alongside `auto_open` and `auto_accept` in the config and Tauri-managed state.
4. Recommend console logging and Tauri event emissions that match the serene, informative tone used by the champ-select, auto-open, auto-accept, and dodge flows, ensuring any new output slots naturally into existing UX expectations.
5. Outline the configuration and UI changes necessary so an "Auto Report" control behaves exactly like the current auto-open/auto-accept toggles, including how commands should expose the toggle state and how the frontend should enable the button only when the feature is active.

Be explicit about which existing helpers (such as `display_champ_select`, analytics hooks, or config accessors) can be reused, and call out any new structs or enums you would introduce. Avoid suggesting large architectural rewrites; focus on incremental additions that preserve the repo's current elegance while enabling optional auto-report behavior.
```
