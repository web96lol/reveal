use crate::{
    champ_select::handle_champ_select_start, end_of_game::handle_end_of_game_phase, AppConfig,
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

                let cfg = cloned_app_handle.state::<AppConfig>();
                let cfg = cfg.0.lock().await;
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
            let cfg = app_handle.state::<AppConfig>();
            let cfg = cfg.0.lock().await;
            if cfg.auto_accept {
                let delay = cfg.accept_delay.saturating_sub(1000);
                tokio::time::sleep(std::time::Duration::from_millis(delay as u64)).await;
                let _resp = remoting_client
                    .post(
                        "/lol-matchmaking/v1/ready-check/accept".to_string(),
                        serde_json::json!({}),
                    )
                    .await;
            }
        }
        "WaitingForStats" | "PreEndOfGame" | "EndOfGame" => {
            let cfg_state = app_handle.state::<AppConfig>();
            let config = cfg_state.0.lock().await.clone();

            if config.auto_report {
                let cloned_handle = app_handle.clone();
                let cloned_client = app_client.clone();

                tauri::async_runtime::spawn(async move {
                    let mut succeeded = false;

                    for attempt in 1..=10 {
                        if handle_end_of_game_phase(&cloned_handle, &cloned_client, &config).await {
                            if attempt > 1 {
                                println!(
                                    "Auto-report finished after {} attempts during post game phase",
                                    attempt
                                );
                            }
                            succeeded = true;
                            break;
                        }

                        if attempt < 10 {
                            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                        }
                    }

                    if !succeeded {
                        println!("Auto-report failed to gather end of game stats after retries");
                    }
                });
            }
        }
        _ => {}
    }

    println!("Client State Update: {}", client_state);
    app_handle
        .emit_all("client_state_update", client_state)
        .unwrap();
}
