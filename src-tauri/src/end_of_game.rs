use crate::{Config, ManagedPostGameState};
use serde::Deserialize;
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EogStatsBlock {
    #[serde(rename = "gameId")]
    game_id: u64,
    local_player: EogPlayer,
    teams: Vec<EogTeam>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EogTeam {
    players: Vec<EogPlayer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EogPlayer {
    #[serde(rename = "summonerId")]
    summoner_id: u64,
    puuid: String,
    #[serde(rename = "summonerName")]
    summoner_name: String,
    #[serde(default)]
    champion_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FriendEntry {
    #[serde(rename = "summonerId")]
    summoner_id: Option<u64>,
}

pub async fn handle_end_of_game_phase(
    app_handle: &AppHandle,
    app_client: &RESTClient,
    config: &Config,
) -> bool {
    if !config.auto_report {
        return true;
    }

    let Some(stats) = fetch_eog_stats(app_client).await else {
        return false;
    };

    if !mark_game_if_new(app_handle, stats.game_id).await {
        println!("Post game already handled for {}", stats.game_id);
        return true;
    }

    let game_id = stats.game_id;
    let my_id = stats.local_player.summoner_id;
    let teams = stats.teams;

    let friend_ids = fetch_friend_ids(app_client).await.unwrap_or_default();
    let mut reported_count = 0u32;

    for team in teams {
        for player in team.players {
            if player.summoner_id == my_id {
                println!(
                    "Skipping auto report for {} ({}) — current account",
                    player.summoner_name,
                    player
                        .champion_name
                        .as_deref()
                        .unwrap_or("Unknown Champion"),
                );
                continue;
            }

            if friend_ids.contains(&player.summoner_id) {
                println!(
                    "Skipping auto report for {} ({}) — marked as friend",
                    player.summoner_name,
                    player
                        .champion_name
                        .as_deref()
                        .unwrap_or("Unknown Champion"),
                );
                continue;
            }

            let body = serde_json::json!({
                "gameId": game_id,
                "categories": REPORT_CATEGORIES,
                "offenderSummonerId": player.summoner_id,
                "offenderPuuid": player.puuid,
            });

            if let Err(err) = app_client
                .post(
                    "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                    body,
                )
                .await
            {
                println!("Unable to auto report {}: {:?}", player.summoner_name, err);
                continue;
            }

            println!(
                "Auto reported {} ({})",
                player.summoner_name,
                player
                    .champion_name
                    .as_deref()
                    .unwrap_or("Unknown Champion"),
            );
            reported_count += 1;
        }
    }

    println!(
        "Auto-report session complete for game {} — reported {} players",
        game_id, reported_count
    );

    true
}

async fn fetch_eog_stats(app_client: &RESTClient) -> Option<EogStatsBlock> {
    let raw_stats = match app_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("Unable to read end of game stats: {:?}", err);
            return None;
        }
    };

    match serde_json::from_value(raw_stats) {
        Ok(stats) => Some(stats),
        Err(err) => {
            println!("Unable to parse end of game stats: {:?}", err);
            None
        }
    }
}

async fn fetch_friend_ids(app_client: &RESTClient) -> Option<HashSet<u64>> {
    let raw_friends = app_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .ok()?;

    let entries: Vec<FriendEntry> = serde_json::from_value(raw_friends).ok()?;

    let ids = entries
        .into_iter()
        .filter_map(|entry| entry.summoner_id)
        .collect::<HashSet<u64>>();

    Some(ids)
}

async fn mark_game_if_new(app_handle: &AppHandle, game_id: u64) -> bool {
    let tracker = app_handle.state::<ManagedPostGameState>();
    let mut tracker = tracker.0.lock().await;

    if tracker.last_handled_game == Some(game_id) {
        return false;
    }

    tracker.last_handled_game = Some(game_id);
    true
}
