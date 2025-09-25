use crate::{AppConfig, ManagedReportState};
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

pub async fn try_auto_report(app_handle: &AppHandle, app_client: &RESTClient) {
    let auto_report_enabled = {
        let cfg = app_handle.state::<AppConfig>();
        let cfg = cfg.0.lock().await;
        cfg.auto_report
    };

    if !auto_report_enabled {
        return;
    }

    let stats_value = match app_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("Failed to fetch end of game stats: {:?}", err);
            return;
        }
    };

    let stats: EogStatsBlock = match serde_json::from_value(stats_value) {
        Ok(block) => block,
        Err(err) => {
            println!("Failed to parse end of game stats: {:?}", err);
            return;
        }
    };

    {
        let report_state = app_handle.state::<ManagedReportState>();
        let mut report_state = report_state.0.lock().await;
        if let Some(last_game) = report_state.last_reported_game {
            if last_game == stats.game_id {
                println!("Auto report already handled for game {}", stats.game_id);
                return;
            }
        }
        report_state.last_reported_game = Some(stats.game_id);
    }

    let friend_ids: HashSet<u64> = match app_client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(value) => match serde_json::from_value::<Vec<FriendEntry>>(value) {
            Ok(entries) => entries
                .into_iter()
                .filter_map(|entry| entry.summoner_id)
                .collect(),
            Err(err) => {
                println!("Failed to parse friend list: {:?}", err);
                HashSet::new()
            }
        },
        Err(err) => {
            println!("Failed to fetch friend list: {:?}", err);
            HashSet::new()
        }
    };

    let my_id = stats.local_player.summoner_id;

    for team in stats.teams {
        for player in team.players {
            if player.summoner_id == my_id {
                println!(
                    "Skipping report for {} ({}) – current account",
                    player.summoner_name,
                    player
                        .champion_name
                        .as_deref()
                        .unwrap_or("Unknown Champion")
                );
                continue;
            }

            if friend_ids.contains(&player.summoner_id) {
                println!(
                    "Skipping report for {} ({}) – friend detected",
                    player.summoner_name,
                    player
                        .champion_name
                        .as_deref()
                        .unwrap_or("Unknown Champion")
                );
                continue;
            }

            let body = serde_json::json!({
                "gameId": stats.game_id,
                "categories": REPORT_CATEGORIES,
                "offenderSummonerId": player.summoner_id,
                "offenderPuuid": player.puuid,
            });

            match app_client
                .post(
                    "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                    body,
                )
                .await
            {
                Ok(_) => {
                    println!(
                        "Auto reported {} ({})",
                        player.summoner_name,
                        player
                            .champion_name
                            .as_deref()
                            .unwrap_or("Unknown Champion")
                    );
                }
                Err(err) => {
                    println!("Failed to auto report {}: {:?}", player.summoner_name, err);
                }
            }
        }
    }
}
