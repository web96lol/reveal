use crate::{lobby, region::RegionInfo, utils::display_champ_select, Config};
use serde::{Deserialize, Serialize};
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampSelectSession {
    pub allow_battle_boost: bool,
    pub allow_duplicate_picks: bool,
    pub allow_locked_events: bool,
    pub allow_rerolling: bool,
    pub allow_skin_selection: bool,
    pub bench_enabled: bool,
    pub boostable_skin_count: i64,
    pub counter: i64,
    pub game_id: u64,
    pub has_simultaneous_bans: bool,
    pub has_simultaneous_picks: bool,
    pub is_custom_game: bool,
    pub is_spectating: bool,
    pub local_player_cell_id: i64,
    pub locked_event_index: i64,
    pub recovery_counter: i64,
    pub rerolls_remaining: i64,
    pub skip_champion_select: bool,
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
    remoting_client: &RESTClient,
    config: &Config,
    app_handle: &AppHandle,
) {
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

    let mut opened = false;
    let mut last_count = 0;

    loop {
        // Stop when champion select ends
        let state = remoting_client
            .get("/lol-gameflow/v1/gameflow-phase".to_string())
            .await;

        if let Ok(s) = state {
            let s = s.to_string().replace('\"', "");
            if s != "ChampSelect" {
                break;
            }
        } else {
            break;
        }

        let team = lobby::get_lobby_info(app_client).await;
        let count = team.participants.len();

        if count > last_count {
            last_count = count;
            app_handle.emit_all("champ_select_started", &team).unwrap();

            if config.auto_open && !opened && count > 0 {
                display_champ_select(&team, region, &config.multi_provider);
                opened = true;
            }
        }

        if count >= 5 {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
