# Repository guidance

- This Tauri app already coordinates League phases through `main.rs`, `state.rs`, and `champ_select.rs`; treat that flow as the reference architecture when planning or coding new features.
- Any end-of-game automation must mirror the `auto_open`/`auto_accept` configuration style: introduce toggles through the shared `Config`, emit calm one-line `println!` status messages, and guard repeated actions with cached game identifiers.
- Use the existing Shaco REST and websocket clients that `main.rs` prepares; do not build new HTTP stacks or spawn unmanaged tasks.
- When documenting or responding, acknowledge how each Rust module participates in the phase pipeline and explain where new work would hook in (e.g., `state::handle_client_state`, a dedicated `end_of_game` module, analytics, UI commands).
- Avoid sweeping repository changes; keep edits scoped and reversible.

