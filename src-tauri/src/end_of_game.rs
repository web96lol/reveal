use crate::Config;
use serde::{Deserialize, Serialize};
use shaco::rest::RESTClient;
use std::collections::HashSet;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

static LAST_REPORTED_GAME: OnceLock<Mutex<Option<u64>>> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EndOfGameStats {
    pub game_id: u64,
    pub local_player: LocalPlayer,
    #[serde(default)]
    pub teams: Vec<Team>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlayer {
    pub summoner_id: u64,
    pub summoner_name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    #[serde(default)]
    pub players: Vec<Player>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    #[serde(default)]
    pub summoner_id: Option<u64>,
    #[serde(default)]
    pub summoner_name: Option<String>,
    #[serde(default)]
    pub champion_name: Option<String>,
    #[serde(default)]
    pub puuid: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerReportPayload {
    pub game_id: u64,
    pub categories: Vec<String>,
    pub offender_summoner_id: u64,
    pub offender_puuid: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    pub summoner_id: String,
}

const REPORT_CATEGORIES: &[&str] = &[
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

pub async fn handle_end_of_game_start(
    remoting_client: &RESTClient,
    app_handle: &AppHandle,
    config: &Config,
) {
    if !config.auto_report {
        return;
    }

    let stats = match fetch_end_of_game_stats(remoting_client).await {
        Some(stats) => stats,
        None => return,
    };

    let last_reported = LAST_REPORTED_GAME.get_or_init(|| Mutex::new(None));
    {
        let mut guard = last_reported.lock().await;
        if guard.as_ref() == Some(&stats.game_id) {
            return;
        }
        *guard = Some(stats.game_id);
    }

    println!("Processing end-of-game reports for game: {}", stats.game_id);
    println!("------------------");

    let friend_ids = get_friend_ids(remoting_client).await;
    let payloads = collect_report_payloads(&stats, &friend_ids);

    if payloads.is_empty() {
        println!("No eligible end-of-game reports to send");
    } else {
        for (payload, player_name, champion_name) in payloads {
            match remoting_client
                .post(
                    "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                    serde_json::to_value(&payload).unwrap(),
                )
                .await
            {
                Ok(_) => {
                    println!(
                        "{} ({}) has been reported",
                        player_name.unwrap_or_else(|| "Unknown Summoner".to_string()),
                        champion_name.unwrap_or_else(|| "Unknown Champion".to_string())
                    );
                }
                Err(err) => {
                    println!(
                        "Failed to report {} ({}): {:?}",
                        player_name.unwrap_or_else(|| "Unknown Summoner".to_string()),
                        champion_name.unwrap_or_else(|| "Unknown Champion".to_string()),
                        err
                    );
                }
            }
        }
    }

    println!("------------------");

    if let Err(err) = app_handle.emit_all("end_game_reports_sent", stats.game_id) {
        println!("Failed to emit end_game_reports_sent event: {:?}", err);
    }
}

async fn fetch_end_of_game_stats(client: &RESTClient) -> Option<EndOfGameStats> {
    match client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(response) => match serde_json::from_value::<EndOfGameStats>(response) {
            Ok(stats) => Some(stats),
            Err(err) => {
                println!("Failed to parse end-of-game stats: {:?}", err);
                None
            }
        },
        Err(err) => {
            println!("Failed to fetch end-of-game stats: {:?}", err);
            None
        }
    }
}

async fn get_friend_ids(remoting_client: &RESTClient) -> HashSet<u64> {
    let mut friend_ids = HashSet::new();

    match remoting_client
        .get("/lol-chat/v1/friends".to_string())
        .await
    {
        Ok(response) => {
            if let Ok(friends) = serde_json::from_value::<Vec<Friend>>(response) {
                for friend in friends {
                    if let Ok(id) = friend.summoner_id.parse::<u64>() {
                        friend_ids.insert(id);
                    }
                }
            }
        }
        Err(err) => {
            println!("Failed to fetch friends list: {:?}", err);
        }
    }

    friend_ids
}

fn collect_report_payloads(
    stats: &EndOfGameStats,
    friend_ids: &HashSet<u64>,
) -> Vec<(PlayerReportPayload, Option<String>, Option<String>)> {
    let mut payloads = Vec::new();
    let mut seen_puuids = HashSet::new();

    for team in &stats.teams {
        for player in &team.players {
            let Some(summoner_id) = player.summoner_id else {
                continue;
            };

            if summoner_id == stats.local_player.summoner_id {
                println!(
                    "{} is the current account, ignoring",
                    player
                        .summoner_name
                        .as_deref()
                        .unwrap_or("Unknown Summoner")
                );
                continue;
            }

            if friend_ids.contains(&summoner_id) {
                println!(
                    "{} is a friend, ignoring",
                    player
                        .summoner_name
                        .as_deref()
                        .unwrap_or("Unknown Summoner")
                );
                continue;
            }

            let Some(puuid) = player.puuid.clone() else {
                continue;
            };

            if !seen_puuids.insert(puuid.clone()) {
                continue;
            }

            let payload = PlayerReportPayload {
                game_id: stats.game_id,
                categories: REPORT_CATEGORIES
                    .iter()
                    .map(|category| category.to_string())
                    .collect(),
                offender_summoner_id: summoner_id,
                offender_puuid: puuid,
            };

            payloads.push((
                payload,
                player.summoner_name.clone(),
                player.champion_name.clone(),
            ));
        }
    }

    payloads
}
