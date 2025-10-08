use std::collections::HashSet;
use std::time::Duration;

use crate::{summoner, AppConfig, ManagedReportState};
use serde_json::Value;
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};

const REPORT_CATEGORIES: &[&str] = [
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

pub async fn record_pending_game(app_handle: &AppHandle, remoting_client: &RESTClient) {
    let session: Value = remoting_client
        .get("/lol-gameflow/v1/session".to_string())
        .await
        .unwrap();

    if let Some(game_data) = session.get("gameData") {
        if let Some(game_id_value) = game_data.get("gameId") {
            if let Some(game_id) = parse_numeric_id(game_id_value) {
                let report_state = app_handle.state::<ManagedReportState>();
                let mut report_state = report_state.0.lock().await;
                report_state.pending_game = Some(game_id);
            }
        }
    }
}

pub async fn auto_report_players(
    app_handle: &AppHandle,
    remoting_client: &RESTClient,
    app_client: &RESTClient,
) {
    let cfg = app_handle.state::<AppConfig>();
    let cfg = cfg.0.lock().await;
    if !cfg.auto_report {
        return;
    }
    drop(cfg);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let eog_stats: Value = remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
        .unwrap();

    let report_state_handle = app_handle.state::<ManagedReportState>();
    let mut report_state = report_state_handle.0.lock().await;

    let mut game_id = eog_stats
        .get("gameId")
        .and_then(|value| parse_numeric_id(value));

    if game_id.is_none() {
        game_id = report_state.pending_game;
    }

    let game_id = match game_id {
        Some(id) => id,
        None => return,
    };

    if report_state.last_reported_game == Some(game_id) {
        return;
    }

    report_state.pending_game = Some(game_id);
    drop(report_state);

    let current_summoner = summoner::get_current_summoner(remoting_client).await;

    let friends_value = app_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .unwrap();

    let mut friend_puuids = HashSet::new();
    if let Some(friends) = friends_value.as_array() {
        for friend in friends {
            if let Some(puuid) = friend.get("puuid").and_then(|value| value.as_str()) {
                friend_puuids.insert(puuid.to_string());
            }
        }
    }

    let mut seen_puuids = HashSet::new();
    let mut report_payloads = Vec::new();

    if let Some(teams) = eog_stats.get("teams").and_then(|value| value.as_array()) {
        for team in teams {
            if let Some(players) = team.get("players").and_then(|value| value.as_array()) {
                for player in players {
                    let puuid = match player.get("puuid").and_then(|value| value.as_str()) {
                        Some(puuid) => puuid,
                        None => continue,
                    };

                    if puuid == current_summoner.puuid {
                        continue;
                    }

                    if !seen_puuids.insert(puuid.to_string()) {
                        continue;
                    }

                    if friend_puuids.contains(puuid) {
                        continue;
                    }

                    let summoner_id = match player
                        .get("summonerId")
                        .and_then(|value| parse_numeric_id(value))
                    {
                        Some(id) if id != 0 => id,
                        _ => continue,
                    };

                    report_payloads.push(serde_json::json!({
                        "categories": REPORT_CATEGORIES,
                        "offenderPuuid": puuid,
                        "offenderSummonerId": summoner_id,
                        "gameId": game_id,
                    }));
                }
            }
        }
    }

    let report_count = report_payloads.len();
    if report_count == 0 {
        let report_state_handle = app_handle.state::<ManagedReportState>();
        let mut report_state = report_state_handle.0.lock().await;
        report_state.last_reported_game = Some(game_id);
        report_state.pending_game = None;
        return;
    }

    let request_body = serde_json::Value::Array(report_payloads);
    remoting_client
        .post(
            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
            request_body,
        )
        .await
        .unwrap();

    let report_state_handle = app_handle.state::<ManagedReportState>();
    let mut report_state = report_state_handle.0.lock().await;
    report_state.last_reported_game = Some(game_id);
    report_state.pending_game = None;
    report_state.total_reports_sent += report_count as u32;

    println!(
        "Auto reported {} players for game {}. Total auto reports: {}",
        report_count, game_id, report_state.total_reports_sent
    );
}

fn parse_numeric_id(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }

    if let Some(text) = value.as_str() {
        return text.parse::<i64>().ok();
    }

    None
}
