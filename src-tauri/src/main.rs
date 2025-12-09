#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod end_game;
mod champ_select;
mod commands;
mod lobby;
mod region;
mod state;
mod utils;

use crate::champ_select::ChampSelectSession;
use crate::commands::{
    app_ready, dodge, enable_dodge, get_config, get_lcu_info, get_lcu_state, open_opgg_link,
    set_config,
};
use crate::state::get_gameflow_state;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use shaco::model::ws::LcuEvent;
use shaco::rest::RESTClient;
use shaco::utils::process_info;
use shaco::ws::LcuWebsocketClient;
use shaco::{model::ws::LcuSubscriptionType::JsonApiEvent, rest::LCUClientInfo};
use std::time::Duration;
use tauri::{
    AppHandle, CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, WindowEvent,
};
use tauri_plugin_positioner::{on_tray_event, Position, WindowExt};
use tokio::sync::Mutex;

/* ───────────────────────────────────────────────────────────────
   Shared Global State Wrappers
───────────────────────────────────────────────────────────────*/

struct LCU(Mutex<LCUState>);

pub struct LCUState {
    pub connected: bool,
    pub data: Option<LCUClientInfo>,
}

struct ManagedDodgeState(Mutex<DodgeState>);

pub struct DodgeState {
    pub last_dodge: Option<u64>,
    pub enabled: Option<u64>,
}

struct ManagedReportState(Mutex<ReportState>);
pub struct ReportState {
    pub last_report: Option<u64>,
}

struct AppConfig(Mutex<Config>);

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub auto_open: bool,
    pub auto_accept: bool,
    pub accept_delay: u32,
    #[serde(default = "default_provider")]
    pub multi_provider: String,
    #[serde(default)]
    pub auto_report: bool,
}

fn default_provider() -> String {
    "opgg".to_string()
}

/* ───────────────────────────────────────────────────────────────
   Main Application Entry
───────────────────────────────────────────────────────────────*/

