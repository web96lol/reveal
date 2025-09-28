# Auto-report Shared State & Mutex Usage

This note explains how the auto-report toggle shares the same concurrency model as the existing auto-open and auto-accept automations, and why the `tokio::sync::Mutex` guarding `AppConfig` is sufficient for coordinating configuration reads across async tasks.

## Managed state registrations

`main.rs` registers three pieces of managed state with Tauri: `LCU`, `ManagedDodgeState`, and `AppConfig`. Each wrapper is a tuple struct over a `tokio::sync::Mutex`, so all async consumers share a single instance while getting `Send + Sync` access:

```rust
struct AppConfig(Mutex<Config>);
```

Because child modules can access their parent's private items, `Config` itself does not need to be `pub`; the fields are public so commands can serialize them, mirroring how the auto-open and auto-accept flags have always been exposed.

## Configuration lifecycle

During startup the app seeds `config.json`, loads it into `Config`, and manages it through `AppConfig(Mutex::new(cfg))`. Any command (for example, `set_config`) or event handler (`handle_client_state`) accesses the current values by acquiring the async mutex:

```rust
let cfg_state = app_handle.state::<AppConfig>();
let mut cfg = cfg_state.0.lock().await;
```

The lock lives only for the scope where the configuration is needed. Once the guard drops, other tasks—such as a simultaneous ready-check accept or a UI-driven `set_config`—can acquire the mutex.

## Gameflow dispatching

`state::handle_client_state` owns the fan-out from websocket events. Every branch that requires configuration data follows the same pattern:

* **Champ Select:** clone the `AppHandle` and REST clients, spawn a task, sleep five seconds, then lock `AppConfig` to drive `handle_champ_select_start`. This mirrors the original auto-open implementation.
* **Ready Check:** lock `AppConfig`, check `auto_accept`, and, if enabled, delay for `accept_delay - 1000` before posting to the accept endpoint—unchanged from the pre-auto-report code.
* **Post-game phases:** clone the handles, spawn a task, and lock `AppConfig` once inside to read `auto_report` and submit reports. The new branch matches the structure used by champ select.

Because each branch only holds the lock while reading config (and clones the data when longer-lived ownership is required), there is no risk of the mutex being held across awaits that would deadlock other tasks. The spawned post-game task, for example, clones the `Config` so that report submission does not need to keep the mutex guard alive.

## Relationship to the C# reference

The original C# reference code performed all logic on a single thread without explicit locking. In Rust/Tauri, the async runtime introduces concurrent tasks (websocket listener, UI commands), so the `tokio::Mutex` ensures only one mutable borrow of the shared config exists at a time. This pattern is the same one that has safely powered auto-open and auto-accept since the project's inception.

In short, the auto-report flow leverages the same `AppHandle` accessors and mutex-guarded configuration as the existing automations, so its state handling is consistent with, and no more complex than, auto-open/auto-accept.
