Use this prompt when you want an assistant to study the Reveal repo and craft a serene implementation plan for an end-of-game auto-report feature that mirrors the existing champ-select automation:

```
You are helping with a polished Tauri desktop app that already uses the custom Shaco crate for League Client REST + websocket access. First, review the supplied C# console sample (`end_game` + `lcu.cs`) to understand the intended report bias rules and logging cadence. Then audit the Rust project, calling out how every `.rs` module (`main`, `state`, `champ_select`, `lobby`, `region`, `summoner`, `analytics`, `commands`, `utils`) currently collaborates—especially how `state::handle_client_state` and `champ_select::handle_champ_select_start` reuse cloned `RESTClient`s, guarded mutex state, and calm one-line `println!`s.

Produce an incremental plan (no architecture rewrites) that:
1. Extends the phase pipeline so `PreEndOfGame` and `EndOfGame` branch just like champ select, spawning async work that reuses the prepared Shaco clients and respects dedupe-by-game-id rules.
2. Designs a new `end_of_game` module that mirrors the C# logic: fetch `/lol-end-of-game/v1/eog-stats-block`, track `lastGameId`, skip the local player + friends, and send `/lol-player-report-sender/v1/end-of-game-reports` with configurable categories, surfacing graceful error logs only.
3. Specifies how to add an `auto_report` toggle beside `auto_open`/`auto_accept` in `Config`, managed state, Tauri commands, and the Svelte UI, including when the frontend button is enabled.
4. Points out any analytics or event emissions that should fire, matching the existing UX tone and avoiding spammy output.

List the exact helpers, structs, or enums to reuse or introduce, and detail testing/observability notes so the feature stays as blissfully modular as the current dodge + auto-open flow.
```
