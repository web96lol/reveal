use crate::{
    champ_select::handle_champ_select_start, end_game::handle_end_game, AppConfig, ManagedDodgeState,
};
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};

pub async fn get_gameflow_state(remoting_client: &RESTClient) -> String {
    let gameflow_state = remoting_client
        .get("/lol-gameflow/v1/gameflow-phase".to_string())
        .await
        .unwrap()
        .to_string();

    let cleaned_state = gameflow_state.replace('\"', "");
    cleaned_state
}

pub async fn handle_client_state(
    client_state: String,
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
    app_client: &RESTClient,
) {
    match client_state.as_str() {
        "ChampSelect" => {
            let cloned_app_handle = app_handle.clone();
            let cloned_app_client = app_client.clone();
            let cloned_remoting = remoting_client.clone();

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                let cfg = {
                    let cfg_state = cloned_app_handle.state::<AppConfig>();
                    let cfg = cfg_state.0.lock().await;
                    cfg.clone()
                };
                handle_champ_select_start(
                    &cloned_app_client,
                    &cloned_remoting,
                    &cfg,
                    &cloned_app_handle,
                )
                .await;
            });
        }
        "ReadyCheck" => {
            let (auto_accept, accept_delay) = {
                let cfg = app_handle.state::<AppConfig>();
                let cfg = cfg.0.lock().await;
                (cfg.auto_accept, cfg.accept_delay)
            };
            if auto_accept {
                tokio::time::sleep(std::time::Duration::from_millis(
                    (accept_delay as u64) - 1000,
                ))
                .await;
                let _resp = remoting_client
                    .post(
                        "/lol-matchmaking/v1/ready-check/accept".to_string(),
                        serde_json::json!({}),
                    )
                    .await;
            }
        }
        "PreEndOfGame" | "EndOfGame" => {
            let cloned_app_handle = app_handle.clone();
            let cloned_remoting = remoting_client.clone();

            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                let cfg = {
                    let cfg_state = cloned_app_handle.state::<AppConfig>();
                    let cfg = cfg_state.0.lock().await;
                    cfg.clone()
                };

                if !cfg.auto_report {
                    return;
                }

                let last_reported_game = {
                    let dodge_state = cloned_app_handle.state::<ManagedDodgeState>();
                    let dodge_state = dodge_state.0.lock().await;
                    dodge_state.last_reported_game
                };

                if let Some(new_last) = handle_end_game(
                    &cloned_remoting,
                    &cfg,
                    &cloned_app_handle,
                    last_reported_game,
                )
                .await
                {
                    let dodge_state = cloned_app_handle.state::<ManagedDodgeState>();
                    let mut dodge_state = dodge_state.0.lock().await;
                    dodge_state.last_reported_game = Some(new_last);
                }
            });
        }
        _ => {}
    }

    println!("Client State Update: {}", client_state);
    app_handle
        .emit_all("client_state_update", client_state)
        .unwrap();
}
