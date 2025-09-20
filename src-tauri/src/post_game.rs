use crate::summoner::get_current_summoner;
use crate::summoner::Summoner;
use serde::{Deserialize, Serialize};
use serde_json::json;
use shaco::rest::RESTClient;
use std::collections::HashSet;
use tokio::sync::Mutex;

const REPORT_CATEGORIES: &[&str] = &[
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

#[derive(Default)]
pub struct PostGameState {
    pub cached_friends: Option<HashSet<String>>,
    pub last_processed_game_id: Option<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerActionSummary {
    pub puuid: String,
    pub summoner_id: i64,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub summoner_name: Option<String>,
    pub report_sent: bool,
    pub categories: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostGameSummary {
    pub game_id: i64,
    pub players: Vec<PlayerActionSummary>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FriendInfo {
    pub puuid: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EogStatsBlock {
    #[serde(rename = "gameId")]
    pub game_id: i64,
    pub teams: Vec<EogTeam>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EogTeam {
    pub players: Vec<EogPlayer>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EogPlayer {
    pub puuid: String,
    pub summoner_id: i64,
    pub summoner_name: Option<String>,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
    pub riot_id_game_name: Option<String>,
    pub riot_id_tagline: Option<String>,
    pub team_id: Option<i64>,
    pub champion_id: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameflowSession {
    pub phase: Option<String>,
    pub game_data: Option<GameflowGameData>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct GameflowGameData {
    pub game_id: Option<i64>,
}

pub async fn process_last_game(
    state: &Mutex<PostGameState>,
    app_client: &RESTClient,
    remoting_client: &RESTClient,
) -> Result<PostGameSummary, String> {
    let mut cached_friends;
    let last_processed_game_id;

    {
        let state_guard = state.lock().await;
        cached_friends = state_guard.cached_friends.clone();
        last_processed_game_id = state_guard.last_processed_game_id;
    }

    if cached_friends.is_none() {
        let fetched = fetch_friend_ids(app_client).await?;
        let mut state_guard = state.lock().await;
        state_guard.cached_friends = Some(fetched.clone());
        cached_friends = Some(fetched);
    }

    let friend_ids = cached_friends.unwrap();

    let eog_value = remoting_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
        .map_err(|err| format!("Failed to fetch end of game stats: {:?}", err))?;

    if eog_value.is_null() {
        return Err("No end of game stats available".to_string());
    }

    let eog: EogStatsBlock = serde_json::from_value(eog_value.clone())
        .map_err(|err| format!("Failed to parse end of game stats: {:?}", err))?;

    if let Some(last) = last_processed_game_id {
        if last == eog.game_id {
            return Err("Last game already processed".to_string());
        }
    }

    let session = remoting_client
        .get("/lol-gameflow/v1/session".to_string())
        .await
        .map_err(|err| format!("Failed to fetch gameflow session: {:?}", err))?;

    let _session: GameflowSession = serde_json::from_value(session.clone())
        .map_err(|err| format!("Failed to parse gameflow session: {:?}", err))?;

    let summoner = get_current_summoner(remoting_client).await;
    let self_puuid = summoner.puuid.clone();

    let mut targets: Vec<(EogPlayer, PlayerActionSummary)> = Vec::new();
    for team in &eog.teams {
        for player in &team.players {
            if player.puuid.is_empty() {
                continue;
            }

            if player.puuid == self_puuid {
                continue;
            }

            if friend_ids.contains(&player.puuid) {
                continue;
            }

            targets.push((
                player.clone(),
                PlayerActionSummary {
                    puuid: player.puuid.clone(),
                    summoner_id: player.summoner_id,
                    game_name: preferred_game_name(player),
                    tag_line: preferred_tag_line(player),
                    summoner_name: player.summoner_name.clone(),
                    report_sent: false,
                    categories: REPORT_CATEGORIES
                        .iter()
                        .map(|category| category.to_string())
                        .collect(),
                },
            ));
        }
    }

    if targets.is_empty() {
        let mut state_guard = state.lock().await;
        state_guard.last_processed_game_id = Some(eog.game_id);
        return Ok(PostGameSummary {
            game_id: eog.game_id,
            players: vec![],
        });
    }

    let offenders_payload: Vec<_> = targets
        .iter()
        .map(|(player, _)| build_offender_payload(player, &summoner, eog.game_id))
        .collect();

    if !offenders_payload.is_empty() {
        match remoting_client
            .post(
                "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                json!({
                    "gameId": eog.game_id,
                    "offenders": offenders_payload,
                }),
            )
            .await
        {
            Ok(_) => {
                for (_, summary) in targets.iter_mut() {
                    summary.report_sent = true;
                }
            }
            Err(err) => {
                println!("Failed to send reports for last game: {:?}", err);
            }
        }
    }

    let players = targets.into_iter().map(|(_, summary)| summary).collect();

    let mut state_guard = state.lock().await;
    state_guard.last_processed_game_id = Some(eog.game_id);

    Ok(PostGameSummary {
        game_id: eog.game_id,
        players,
    })
}

async fn fetch_friend_ids(app_client: &RESTClient) -> Result<HashSet<String>, String> {
    let friends = app_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .map_err(|err| format!("Failed to fetch friends list: {:?}", err))?;

    let friends: Vec<FriendInfo> = serde_json::from_value(friends)
        .map_err(|err| format!("Failed to parse friends list: {:?}", err))?;

    Ok(friends.into_iter().map(|f| f.puuid).collect())
}

fn preferred_game_name(player: &EogPlayer) -> Option<String> {
    player
        .game_name
        .clone()
        .or_else(|| player.riot_id_game_name.clone())
        .or_else(|| player.summoner_name.clone())
}

fn preferred_tag_line(player: &EogPlayer) -> Option<String> {
    player
        .tag_line
        .clone()
        .or_else(|| player.riot_id_tagline.clone())
}

fn build_offender_payload(
    player: &EogPlayer,
    reporter: &Summoner,
    game_id: i64,
) -> serde_json::Value {
    json!({
        "gameId": game_id,
        "offenderPuuid": player.puuid,
        "offenderSummonerId": player.summoner_id,
        "reportedSummonerId": reporter.summoner_id,
        "reportedSummonerName": reporter.game_name,
        "reportedPuuid": reporter.puuid,
        "reportedTeamId": player.team_id,
        "reportedChampionId": player.champion_id,
        "comment": "",
        "reasonIds": Vec::<String>::new(),
        "categories": REPORT_CATEGORIES,
    })
}
