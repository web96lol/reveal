use crate::{summoner::get_friend_summoner_ids, AppConfig, ManagedDodgeState};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shaco::rest::RESTClient;
use std::{collections::HashSet, time::Duration};
use tauri::{AppHandle, Manager};

struct ProcessOutcome {
    game_id: u64,
    reports_sent: Vec<Player>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndOfGameStats {
    pub game_id: u64,
    pub local_player: LocalPlayer,
    pub teams: Vec<Team>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlayer {
    pub summoner_id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub players: Vec<Player>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub summoner_id: u64,
    pub summoner_name: String,
    pub champion_name: String,
    pub puuid: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerReportPayload {
    pub game_id: u64,
    pub categories: Vec<&'static str>,
    pub offender_summoner_id: u64,
    pub offender_puuid: String,
}

pub async fn handle_end_game_start(app_handle: &AppHandle, remoting_client: &RESTClient) {
    tokio::time::sleep(Duration::from_millis(500)).await;

    let cfg_state = app_handle.state::<AppConfig>();
    if !cfg_state.0.lock().await.auto_report {
        return;
    }

    let dodge_state = app_handle.state::<ManagedDodgeState>();
    let previous_game_id = {
        let mut dodge_state = dodge_state.0.lock().await;

        if dodge_state.is_reporting {
            return;
        }

        dodge_state.is_reporting = true;
        dodge_state.last_reported_game
    };

    let friend_ids = get_friend_summoner_ids(remoting_client).await;

    let outcome = process_end_game(remoting_client, previous_game_id, friend_ids).await;

    {
        let mut dodge_state = dodge_state.0.lock().await;
        dodge_state.is_reporting = false;

        if let Some(outcome) = outcome.as_ref() {
            dodge_state.last_reported_game = Some(outcome.game_id);
        }
    }

    if let Some(outcome) = outcome {
        if !outcome.reports_sent.is_empty() {
            let _ = app_handle.emit_all("end_game_reports_sent", &outcome.reports_sent);
        }
    }
}

async fn process_end_game(
    remoting_client: &RESTClient,
    previous_game_id: Option<u64>,
    friend_ids: HashSet<u64>,
) -> Option<ProcessOutcome> {
    let stats_value = match remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("Failed to fetch end of game stats: {err:?}");
            return None;
        }
    };

    let stats: EndOfGameStats = match serde_json::from_value(stats_value) {
        Ok(stats) => stats,
        Err(err) => {
            println!("Failed to parse end of game stats: {err:?}");
            return None;
        }
    };

    if previous_game_id.map_or(false, |id| id == stats.game_id) {
        return None;
    }

    let current_player_id = stats.local_player.summoner_id;
    let game_id = stats.game_id;
    let mut report_tasks = Vec::new();

    for team in stats.teams.iter() {
        for player in team.players.iter() {
            if player.summoner_id == current_player_id {
                continue;
            }

            if friend_ids.contains(&player.summoner_id) {
                continue;
            }

            let client = remoting_client.clone();
            let player_clone = player.clone();

            report_tasks
                .push(async move { handle_player_report(client, game_id, player_clone).await });
        }
    }

    let results = join_all(report_tasks).await;
    let reports_sent: Vec<Player> = results.into_iter().flatten().collect();

    Some(ProcessOutcome {
        game_id,
        reports_sent,
    })
}

async fn handle_player_report(
    remoting_client: RESTClient,
    game_id: u64,
    player: Player,
) -> Option<Player> {
    let payload = PlayerReportPayload {
        game_id,
        categories: vec![
            "NEGATIVE_ATTITUDE",
            "VERBAL_ABUSE",
            "LEAVING_AFK",
            "ASSISTING_ENEMY_TEAM",
            "HATE_SPEECH",
            "THIRD_PARTY_TOOLS",
            "INAPPROPRIATE_NAME",
        ],
        offender_summoner_id: player.summoner_id,
        offender_puuid: player.puuid.clone(),
    };

    let body = Value::Array(vec![serde_json::to_value(&payload).unwrap()]);

    match remoting_client
        .post(
            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
            body,
        )
        .await
    {
        Ok(_) => Some(player),
        Err(err) => {
            println!(
                "Failed to send report for {}#{} ({:?}): {:?}",
                player.summoner_name, player.champion_name, player.summoner_id, err
            );
            None
        }
    }
}
