use crate::{
    champ_select::{self, ChampSelectSession},
    lobby::get_lobby_info,
    region::RegionInfo,
    sync_config,
    utils::display_champ_select,
    AppConfig, Config, ManagedDodgeState, LCU,
};
use shaco::rest::{LCUClientInfo, RESTClient};
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn app_ready(
    app_handle: AppHandle,
    lcu: tauri::State<'_, LCU>,
    cfg: tauri::State<'_, AppConfig>,
) -> Result<Config, ()> {
    println!("App Ready!");
    let lcu_guard = lcu.0.lock().await;
    let connected = lcu_guard.connected;
    let last_state = lcu_guard.last_state.clone();
    drop(lcu_guard);

    let cfg_guard = cfg.0.lock().await;
    let cfg_clone = cfg_guard.clone();
    drop(cfg_guard);

    println!("LCU State: {}", connected);
    println!("Config: {:?}", cfg_clone);

    app_handle.emit_all("lcu_state_update", connected).unwrap();

    if let Some(state) = last_state {
        app_handle.emit_all("client_state_update", state).unwrap();
    }

    Ok(cfg_clone)
}

#[tauri::command]
pub async fn get_lcu_state(lcu: tauri::State<'_, LCU>) -> Result<bool, ()> {
    let lcu = lcu.0.lock().await;
    Ok(lcu.connected)
}

#[tauri::command]
pub async fn get_config(cfg: tauri::State<'_, AppConfig>) -> Result<Config, ()> {
    let cfg = cfg.0.lock().await;
    Ok(cfg.clone())
}

#[tauri::command]
pub async fn set_config(
    cfg: tauri::State<'_, AppConfig>,
    new_cfg: Config,
    app_handle: AppHandle,
) -> Result<(), ()> {
    println!("Setting Config: {:?}", new_cfg);
    let mut cfg = cfg.0.lock().await;
    *cfg = new_cfg.clone();
    let cfg_snapshot = cfg.clone();
    drop(cfg);

    sync_config(&app_handle, &cfg_snapshot).await;

    Ok(())
}

#[tauri::command]
pub async fn get_lcu_info(lcu: tauri::State<'_, LCU>) -> Result<LCUClientInfo, ()> {
    let lcu = lcu.0.lock().await;
    Ok(lcu.data.clone().unwrap())
}

#[tauri::command]
pub async fn open_opgg_link(app_handle: AppHandle) -> Result<(), ()> {
    let lcu_state = app_handle.state::<LCU>();
    let lcu_state = lcu_state.0.lock().await;
    let Some(lcu_info) = lcu_state.data.clone() else {
        return Err(());
    };
    let app_client = RESTClient::new(lcu_info, false).map_err(|_| ())?;

    let config = app_handle.state::<AppConfig>();
    let config = config.0.lock().await;

    let team = get_lobby_info(&app_client).await;
    let region_info: RegionInfo = serde_json::from_value(
        app_client
            .get("/riotclient/region-locale".to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    let region = match region_info.web_region.as_str() {
        "SG2" => "SG",
        _ => &region_info.web_region,
    };

    display_champ_select(&team, region, &config.multi_provider);

    Ok(())
}

#[tauri::command]
pub async fn dodge(app_handle: AppHandle) -> Result<(), String> {
    let lcu_state = app_handle.state::<LCU>();
    let lcu_state = lcu_state.0.lock().await;
    let Some(lcu_info) = lcu_state.data.clone() else {
        return Err("LCU not connected".into());
    };
    let remoting_client = RESTClient::new(lcu_info, true).map_err(|e| e.to_string())?;

    println!("Attempting to quit champ select...");
    remoting_client
        .post(
            "/lol-login/v1/session/invoke?destination=lcdsServiceProxy&method=call&args=[\"\",\"teambuilder-draft\",\"quitV2\",\"\"]".to_string(),
            serde_json::json!({}),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn enable_dodge(app_handle: AppHandle) -> Result<(), String> {
    let lcu_state = app_handle.state::<LCU>();
    let lcu_state = lcu_state.0.lock().await;
    let Some(lcu_info) = lcu_state.data.clone() else {
        return Err("LCU not connected".into());
    };
    let remoting_client = RESTClient::new(lcu_info, true).map_err(|e| e.to_string())?;

    {
        let dodge_state = app_handle.state::<ManagedDodgeState>();
        let mut dodge_state = dodge_state.0.lock().await;
        if dodge_state.enabled.is_some() {
            if let Some(handle) = dodge_state.timer.take() {
                handle.abort();
            }
            dodge_state.enabled = None;
            drop(dodge_state);
            let _ = app_handle.emit_all("dodge_state_update", false);
            return Ok(());
        }
    }

    let champ_select = serde_json::from_value::<ChampSelectSession>(
        remoting_client
            .get("/lol-champ-select/v1/session".to_string())
            .await
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    champ_select::set_dodge_enabled(&app_handle, champ_select.game_id).await;

    Ok(())
}
