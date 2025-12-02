// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod champ_select;
mod commands;
mod end_game;
mod lobby;
mod region;
mod state;
mod utils;

use champ_select::{handle_champ_select_ws_event, reset_dodge_state, ChampSelectSession};
use commands::{
    app_ready, dodge, enable_dodge, get_config, get_lcu_info, get_lcu_state, open_opgg_link,
    set_config,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use shaco::model::ws::LcuEvent;
use shaco::rest::RESTClient;
use shaco::utils::process_info;
use shaco::ws::LcuWebsocketClient;
use shaco::{model::ws::LcuSubscriptionType::JsonApiEvent, rest::LCUClientInfo};
use std::{io, time::Duration};
use tauri::{
    async_runtime::JoinHandle, AppHandle, CustomMenuItem, Manager, SystemTray, SystemTrayEvent,
    SystemTrayMenu, SystemTrayMenuItem, Window, WindowEvent,
};
use tauri_plugin_positioner::{Position, WindowExt};
use tokio::sync::Mutex;

struct LCU(Mutex<LCUState>);

pub struct LCUState {
    pub connected: bool,
    pub data: Option<LCUClientInfo>,
    pub last_state: Option<String>,
}

pub struct AppConfig(Mutex<Config>);

pub struct ManagedDodgeState(Mutex<DodgeState>);

pub struct DodgeState {
    pub last_dodge: Option<u64>,
    pub enabled: Option<u64>,
    pub timer: Option<JoinHandle<()>>,
}

pub struct ReportState(pub Mutex<ReportCache>);

#[derive(Default)]
pub struct ReportCache {
    pub last_reported_game: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub auto_open: bool,
    pub auto_accept: bool,
    #[serde(default)]
    pub auto_report: bool,
    pub accept_delay: u32,
    #[serde(default = "default_provider")]
    pub multi_provider: String,
}

fn default_provider() -> String {
    "opgg".to_string()
}

const MAIN_WINDOW_LABEL: &str = "main";
const MENU_ID_OPEN: &str = "open";
const MENU_ID_AUTO_REPORT: &str = "auto_report";
const MENU_ID_AUTO_OPEN: &str = "auto_open";
const MENU_ID_AUTO_ACCEPT: &str = "auto_accept";
const MENU_ID_QUIT: &str = "quit";

#[derive(Clone, Copy)]
enum ConfigFlag {
    AutoReport,
    AutoOpen,
    AutoAccept,
}

fn main() {
    let tray = build_system_tray();

    tauri::Builder::default()
        .manage(LCU(Mutex::new(LCUState {
            connected: false,
            data: None,
            last_state: None,
        })))
        .manage(ReportState(Mutex::new(ReportCache::default())))
        .manage(ManagedDodgeState(Mutex::new(DodgeState {
            last_dodge: None,
            enabled: None,
            timer: None,
        })))
        .plugin(tauri_plugin_positioner::init())
        .system_tray(tray)
        .on_window_event(|event| {
            if let WindowEvent::CloseRequested { api, .. } = event.event() {
                if event.window().label() == MAIN_WINDOW_LABEL {
                    let window = event.window();
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .on_system_tray_event(|app, event| {
            tauri_plugin_positioner::on_tray_event(app, &event);
            match event {
                SystemTrayEvent::LeftClick { .. } => toggle_main_window(app),
                SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                    MENU_ID_OPEN => show_main_window_from_app(app),
                    MENU_ID_AUTO_REPORT => toggle_config_flag(app, ConfigFlag::AutoReport),
                    MENU_ID_AUTO_OPEN => toggle_config_flag(app, ConfigFlag::AutoOpen),
                    MENU_ID_AUTO_ACCEPT => toggle_config_flag(app, ConfigFlag::AutoAccept),
                    MENU_ID_QUIT => app.exit(0),
                    _ => {}
                },
                _ => {}
            }
        })
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
                    auto_accept: false,
                    auto_report: false,
                    accept_delay: 2000,
                    multi_provider: "opgg".to_string(),
                };

                let cfg_json = serde_json::to_string(&cfg).unwrap();
                std::fs::write(&cfg_path, cfg_json).unwrap();
            }

            let cfg_json = std::fs::read_to_string(&cfg_path).unwrap();
            let cfg: Config = serde_json::from_str(&cfg_json).unwrap();
            app.manage(AppConfig(Mutex::new(cfg.clone())));
            update_tray_menu_labels(&app_handle, &cfg);

            tauri::async_runtime::spawn(async move {
                let mut connected = true;

                loop {
                    let args = process_info::get_league_process_args();
                    if args.is_none() {
                        if connected {
                            println!("Waiting for League Client to open...");
                            connected = false;
                            app_handle.emit_all("lcu_state_update", false).unwrap();

                            let lcu_state = app_handle.state::<LCU>();
                            let mut guard = lcu_state.0.lock().await;
                            guard.connected = false;
                            guard.data = None;
                            guard.last_state = None;
                            drop(guard);
                        }

                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }

                    let args = args.unwrap();

                    let lcu_info = process_info::get_auth_info(args).unwrap();
                    let app_client = RESTClient::new(lcu_info.clone(), false).unwrap();
                    let remoting_client = RESTClient::new(lcu_info.clone(), true).unwrap();

                    let cloned_app_handle = app_handle.clone();
                    let lcu = cloned_app_handle.state::<LCU>();

                    connected = true;
                    app_handle.emit_all("lcu_state_update", true).unwrap();

                    let mut lcu = lcu.0.lock().await;
                    lcu.connected = true;
                    lcu.data = Some(lcu_info);
                    lcu.last_state = None;

                    drop(lcu);

                    // The websocket event API will not be opened until a few seconds after the client is opened.
                    let mut ws = match LcuWebsocketClient::connect().await {
                        Ok(ws) => ws,
                        Err(_) => {
                            let mut attempts = 0;
                            loop {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                if attempts > 5 {
                                    panic!("Failed to connect to League Client!");
                                }

                                attempts += 1;
                                match LcuWebsocketClient::connect().await {
                                    Ok(ws) => break ws,
                                    Err(_) => continue,
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

                    println!("Connected to League Client!");

                    let state = state::get_gameflow_state(&remoting_client).await;
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn handle_ws_message(
    msg: LcuEvent,
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
    app_client: &RESTClient,
) {
    let msg_type = msg.subscription_type.to_string();

    match msg_type.as_str() {
        "OnJsonApiEvent_lol-gameflow_v1_gameflow-phase" => {
            let client_state = msg.data.to_string().replace('\"', "");
            state::handle_client_state(client_state, app_handle, remoting_client, app_client).await;
        }
        "OnJsonApiEvent_lol-champ-select_v1_session" => {
            let champ_select = serde_json::from_value::<ChampSelectSession>(msg.data.clone());
            if let Ok(session) = champ_select {
                handle_champ_select_ws_event(session, app_handle, remoting_client).await;
            } else {
                println!("Failed to parse champ select session!, {:?}", champ_select);
                reset_dodge_state(app_handle).await;
            }
        }
        _ => {
            println!("Unhandled Message: {}", msg_type);
        }
    }
}

fn build_system_tray() -> SystemTray {
    let open_window = CustomMenuItem::new(MENU_ID_OPEN, "Open Reveal");
    let auto_open = CustomMenuItem::new(
        MENU_ID_AUTO_OPEN,
        format_toggle_label("Auto Open Multi", false),
    );
    let auto_accept = CustomMenuItem::new(
        MENU_ID_AUTO_ACCEPT,
        format_toggle_label("Auto Accept", false),
    );
    let auto_report = CustomMenuItem::new(
        MENU_ID_AUTO_REPORT,
        format_toggle_label("Auto Report", false),
    );
    let quit = CustomMenuItem::new(MENU_ID_QUIT, "Quit");

    let menu = SystemTrayMenu::new()
        .add_item(open_window)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(auto_open)
        .add_item(auto_accept)
        .add_item(auto_report)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    SystemTray::new().with_menu(menu)
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW_LABEL) {
        match window.is_visible() {
            Ok(true) => {
                let _ = window.hide();
            }
            Ok(false) | Err(_) => show_main_window(&window),
        }
    }
}

fn show_main_window_from_app(app: &AppHandle) {
    if let Some(window) = app.get_window(MAIN_WINDOW_LABEL) {
        show_main_window(&window);
    }
}

fn show_main_window(window: &Window) {
    let _ = window.unminimize();
    let _ = window.move_window(Position::TrayCenter);
    let _ = window.show();
    let _ = window.set_focus();
}

fn toggle_config_flag(app: &AppHandle, flag: ConfigFlag) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let cfg_state = app_handle.state::<AppConfig>();
        let mut cfg_guard = cfg_state.0.lock().await;
        match flag {
            ConfigFlag::AutoReport => cfg_guard.auto_report = !cfg_guard.auto_report,
            ConfigFlag::AutoOpen => cfg_guard.auto_open = !cfg_guard.auto_open,
            ConfigFlag::AutoAccept => cfg_guard.auto_accept = !cfg_guard.auto_accept,
        }

        let cfg_snapshot = cfg_guard.clone();
        drop(cfg_guard);

        sync_config(&app_handle, &cfg_snapshot).await;
    });
}

pub(crate) async fn sync_config(app: &AppHandle, cfg: &Config) {
    if let Err(err) = persist_config(app, cfg).await {
        eprintln!("failed to persist config: {err}");
    }

    update_tray_menu_labels(app, cfg);
    let _ = app.emit_all("config_updated", cfg.clone());
}

async fn persist_config(app: &AppHandle, cfg: &Config) -> io::Result<()> {
    if let Some(cfg_dir) = app.path_resolver().app_config_dir() {
        if !cfg_dir.exists() {
            tokio::fs::create_dir_all(&cfg_dir).await?;
        }
        let cfg_path = cfg_dir.join("config.json");
        let cfg_json = serde_json::to_string(cfg).unwrap();
        tokio::fs::write(cfg_path, cfg_json).await
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "config directory unavailable",
        ))
    }
}

fn update_tray_menu_labels(app: &AppHandle, cfg: &Config) {
    let tray_handle = app.tray_handle();
    let _ = tray_handle
        .get_item(MENU_ID_AUTO_OPEN)
        .set_title(format_toggle_label("Auto Open Multi", cfg.auto_open));
    let _ = tray_handle
        .get_item(MENU_ID_AUTO_ACCEPT)
        .set_title(format_toggle_label("Auto Accept", cfg.auto_accept));
    let _ = tray_handle
        .get_item(MENU_ID_AUTO_REPORT)
        .set_title(format_toggle_label("Auto Report", cfg.auto_report));
}

fn format_toggle_label(name: &str, value: bool) -> String {
    let status = if value { "On" } else { "Off" };
    format!("{name}: {status}")
}
