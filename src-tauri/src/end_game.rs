use std::collections::HashSet;

use serde_json::Value;
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};

use crate::AppConfig;

const REPORT_CATEGORIES: [&str; 7] = [
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

#[derive(Debug, Clone)]
struct PlayerRecord {
    summoner_id: String,
    puuid: String,
    name: Option<String>,
    tag: Option<String>,
    is_local: bool,
}

impl PlayerRecord {
    fn display_name(&self) -> String {
        match (&self.name, &self.tag) {
            (Some(name), Some(tag)) if !tag.is_empty() => format!("{}#{}", name, tag),
            (Some(name), _) => name.clone(),
            _ => self.puuid.clone(),
        }
    }
}

pub async fn handle_end_of_game(app_handle: &AppHandle, remoting_client: &RESTClient) {
    let auto_report_enabled = {
        let cfg = app_handle.state::<AppConfig>();
        let cfg = cfg.0.lock().await;
        cfg.auto_report
    };

    if !auto_report_enabled {
        return;
    }

    println!("Auto report: processing end of game stats");

    let friend_puuids = fetch_friend_puuids(remoting_client).await;

    let session_data = match remoting_client
        .get("/lol-gameflow/v1/session".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!(
                "Auto report: failed to fetch gameflow session ({:?}). Skipping reporting.",
                err
            );
            return;
        }
    };

    let eog_stats = match remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!(
                "Auto report: failed to fetch end-of-game data ({:?}). Skipping reporting.",
                err
            );
            return;
        }
    };

    let game_id = match extract_game_id(&eog_stats) {
        Some(id) => id,
        None => {
            println!("Auto report: missing gameId in end-of-game data. Skipping reporting.");
            return;
        }
    };

    let mut players = collect_players(&eog_stats);
    if players.is_empty() {
        println!("Auto report: no players discovered in end-of-game data.");
        return;
    }

    let (mut local_summoner_id, mut local_puuid) = extract_local_identifiers(&session_data);
    if local_summoner_id.is_none() || local_puuid.is_none() {
        if let Some(local_player) = players.iter().find(|player| player.is_local) {
            if local_summoner_id.is_none() {
                local_summoner_id = Some(local_player.summoner_id.clone());
            }
            if local_puuid.is_none() {
                local_puuid = Some(local_player.puuid.clone());
            }
        }
    }

    for player in players.into_iter() {
        if player.summoner_id.is_empty() || player.puuid.is_empty() {
            continue;
        }

        if player.is_local {
            continue;
        }

        if let Some(ref puuid) = local_puuid {
            if &player.puuid == puuid {
                continue;
            }
        }

        if let Some(ref summoner_id) = local_summoner_id {
            if &player.summoner_id == summoner_id {
                continue;
            }
        }

        if friend_puuids.contains(&player.puuid) {
            continue;
        }

        if let Err(err) = submit_report(remoting_client, &game_id, &player).await {
            println!(
                "Auto report: failed to report {} ({:?}).",
                player.display_name(),
                err
            );
        }
    }
}

async fn fetch_friend_puuids(client: &RESTClient) -> HashSet<String> {
    match client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(value) => value
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("puuid").and_then(|v| v.as_str()))
                    .map(|puuid| puuid.to_string())
                    .collect::<HashSet<String>>()
            })
            .unwrap_or_default(),
        Err(err) => {
            println!(
                "Auto report: failed to fetch friends list ({:?}). Skipping friend filter.",
                err
            );
            HashSet::new()
        }
    }
}

async fn submit_report(
    client: &RESTClient,
    game_id: &str,
    player: &PlayerRecord,
) -> Result<(), String> {
    let payload = serde_json::json!({
        "gameId": game_id,
        "categories": REPORT_CATEGORIES,
        "offenderSummonerId": player.summoner_id,
        "offenderPuuid": player.puuid,
    });

    match client
        .post(
            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
            payload,
        )
        .await
    {
        Ok(_) => {
            println!(
                "Auto report: submitted report for {}.",
                player.display_name()
            );
            Ok(())
        }
        Err(err) => Err(format!("{:?}", err)),
    }
}

