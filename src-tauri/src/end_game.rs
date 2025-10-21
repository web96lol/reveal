use crate::ReportState;
use serde_json::Value;
use shaco::rest::RESTClient;
use std::collections::HashSet;
use tauri::{AppHandle, Manager};

const REPORT_CATEGORIES: &[&str] = &[
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

/// Deduplicates end-of-game handling and auto-submits player reports.
pub async fn handle_end_of_game(
    app_handle: AppHandle,
    app_client: RESTClient,
    remoting_client: RESTClient,
) {
    println!("auto_report: end_of_game handler invoked");

    let response = match remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("auto_report: failed to fetch eog-stats-block: {err:?}");
            return;
        }
    };

    let game_id = match extract_game_id(&response) {
        Some(id) => id,
        None => {
            println!("auto_report: game id missing, cannot submit reports. payload={response:?}");
            return;
        }
    };

    // Deduplicate per game so we never report twice.
    let state = app_handle.state::<ReportState>();
    {
        let mut guard = state.0.lock().await;
        if guard.last_reported_game == Some(game_id) {
            println!("auto_report: already processed game {game_id}, skipping");
            return;
        }
        guard.last_reported_game = Some(game_id);
    }

    let friend_ids = match fetch_friend_ids(&app_client).await {
        Ok(ids) => ids,
        Err(err) => {
            println!("auto_report: failed to load friends list: {err}");
            HashSet::new()
        }
    };

    let local_player = response.get("localPlayer").and_then(|p| {
        let summoner_id = p.get("summonerId").and_then(|v| v.as_u64())?;
        let puuid = p.get("puuid").and_then(|v| v.as_str())?.to_string();
        Some((summoner_id, puuid))
    });

    let Some((local_summoner_id, _local_puuid)) = local_player else {
        println!("auto_report: failed to locate local player in eog payload");
        return;
    };

    let mut reports_sent = 0usize;
    let mut errors = 0usize;

    if let Some(teams) = response.get("teams").and_then(|v| v.as_array()) {
        for team in teams {
            if let Some(players) = team.get("players").and_then(|p| p.as_array()) {
                for player in players {
                    let Some(player_id) = player.get("summonerId").and_then(|v| v.as_u64()) else {
                        continue;
                    };

                    if player_id == local_summoner_id || friend_ids.contains(&player_id) {
                        continue;
                    }

                    let Some(player_puuid) = player.get("puuid").and_then(|v| v.as_str()) else {
                        continue;
                    };

                    let payload = serde_json::json!({
                        "gameId": game_id,
                        "categories": REPORT_CATEGORIES,
                        "offenderSummonerId": player_id,
                        "offenderPuuid": player_puuid,
                    });

                    match remoting_client
                        .post(
                            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                            payload,
                        )
                        .await
                    {
                        Ok(_) => {
                            reports_sent += 1;
                        }
                        Err(err) => {
                            errors += 1;
                            println!(
                                "auto_report: failed to submit report for player {player_id}: {err:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    println!(
        "auto_report: reports submitted for game {game_id} – success: {reports_sent}, errors: {errors}"
    );
}

fn extract_game_id(value: &Value) -> Option<u64> {
    fn parse_candidate(candidate: &Value) -> Option<u64> {
        match candidate {
            Value::Number(num) => num.as_u64(),
            Value::String(str_num) => str_num.parse().ok(),
            _ => None,
        }
    }

    if let Some(id) = value.get("gameId").and_then(parse_candidate) {
        return Some(id);
    }

    // Handle the common nested shapes we've seen in the LCU payload.
    for path in [
        "/gameId",
        "/gameResult/gameId",
        "/gameSummary/gameId",
        "/teams/0/gameId",
        "/localPlayer/gameId",
    ] {
        if let Some(id_value) = value.pointer(path) {
            if let Some(id) = parse_candidate(id_value) {
                return Some(id);
            }
        }
    }

    match value {
        Value::Object(map) => map.values().find_map(extract_game_id),
        Value::Array(arr) => arr.iter().find_map(extract_game_id),
        _ => None,
    }
}

async fn fetch_friend_ids(app_client: &RESTClient) -> Result<HashSet<u64>, String> {
    let response = app_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .map_err(|err| format!("{err:?}"))?;

    let mut ids = HashSet::new();
    if let Some(arr) = response.as_array() {
        for friend in arr {
            if let Some(id) = friend.get("summonerId").and_then(|v| v.as_u64()) {
                ids.insert(id);
            }
        }
    }

    Ok(ids)
}
