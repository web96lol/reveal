use crate::{summoner::get_friend_ids, Config};
use serde::{Deserialize, Serialize};
use shaco::rest::RESTClient;
use std::collections::HashSet;
use tauri::{AppHandle, Manager};

#[derive(Deserialize)]
pub struct EndOfGameStats {
    #[serde(rename = "gameId")]
    pub game_id: u64,
    #[serde(rename = "localPlayer")]
    pub local_player: LocalPlayer,
    pub teams: Vec<Team>,
}

#[derive(Deserialize)]
pub struct LocalPlayer {
    #[serde(rename = "summonerId")]
    pub summoner_id: u64,
}

#[derive(Deserialize)]
pub struct Team {
    pub players: Vec<Player>,
}

#[derive(Deserialize)]
pub struct Player {
    #[serde(rename = "summonerId")]
    pub summoner_id: u64,
    #[serde(rename = "summonerName")]
    pub summoner_name: String,
    #[serde(rename = "championName")]
    pub champion_name: String,
    pub puuid: String,
}

#[derive(Serialize)]
pub struct PlayerReportPayload {
    #[serde(rename = "gameId")]
    pub game_id: u64,
    pub categories: Vec<String>,
    #[serde(rename = "offenderSummonerId")]
    pub offender_summoner_id: u64,
    #[serde(rename = "offenderPuuid")]
    pub offender_puuid: String,
}

pub async fn handle_end_game(
    remoting_client: &RESTClient,
    config: &Config,
    app_handle: &AppHandle,
    last_game_id: Option<u64>,
) -> Option<u64> {
    if !config.auto_report {
        return None;
    }

    let friend_ids = get_friend_ids(remoting_client).await;

    let stats_value = match remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(err) => {
            println!("Failed to fetch end of game stats: {:?}", err);
            return None;
        }
    };

    let stats = match serde_json::from_value::<EndOfGameStats>(stats_value) {
        Ok(stats) => stats,
        Err(err) => {
            println!("Failed to parse end of game stats: {:?}", err);
            return None;
        }
    };

    if last_game_id.map(|id| id == stats.game_id).unwrap_or(false) {
        return None;
    }

    let current_player = stats.local_player.summoner_id;

    for team in &stats.teams {
        for player in &team.players {
            handle_player_report(
                remoting_client,
                player,
                stats.game_id,
                current_player,
                &friend_ids,
            )
            .await;
        }
    }

    if let Err(err) = app_handle.emit_all("end_game_reports_sent", stats.game_id) {
        println!("Failed to emit end_game_reports_sent event: {:?}", err);
    }

    Some(stats.game_id)
}

async fn handle_player_report(
    remoting_client: &RESTClient,
    player: &Player,
    game_id: u64,
    current_player_id: u64,
    friend_ids: &HashSet<u64>,
) {
    if player.summoner_id == current_player_id {
        return;
    }

    if friend_ids.contains(&player.summoner_id) {
        return;
    }

    let payload = PlayerReportPayload {
        game_id,
        categories: vec![
            "NEGATIVE_ATTITUDE".to_string(),
            "VERBAL_ABUSE".to_string(),
            "LEAVING_AFK".to_string(),
            "ASSISTING_ENEMY_TEAM".to_string(),
            "HATE_SPEECH".to_string(),
            "THIRD_PARTY_TOOLS".to_string(),
            "INAPPROPRIATE_NAME".to_string(),
        ],
        offender_summoner_id: player.summoner_id,
        offender_puuid: player.puuid.clone(),
    };

    let _ = remoting_client
        .post(
            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
            serde_json::to_value(payload).unwrap(),
        )
        .await;
}
