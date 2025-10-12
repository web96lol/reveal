use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

pub struct ManagedReportState(pub Mutex<ReportState>);

#[derive(Default)]
pub struct ReportState {
    pub last_reported_game: Option<u64>,
    pub local_player_id: Option<u64>,
    pub friend_ids: HashSet<u64>,
    pub friends_loaded: bool,
}

const REPORT_CATEGORIES: [&str; 7] = [
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FriendEntry {
    summoner_id: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EogStatsBlock {
    #[serde(default)]
    game_id: Option<u64>,
    #[serde(default)]
    local_player: Option<EogPlayer>,
    #[serde(default)]
    teams: Vec<EogTeam>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EogTeam {
    #[serde(default)]
    players: Vec<EogPlayer>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EogPlayer {
    #[serde(default)]
    summoner_id: Option<u64>,
    #[serde(default)]
    puuid: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRequest<'a> {
    game_id: u64,
    categories: &'a [&'a str],
    #[serde(rename = "offenderSummonerId")]
    offender_summoner_id: u64,
    #[serde(rename = "offenderPuuid")]
    offender_puuid: &'a str,
}

pub async fn preload_friends(app_handle: &AppHandle, app_client: &RESTClient) {
    if should_skip_friend_fetch(app_handle).await {
        return;
    }

    let friends = match fetch_friend_ids(app_client).await {
        Some(ids) => ids,
        None => return,
    };

    let report_state = app_handle.state::<ManagedReportState>();
    let mut report_state = report_state.0.lock().await;
    report_state.friend_ids = friends;
    report_state.friends_loaded = true;
}

pub async fn reset_connection_state(app_handle: &AppHandle) {
    let report_state = app_handle.state::<ManagedReportState>();
    let mut report_state = report_state.0.lock().await;
    report_state.friends_loaded = false;
    report_state.friend_ids.clear();
    report_state.last_reported_game = None;
    report_state.local_player_id = None;
}

pub async fn handle_end_of_game(app_handle: &AppHandle, app_client: &RESTClient) {
    if ensure_friend_list(app_handle, app_client).await.is_none() {
        return;
    }

    let stats_block = match app_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => serde_json::from_value::<EogStatsBlock>(value).ok(),
        Err(err) => {
            println!("Failed to load end of game stats: {:?}", err);
            None
        }
    };

    let Some(stats_block) = stats_block else {
        return;
    };

    let Some(game_id) = stats_block.game_id else {
        return;
    };

    let local_player = stats_block.local_player.clone();
    let teams = stats_block.teams;

    let report_state = app_handle.state::<ManagedReportState>();
    let (local_id, friend_ids) = {
        let mut guard = report_state.0.lock().await;
        if guard.last_reported_game == Some(game_id) {
            return;
        }
        guard.last_reported_game = Some(game_id);
        if let Some(local_player) = local_player.as_ref() {
            guard.local_player_id = local_player.summoner_id;
        }
        (guard.local_player_id, guard.friend_ids.clone())
    };

    println!("Auto-reporting teammates for game {}", game_id);

    for team in teams {
        for player in team.players {
            let Some(summoner_id) = player.summoner_id else {
                continue;
            };

            if Some(summoner_id) == local_id {
                continue;
            }

            if friend_ids.contains(&summoner_id) {
                continue;
            }

            let Some(puuid) = player.puuid.as_deref() else {
                continue;
            };

            let request = ReportRequest {
                game_id,
                categories: &REPORT_CATEGORIES,
                offender_summoner_id: summoner_id,
                offender_puuid: puuid,
            };

            if let Err(err) = app_client
                .post(
                    "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                    request,
                )
                .await
            {
                println!("Failed to auto report player {}: {:?}", summoner_id, err);
            }
        }
    }
}

async fn ensure_friend_list(app_handle: &AppHandle, app_client: &RESTClient) -> Option<()> {
    if should_skip_friend_fetch(app_handle).await {
        return Some(());
    }

    let friends = fetch_friend_ids(app_client).await?;

    let report_state = app_handle.state::<ManagedReportState>();
    let mut report_state = report_state.0.lock().await;
    report_state.friend_ids = friends;
    report_state.friends_loaded = true;

    Some(())
}

async fn should_skip_friend_fetch(app_handle: &AppHandle) -> bool {
    let report_state = app_handle.state::<ManagedReportState>();
    let report_state = report_state.0.lock().await;
    report_state.friends_loaded
}

async fn fetch_friend_ids(app_client: &RESTClient) -> Option<HashSet<u64>> {
    let response = match app_client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(value) => value,
        Err(err) => {
            println!("Failed to fetch friends list: {:?}", err);
            return None;
        }
    };

    let friends: Vec<FriendEntry> = match serde_json::from_value(response) {
        Ok(entries) => entries,
        Err(err) => {
            println!("Failed to parse friends list: {:?}", err);
            return None;
        }
    };

    let friend_ids = friends
        .into_iter()
        .filter_map(|friend| friend.summoner_id)
        .collect::<HashSet<_>>();

    Some(friend_ids)
}
