use crate::{Config, ManagedEndOfGameState};
use serde::Serialize;
use shaco::rest::RESTClient;
use std::collections::HashSet;
use tauri::{AppHandle, Manager};

const REPORT_CATEGORIES: [&str; 7] = [
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportSummary {
    game_id: u64,
    outcomes: Vec<ReportOutcome>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportOutcome {
    summoner_name: String,
    champion_name: String,
    status: String,
    message: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportPayload {
    game_id: u64,
    categories: Vec<&'static str>,
    offender_summoner_id: u64,
    offender_puuid: String,
}

pub async fn handle_end_of_game(
    remoting_client: &RESTClient,
    app_client: &RESTClient,
    config: &Config,
    app_handle: &AppHandle,
) {
    if !config.auto_report {
        return;
    }

    let eog_value = match remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("Failed to fetch end-of-game stats: {err}");
            return;
        }
    };

    let game_id = match parse_id(eog_value.get("gameId")) {
        Some(id) => id,
        None => return,
    };

    let eog_state = app_handle.state::<ManagedEndOfGameState>();
    {
        let mut state = eog_state.0.lock().await;
        if state.last_game_id == Some(game_id) {
            return;
        }
        state.last_game_id = Some(game_id);
    }

    let local_player_id = eog_value
        .get("localPlayer")
        .and_then(|lp| parse_id(lp.get("summonerId")));

    let friend_ids = load_friend_ids(app_client).await;

    let mut outcomes = Vec::new();

    if let Some(teams) = eog_value.get("teams").and_then(|v| v.as_array()) {
        for team in teams {
            if let Some(players) = team.get("players").and_then(|v| v.as_array()) {
                for player in players {
                    if let Some(outcome) = handle_player(
                        remoting_client,
                        player,
                        game_id,
                        local_player_id,
                        &friend_ids,
                    )
                    .await
                    {
                        outcomes.push(outcome);
                    }
                }
            }
        }
    }

    if outcomes.is_empty() {
        return;
    }

    let summary = ReportSummary { game_id, outcomes };
    if let Err(err) = app_handle.emit_all("end_of_game_processed", &summary) {
        println!("Failed to emit end_of_game_processed event: {err}");
    }
}

async fn load_friend_ids(client: &RESTClient) -> HashSet<u64> {
    let mut ids = HashSet::new();

    match client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(value) => {
            if let Some(friends) = value.as_array() {
                for friend in friends {
                    if let Some(id) = parse_id(friend.get("summonerId")) {
                        ids.insert(id);
                    }
                }
            }
        }
        Err(err) => {
            println!("Failed to load friends list: {err}");
        }
    }

    ids
}

async fn handle_player(
    remoting_client: &RESTClient,
    player: &serde_json::Value,
    game_id: u64,
    local_player_id: Option<u64>,
    friends: &HashSet<u64>,
) -> Option<ReportOutcome> {
    let summoner_name = player
        .get("summonerName")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let champion_name = player
        .get("championName")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();

    let Some(summoner_id) = parse_id(player.get("summonerId")) else {
        println!(
            "{} ({}) has no summoner id, skipping",
            summoner_name, champion_name
        );
        return Some(ReportOutcome {
            summoner_name,
            champion_name,
            status: "skipped".to_string(),
            message: Some("Missing summoner id".to_string()),
        });
    };

    if local_player_id == Some(summoner_id) {
        println!(
            "{} ({}) is the current account, ignoring",
            summoner_name, champion_name
        );
        return Some(ReportOutcome {
            summoner_name,
            champion_name,
            status: "skipped".to_string(),
            message: Some("Local player".to_string()),
        });
    }

    if friends.contains(&summoner_id) {
        println!(
            "{} ({}) is a friend, ignoring",
            summoner_name, champion_name
        );
        return Some(ReportOutcome {
            summoner_name,
            champion_name,
            status: "skipped".to_string(),
            message: Some("Friend detected".to_string()),
        });
    }

    let Some(puuid) = player.get("puuid").and_then(|v| v.as_str()) else {
        println!(
            "{} ({}) has no PUUID, skipping",
            summoner_name, champion_name
        );
        return Some(ReportOutcome {
            summoner_name,
            champion_name,
            status: "skipped".to_string(),
            message: Some("Missing PUUID".to_string()),
        });
    };

    let payload = ReportPayload {
        game_id,
        categories: REPORT_CATEGORIES.to_vec(),
        offender_summoner_id: summoner_id,
        offender_puuid: puuid.to_string(),
    };

    let res = remoting_client
        .post(
            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
            &payload,
        )
        .await;

    match res {
        Ok(_) => {
            println!("{} ({}) has been reported", summoner_name, champion_name);
            Some(ReportOutcome {
                summoner_name,
                champion_name,
                status: "reported".to_string(),
                message: None,
            })
        }
        Err(err) => {
            println!(
                "Failed to report {} ({}): {}",
                summoner_name, champion_name, err
            );
            Some(ReportOutcome {
                summoner_name,
                champion_name,
                status: "failed".to_string(),
                message: Some(format!("Request error: {err}")),
            })
        }
    }
}

fn parse_id(value: Option<&serde_json::Value>) -> Option<u64> {
    value.and_then(|v| {
        if let Some(id) = v.as_u64() {
            Some(id)
        } else if let Some(id_str) = v.as_str() {
            id_str.parse().ok()
        } else {
            None
        }
    })
}
