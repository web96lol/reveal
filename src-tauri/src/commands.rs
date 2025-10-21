use crate::{AppConfig, Config, LCU};
use shaco::rest::LCUClientInfo;
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
    *cfg = new_cfg;

    // Save config to disk
    let cfg_folder = app_handle.path_resolver().app_config_dir().unwrap();
    let cfg_path = cfg_folder.join("config.json");
    let cfg_json = serde_json::to_string(&cfg.clone()).unwrap();
    tokio::fs::write(&cfg_path, cfg_json).await.unwrap();

    Ok(())
}

#[tauri::command]
pub async fn get_lcu_info(lcu: tauri::State<'_, LCU>) -> Result<LCUClientInfo, ()> {
    let lcu = lcu.0.lock().await;
    Ok(lcu.data.clone().unwrap())
}
