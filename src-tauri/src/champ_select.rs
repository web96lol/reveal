use crate::{lobby, region::RegionInfo, utils::display_champ_select, Config, ManagedDodgeState};
use serde::{Deserialize, Serialize};
use shaco::rest::RESTClient;
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSession {
    pub game_id: u64,
    pub timer: Timer,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub adjusted_time_left_in_phase: u64,
    pub internal_now_in_epoch_ms: u64,
    pub is_infinite: bool,
    pub phase: String,
    pub total_time_in_phase: i64,
}

pub async fn handle_champ_select_start(
    app_client: &RESTClient,
    _remoting_client: &RESTClient,
    config: &Config,
    app_handle: &AppHandle,
) {
    let team = lobby::get_lobby_info(app_client).await;
    let region_info: RegionInfo = serde_json::from_value(
        app_client
            .get("/riotclient/region-locale".to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    app_handle.emit_all("champ_select_started", &team).unwrap();

    if config.auto_open {
        let region = match region_info.web_region.as_str() {
            "SG2" => "SG",
            _ => &region_info.web_region,
        };

        display_champ_select(&team, region, &config.multi_provider);
    }
}

pub async fn handle_champ_select_ws_event(
    champ_select: ChampSelectSession,
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
) {
    if champ_select.timer.phase == "FINALIZATION" {
        schedule_last_second_dodge(
            champ_select.timer.adjusted_time_left_in_phase,
            champ_select.game_id,
            app_handle,
            remoting_client,
        )
        .await;
    }
}

pub async fn set_dodge_enabled(app_handle: &AppHandle, game_id: u64) {
    let dodge_state = app_handle.state::<ManagedDodgeState>();
    let mut dodge_state = dodge_state.0.lock().await;

    if dodge_state.last_dodge == Some(game_id) || dodge_state.enabled == Some(game_id) {
        return;
    }

    if let Some(handle) = dodge_state.timer.take() {
        handle.abort();
    }

    dodge_state.enabled = Some(game_id);
    drop(dodge_state);

    let _ = app_handle.emit_all("dodge_state_update", true);
}

pub async fn reset_dodge_state(app_handle: &AppHandle) {
    let dodge_state = app_handle.state::<ManagedDodgeState>();
    let mut dodge_state = dodge_state.0.lock().await;
    dodge_state.enabled = None;
    if let Some(handle) = dodge_state.timer.take() {
        handle.abort();
    }
    drop(dodge_state);

    let _ = app_handle.emit_all("dodge_state_update", false);
}

async fn schedule_last_second_dodge(
    time_ms: u64,
    game_id: u64,
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
) {
    let dodge_state = app_handle.state::<ManagedDodgeState>();
    let mut dodge_state = dodge_state.0.lock().await;

    if dodge_state.last_dodge == Some(game_id) || dodge_state.enabled != Some(game_id) {
        return;
    }

    if let Some(handle) = dodge_state.timer.take() {
        handle.abort();
    }

    println!("Spawned task to dodge in finalization timer: {}ms", time_ms);

    let delay = time_ms.saturating_sub(100);
    let cloned_handle = app_handle.clone();
    let cloned_client = remoting_client.clone();
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay)).await;
        println!("Last second dodge calling quit endpoint...");
        if let Err(err) = cloned_client
            .post(
                "/lol-login/v1/session/invoke?destination=lcdsServiceProxy&method=call&args=[\"\",\"teambuilder-draft\",\"quitV2\",\"\"]"
                    .to_string(),
                serde_json::json!({}),
            )
            .await
        {
            eprintln!("failed to quit champ select: {err}");
        }
        finalize_dodge(&cloned_handle, game_id).await;
    });

    dodge_state.timer = Some(handle);
}

async fn finalize_dodge(app_handle: &AppHandle, game_id: u64) {
    let dodge_state = app_handle.state::<ManagedDodgeState>();
    let mut dodge_state = dodge_state.0.lock().await;
    dodge_state.enabled = None;
    dodge_state.last_dodge = Some(game_id);
    if let Some(handle) = dodge_state.timer.take() {
        handle.abort();
    }
    drop(dodge_state);

    let _ = app_handle.emit_all("dodge_state_update", false);
}