fn extract_game_id(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(id_value) = map.get("gameId") {
                if let Some(id) = value_to_string(id_value) {
                    return Some(id);
                }
            }
            for child in map.values() {
                if let Some(id) = extract_game_id(child) {
                    return Some(id);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(extract_game_id),
        _ => None,
    }
}

fn collect_players(value: &Value) -> Vec<PlayerRecord> {
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    collect_players_recursive(value, false, &mut seen, &mut records);
    records
}

fn collect_players_recursive(
    value: &Value,
    is_local_hint: bool,
    seen: &mut HashSet<String>,
    out: &mut Vec<PlayerRecord>,
) {
    match value {
        Value::Object(map) => {
            let mut current_is_local = is_local_hint;
            if map
                .get("isLocalSummoner")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || map.get("isSelf").and_then(|v| v.as_bool()).unwrap_or(false)
                || map
                    .get("isLocalPlayer")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                || map.get("isMe").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                current_is_local = true;
            }

            if let Some(local_player) = map.get("localPlayer") {
                collect_players_recursive(local_player, true, seen, out);
            }

            if let (Some(puuid), Some(summoner_id)) = (
                map.get("puuid").and_then(|v| v.as_str()),
                map.get("summonerId").and_then(value_to_string),
            ) {
                if seen.insert(puuid.to_string()) {
                    let name = map
                        .get("gameName")
                        .or_else(|| map.get("summonerName"))
                        .or_else(|| map.get("name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let tag = map
                        .get("tagLine")
                        .or_else(|| map.get("gameTag"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    out.push(PlayerRecord {
                        summoner_id,
                        puuid: puuid.to_string(),
                        name,
                        tag,
                        is_local: current_is_local,
                    });
                }
            }

            for child in map.values() {
                collect_players_recursive(child, current_is_local, seen, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_players_recursive(item, is_local_hint, seen, out);
            }
        }
        _ => {}
    }
}

fn extract_local_identifiers(value: &Value) -> (Option<String>, Option<String>) {
    let mut result = (None, None);
    extract_local_identifiers_recursive(value, false, &mut result);
    result
}

fn extract_local_identifiers_recursive(
    value: &Value,
    is_local_hint: bool,
    result: &mut (Option<String>, Option<String>),
) {
    if result.0.is_some() && result.1.is_some() {
        return;
    }

    match value {
        Value::Object(map) => {
            if let Some(local_id) = map.get("localSummonerId").and_then(value_to_string) {
                if result.0.is_none() {
                    result.0 = Some(local_id);
                }
            }
            if let Some(local_puuid) = map.get("localPlayerPuuid").and_then(|v| v.as_str()) {
                if result.1.is_none() {
                    result.1 = Some(local_puuid.to_string());
                }
            }

            let mut current_is_local = is_local_hint;
            if map
                .get("isLocalSummoner")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || map.get("isSelf").and_then(|v| v.as_bool()).unwrap_or(false)
                || map
                    .get("isLocalPlayer")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                || map.get("isMe").and_then(|v| v.as_bool()).unwrap_or(false)
            {
                current_is_local = true;
            }

            if current_is_local {
                if let Some(summoner_id) = map.get("summonerId").and_then(value_to_string) {
                    if result.0.is_none() {
                        result.0 = Some(summoner_id);
                    }
                }
                if let Some(puuid) = map.get("puuid").and_then(|v| v.as_str()) {
                    if result.1.is_none() {
                        result.1 = Some(puuid.to_string());
                    }
                }
            }

            if let Some(local_player) = map.get("localPlayer") {
                extract_local_identifiers_recursive(local_player, true, result);
            }

            for child in map.values() {
                extract_local_identifiers_recursive(child, current_is_local, result);
            }
        }
        Value::Array(items) => {
            for item in items {
                extract_local_identifiers_recursive(item, is_local_hint, result);
            }
        }
        _ => {}
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(num) => Some(num.to_string()),
        _ => None,
    }
}
