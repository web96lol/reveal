use std::collections::HashSet;

use crate::{default_report_categories, Config};
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
pub struct PlayerReportOutcome {
    pub summoner_name: String,
    pub champion_name: Option<String>,
    pub status: ReportOutcomeStatus,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReportOutcomeStatus {
    Reported,
    SkippedFriend,
    SkippedSelf,
    Failed,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndOfGameReportEvent {
    pub game_id: u64,
    pub results: Vec<PlayerReportOutcome>,
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

pub async fn handle_end_of_game_phase(
    app_client: &RESTClient,
    remoting_client: &RESTClient,
    app_handle: &AppHandle,
    config: &Config,
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

    let categories = if config.report_categories.is_empty() {
        default_report_categories()
    } else {
        config.report_categories.clone()
    };

    println!(
        "Evaluating end-of-game reports for game {}",
        snapshot.game_id
    );

    let mut outcomes = Vec::new();

    for player in snapshot.players {
        if player.summoner_id == snapshot.local_summoner_id {
            outcomes.push(PlayerReportOutcome {
                summoner_name: player.summoner_name,
                champion_name: player.champion_name,
                status: ReportOutcomeStatus::SkippedSelf,
                message: "Skipped local player".to_string(),
            });
            continue;
        }

        if friend_ids.contains(&player.summoner_id) {
            outcomes.push(PlayerReportOutcome {
                summoner_name: player.summoner_name,
                champion_name: player.champion_name,
                status: ReportOutcomeStatus::SkippedFriend,
                message: "Skipped friend".to_string(),
            });
            continue;
        }

        let payload = serde_json::json!({
            "gameId": snapshot.game_id,
            "categories": &categories,
            "offenderSummonerId": player.summoner_id,
            "offenderPuuid": player.puuid,
        });

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
                outcomes.push(PlayerReportOutcome {
                    summoner_name: player.summoner_name,
                    champion_name: player.champion_name,
                    status: ReportOutcomeStatus::Reported,
                    message: "Report submitted".to_string(),
                });
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
                    outcomes.push(PlayerReportOutcome {
                        summoner_name: player.summoner_name,
                        champion_name: player.champion_name,
                        status: ReportOutcomeStatus::Reported,
                        message: "Report acknowledged".to_string(),
                    });
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
                    outcomes.push(PlayerReportOutcome {
                        summoner_name: player.summoner_name,
                        champion_name: player.champion_name,
                        status: ReportOutcomeStatus::Failed,
                        message: format!("Report failed: {}", err),
                    });
                }
            }
        }
    }

    println!(
        "Finished processing auto-report for game {}",
        snapshot.game_id
    );

    let event = EndOfGameReportEvent {
        game_id: snapshot.game_id,
        results: outcomes,
    };

    app_handle
        .emit_all("end_of_game_report", event)
        .unwrap_or_else(|err| println!("Failed to emit end_of_game_report: {}", err));
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
