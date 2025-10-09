use std::collections::HashSet;

use crate::default_report_categories;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

#[derive(Debug, Default)]
pub struct ReportState {
    pub last_game_id: Option<u64>,
    pub local_summoner_id: Option<u64>,
    pub friend_ids: HashSet<u64>,
}

pub struct ManagedReportState(pub Mutex<ReportState>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndOfGameSummary {
    pub game_id: u64,
    pub reported_count: usize,
    pub skipped_count: usize,
}

struct EndOfGameSnapshot {
    game_id: u64,
    local_summoner_id: u64,
    players: Vec<PlayerSummary>,
}

struct PlayerSummary {
    summoner_id: u64,
    summoner_name: String,
    champion_name: Option<String>,
    puuid: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportRequest {
    game_id: u64,
    categories: Vec<String>,
    offender_summoner_id: u64,
    offender_puuid: String,
}

pub async fn handle_end_of_game_phase(
    app_client: &RESTClient,
    remoting_client: &RESTClient,
    app_handle: &AppHandle,
) {
    let snapshot = match fetch_end_of_game_snapshot(remoting_client).await {
        Ok(snapshot) => snapshot,
        Err(err) => {
            println!("Unable to read end-of-game stats: {}", err);
            return;
        }
    };

    let report_state = app_handle.state::<ManagedReportState>();

    ensure_friend_ids(app_client, &report_state).await;

    let mut state = report_state.0.lock().await;
    if let Some(last_game_id) = state.last_game_id {
        if last_game_id == snapshot.game_id {
            println!(
                "End of game {} already handled, skipping additional report run",
                snapshot.game_id
            );
            return;
        }
    }

    state.last_game_id = Some(snapshot.game_id);
    state.local_summoner_id = Some(snapshot.local_summoner_id);
    let friend_ids = state.friend_ids.clone();
    drop(state);

    let categories = default_report_categories();

    println!(
        "Evaluating end-of-game reports for game {}",
        snapshot.game_id
    );

    let mut reported_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;

    for player in snapshot.players {
        if player.summoner_id == snapshot.local_summoner_id {
            println!("Skipping report for local player {}", player.summoner_name);
            skipped_count += 1;
            continue;
        }

        if friend_ids.contains(&player.summoner_id) {
            println!("Skipping report for friend {}", player.summoner_name);
            skipped_count += 1;
            continue;
        }

        let payload = ReportRequest {
            game_id: snapshot.game_id,
            categories: categories.clone(),
            offender_summoner_id: player.summoner_id,
            offender_puuid: player.puuid.clone(),
        };

        let endpoint = "/lol-player-report-sender/v1/end-of-game-reports".to_string();
        let report_result = remoting_client.post(endpoint, payload).await;

        match report_result {
            Ok(_) => {
                println!(
                    "Reported {} ({})",
                    player.summoner_name,
                    player
                        .champion_name
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string())
                );
                reported_count += 1;
            }
            Err(err) => {
                if err.is_decode() || err.status() == Some(StatusCode::NO_CONTENT) {
                    println!(
                        "Reported {} ({})",
                        player.summoner_name,
                        player
                            .champion_name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string())
                    );
                    reported_count += 1;
                } else {
                    println!(
                        "Failed to report {} ({}): {}",
                        player.summoner_name,
                        player
                            .champion_name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        err
                    );
                    failed_count += 1;
                }
            }
        }
    }

    if failed_count > 0 {
        println!(
            "Finished processing auto-report for game {}: {} reported, {} skipped, {} failed",
            snapshot.game_id, reported_count, skipped_count, failed_count
        );
    } else {
        println!(
            "Finished processing auto-report for game {}: {} reported, {} skipped",
            snapshot.game_id, reported_count, skipped_count
        );
    }

    let event = EndOfGameSummary {
        game_id: snapshot.game_id,
        reported_count,
        skipped_count,
    };

    app_handle
        .emit_all("end_of_game_processed", event)
        .unwrap_or_else(|err| println!("Failed to emit end_of_game_processed: {}", err));
}

async fn fetch_end_of_game_snapshot(
    remoting_client: &RESTClient,
) -> Result<EndOfGameSnapshot, String> {
    let value = remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
        .map_err(|err| err.to_string())?;

    let game_id = value
        .get("gameId")
        .and_then(value_to_u64)
        .ok_or_else(|| "Missing gameId".to_string())?;

    let local_player = value
        .get("localPlayer")
        .and_then(Value::as_object)
        .ok_or_else(|| "Missing localPlayer".to_string())?;

    let local_summoner_id = local_player
        .get("summonerId")
        .and_then(value_to_u64)
        .ok_or_else(|| "Missing local summonerId".to_string())?;

    let teams = value
        .get("teams")
        .and_then(Value::as_array)
        .ok_or_else(|| "Missing teams".to_string())?;

    let mut players = Vec::new();

    for team in teams {
        if let Some(entries) = team.get("players").and_then(Value::as_array) {
            for entry in entries {
                let summoner_id = match entry.get("summonerId").and_then(value_to_u64) {
                    Some(id) => id,
                    None => continue,
                };
                let summoner_name = entry
                    .get("summonerName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let champion_name = entry
                    .get("championName")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                let puuid = entry
                    .get("puuid")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                players.push(PlayerSummary {
                    summoner_id,
                    summoner_name,
                    champion_name,
                    puuid,
                });
            }
        }
    }

    Ok(EndOfGameSnapshot {
        game_id,
        local_summoner_id,
        players,
    })
}

async fn ensure_friend_ids(app_client: &RESTClient, report_state: &ManagedReportState) {
    let mut needs_refresh = false;
    {
        let state = report_state.0.lock().await;
        if state.friend_ids.is_empty() {
            needs_refresh = true;
        }
    }

    if !needs_refresh {
        return;
    }

    let friends_response = match app_client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(value) => value,
        Err(err) => {
            println!("Unable to fetch friends list: {}", err);
            return;
        }
    };

    let mut friend_ids = HashSet::new();
    if let Some(entries) = friends_response.as_array() {
        for entry in entries {
            if let Some(id) = entry.get("summonerId").and_then(value_to_u64) {
                friend_ids.insert(id);
            }
        }
    }

    let mut state = report_state.0.lock().await;
    if state.friend_ids.is_empty() {
        state.friend_ids = friend_ids;
        println!("Cached {} friend identifiers", state.friend_ids.len());
    }
}

fn value_to_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|v| v.try_into().ok()))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}