fn main() {
    let open_reveal = CustomMenuItem::new("open_reveal".to_string(), "Open Reveal");
    let quit_reveal = CustomMenuItem::new("quit_reveal".to_string(), "Quit Reveal");
    let tray_menu = SystemTrayMenu::new()
        .add_item(open_reveal)
        .add_item(quit_reveal);
    let system_tray = SystemTray::new().with_menu(tray_menu);

    tauri::Builder::default()
        .manage(LCU(Mutex::new(LCUState {
            connected: false,
            data: None,
        })))
        .manage(ManagedDodgeState(Mutex::new(DodgeState {
            last_dodge: None,
            enabled: None,
        })))
        .manage(ManagedReportState(Mutex::new(ReportState {
            last_report: None,
        })))
        .setup(|app| {
            let app_handle = app.handle();
            let cfg_folder = app.path_resolver().app_config_dir().unwrap();

            if !cfg_folder.exists() {
                std::fs::create_dir(&cfg_folder).unwrap();
            }

            let cfg_path = cfg_folder.join("config.json");

            if !cfg_path.exists() {
                let cfg = Config {
                    auto_open: true,
                    auto_accept: true,
                    accept_delay: 2000,
                    multi_provider: "opgg".to_string(),
                    auto_report: false,
                };

                let cfg_json = serde_json::to_string(&cfg).unwrap();
                std::fs::write(&cfg_path, cfg_json).unwrap();
            }

            let cfg_json = std::fs::read_to_string(&cfg_path).unwrap();
            let cfg: Config = serde_json::from_str(&cfg_json).unwrap();
            app.manage(AppConfig(Mutex::new(cfg)));

            tauri::async_runtime::spawn(async move {
                let mut connected = true;

                loop {
                    let args = process_info::get_league_process_args();
                    if args.is_none() {
                        if connected {
                            println!("Waiting for League Client to open...");
                            connected = false;
                            app_handle.emit_all("lcu_state_update", false).unwrap();
                        }

                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    let args = args.unwrap();

                    let lcu_info = process_info::get_auth_info(args).unwrap();
                    let app_client = RESTClient::new(lcu_info.clone(), false).unwrap();
                    let remoting_client = RESTClient::new(lcu_info.clone(), true).unwrap();

                    let cloned_app = app_handle.clone();
                    {
                        let lcu_state = cloned_app.state::<LCU>();
                        let mut guard = lcu_state.0.lock().await;
                        guard.connected = true;
                        guard.data = Some(lcu_info.clone());
                    }

                    connected = true;
                    app_handle.emit_all("lcu_state_update", true).unwrap();

                    let mut ws = match LcuWebsocketClient::connect().await {
                        Ok(ws) => ws,
                        Err(_) => {
                            let mut attempts = 0;
                            loop {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                attempts += 1;
                                if attempts > 5 {
                                    panic!("Failed to connect to League Client websocket!");
                                }

                                if let Ok(ws2) = LcuWebsocketClient::connect().await {
                                    break ws2;
                                }
                            }
                        }
                    };

                    ws.subscribe(JsonApiEvent("/lol-gameflow/v1/gameflow-phase".to_string()))
                        .await
                        .unwrap();

                    ws.subscribe(JsonApiEvent("/lol-champ-select/v1/session".to_string()))
                        .await
                        .unwrap();

                    println!("Connected to League Client WebSocket!");

                    let state = get_gameflow_state(&remoting_client).await;
                    state::handle_client_state(state, &app_handle, &remoting_client, &app_client)
                        .await;

                    while let Some(msg) = ws.next().await {
                        handle_ws_message(msg, &app_handle, &remoting_client, &app_client).await;
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_ready,
            get_lcu_state,
            get_lcu_info,
            get_config,
            set_config,
            open_opgg_link,
            dodge,
            enable_dodge
        ])
     .plugin(tauri_plugin_positioner::init())
        .system_tray(system_tray)
        .on_system_tray_event(|app, event| {
            on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick { .. } => {
                    if let Some(win) = app.get_window("main") {
                        if win.is_visible().unwrap_or(false) {
                            let _ = win.hide();
                        } else {
                            let _ = win.show();
                            let _ = win.set_focus();
                            let _ = win.move_window(Position::TrayCenter);
                        }
                    }
                }

                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    "open_reveal" => {
                        if let Some(win) = app.get_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                            let _ = win.move_window(Position::TrayCenter);
                        }
                    }

                    "quit_reveal" => {
                        app.exit(0);
                    }

                    _ => {}
                },

                _ => {}
            }
        })
        .on_window_event(|event| {
            if let WindowEvent::CloseRequested { api, .. } = event.event() {
                event.window().hide().unwrap();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/* ───────────────────────────────────────────────────────────────
   Websocket Message Routing
───────────────────────────────────────────────────────────────*/

async fn handle_ws_message(
    msg: LcuEvent,
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
    app_client: &RESTClient,
) {
    let msg_type = msg.subscription_type.to_string();

    match msg_type.as_str() {
        "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase" => {
            let client_state = msg.data.to_string().replace('"', "");
            state::handle_client_state(client_state, app_handle, remoting_client, app_client).await;
        }

        "OnJsonApiEvent_lol-champ-select_v1_session" => {
            let champ_select = serde_json::from_value::<ChampSelectSession>(msg.data.clone());
            if champ_select.is_err() {
                println!("Failed to parse champ select session!");
                return;
            }

            let champ_select = champ_select.unwrap();

            if champ_select.timer.phase == "FINALIZATION" {
                let time = champ_select.timer.adjusted_time_left_in_phase;
                let cloned_remoting = remoting_client.clone();
                let game_id = champ_select.game_id;

                let dodge_state = app_handle.state::<ManagedDodgeState>();
                let mut dodge_state = dodge_state.0.lock().await;

                if let Some(last_dodge) = dodge_state.last_dodge {
                    if last_dodge == game_id {
                        return;
                    }
                }

                if (dodge_state.enabled.is_some() && dodge_state.enabled.unwrap() != game_id)
                    || dodge_state.enabled.is_none()
                {
                    return;
                }

                dodge_state.last_dodge = Some(game_id);
                drop(dodge_state);

                println!("Spawning finalization dodge in {}ms", time);

                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(time)).await;
                    println!("Sending dodge…");
                    let _resp = cloned_remoting
                        .post(
                            "/lol-login/v1/session/invoke?destination=lcdsServiceProxy&method=call&args=[\"\",\"teambuilder-draft\",\"quitV2\",\"\"]"
                                .to_string(),
                            serde_json::json!({}),
                        )
                        .await;
                });
            }
        }

        _ => {
            println!("Unhandled Message Type: {}", msg_type);
        }
    }
}
