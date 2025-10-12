use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::Value;
use shaco::rest::RESTClient;
use std::collections::HashSet;
use tokio::sync::Mutex;

const REPORT_CATEGORIES: [&str; 7] = [
    "NEGATIVE_ATTITUDE",
    "VERBAL_ABUSE",
    "LEAVING_AFK",
    "ASSISTING_ENEMY_TEAM",
    "HATE_SPEECH",
    "THIRD_PARTY_TOOLS",
    "INAPPROPRIATE_NAME",
];

#[derive(Default, Debug)]
pub struct EndGameState {
    pub last_game_id: Option<u64>,
    pub local_summoner_id: Option<u64>,
    pub friends_loaded: bool,
    pub friend_ids: HashSet<u64>,
}

impl EndGameState {
    pub fn reset(&mut self) {
        self.last_game_id = None;
        self.local_summoner_id = None;
        self.friends_loaded = false;
        self.friend_ids.clear();
    }
}

pub struct ManagedEndGameState(pub Mutex<EndGameState>);

impl Default for ManagedEndGameState {
    fn default() -> Self {
        Self(Mutex::new(EndGameState::default()))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EndOfGameReportPayload {
    game_id: u64,
    categories: Vec<&'static str>,
    offender_summoner_id: u64,
    offender_puuid: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ReportTarget {
    summoner_id: u64,
    puuid: String,
    summoner_name: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ProcessedGame {
    game_id: u64,
    targets: Vec<ReportTarget>,
}

pub async fn handle_end_of_game(state: &ManagedEndGameState, app_client: &RESTClient) {
    if let Err(err) = process_end_of_game(state, app_client).await {
        println!("End of game handler error: {}", err);
    }
}

async fn process_end_of_game(state: &ManagedEndGameState, app_client: &RESTClient) -> Result<()> {
    if needs_friend_refresh(state).await {
        if let Err(err) = fetch_and_cache_friends(state, app_client).await {
            println!("Failed to refresh friends list: {}", err);
        }
    }

    if !friends_ready(state).await {
        return Ok(());
    }

    let stats = app_client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
        .map_err(|err| anyhow!("failed to fetch end of game stats: {}", err))?;

    let processed = {
        let mut guard = state.0.lock().await;
        match collect_report_targets(&mut guard, &stats)? {
            Some(game) => game,
            None => return Ok(()),
        }
    };

    if processed.targets.is_empty() {
        return Ok(());
    }

    for target in processed.targets {
        let payload = EndOfGameReportPayload {
            game_id: processed.game_id,
            categories: REPORT_CATEGORIES.to_vec(),
            offender_summoner_id: target.summoner_id,
            offender_puuid: target.puuid.clone(),
        };

        let payload = match serde_json::to_value(&payload) {
            Ok(value) => value,
            Err(err) => {
                println!(
                    "Failed to encode auto report payload for {} in game {}: {}",
                    target.summoner_id, processed.game_id, err
                );
                continue;
            }
        };

        match app_client
            .post(
                "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                payload,
            )
            .await
        {
            Ok(_) => {
                let label = target
                    .summoner_name
                    .unwrap_or_else(|| target.summoner_id.to_string());
                println!("Auto reported {} for game {}", label, processed.game_id);
            }
            Err(err) => {
                println!(
                    "Failed to auto report {} in game {}: {}",
                    target.summoner_id, processed.game_id, err
                );
            }
        }
    }

    Ok(())
}

async fn needs_friend_refresh(state: &ManagedEndGameState) -> bool {
    let guard = state.0.lock().await;
    !guard.friends_loaded
}

async fn friends_ready(state: &ManagedEndGameState) -> bool {
    let guard = state.0.lock().await;
    guard.friends_loaded
}

async fn fetch_and_cache_friends(
    state: &ManagedEndGameState,
    app_client: &RESTClient,
) -> Result<()> {
    let response = app_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .map_err(|err| anyhow!("failed to fetch friends: {}", err))?;

    let friends = response
        .as_array()
        .context("friends response is not an array")?;

    let mut ids = HashSet::new();
    for friend in friends {
        if let Some(id_value) = friend.get("summonerId") {
            if let Some(id) = parse_u64(id_value) {
                ids.insert(id);
            }
        }
    }

    let mut guard = state.0.lock().await;
    guard.friend_ids = ids;
    guard.friends_loaded = true;
    Ok(())
}

fn collect_report_targets(
    state: &mut EndGameState,
    stats: &Value,
) -> Result<Option<ProcessedGame>> {
    let game_id = stats
        .get("gameId")
        .and_then(parse_u64)
        .context("missing gameId in stats")?;

    if let Some(last_game) = state.last_game_id {
        if last_game == game_id {
            return Ok(None);
        }
    }

    state.last_game_id = Some(game_id);

    if state.local_summoner_id.is_none() {
        if let Some(local_id) = stats
            .get("localPlayer")
            .and_then(|local| local.get("summonerId"))
            .and_then(parse_u64)
        {
            state.local_summoner_id = Some(local_id);
        }
    }

    let local_id = state
        .local_summoner_id
        .context("missing local summoner id")?;

    let teams = stats
        .get("teams")
        .and_then(|teams| teams.as_array())
        .context("missing teams in end of game stats")?;

    let mut targets = Vec::new();
    for team in teams {
        let Some(players) = team
            .get("players")
            .or_else(|| team.get("playerStats"))
            .and_then(|players| players.as_array())
        else {
            continue;
        };

        for player in players {
            let Some(summoner_id) = player.get("summonerId").and_then(parse_u64) else {
                continue;
            };

            if summoner_id == local_id {
                continue;
            }

            if state.friend_ids.contains(&summoner_id) {
                continue;
            }

            let Some(puuid) = player.get("puuid").and_then(|p| p.as_str()) else {
                continue;
            };

            let name = player
                .get("summonerName")
                .and_then(|name| name.as_str())
                .map(|s| s.to_string());

            targets.push(ReportTarget {
                summoner_id,
                puuid: puuid.to_string(),
                summoner_name: name,
            });
        }
    }

    Ok(Some(ProcessedGame { game_id, targets }))
}

fn parse_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dedupes_by_game_id() {
        let stats = json!({
            "gameId": 123_u64,
            "localPlayer": { "summonerId": 55_u64 },
            "teams": [
                {"players": [
                    {"summonerId": 55_u64, "summonerName": "Self", "puuid": "self"},
                    {"summonerId": 77_u64, "summonerName": "EnemyA", "puuid": "enemy_a"}
                ]},
                {"players": [
                    {"summonerId": 88_u64, "summonerName": "EnemyB", "puuid": "enemy_b"}
                ]}
            ]
        });

        let mut state = EndGameState::default();
        state.friends_loaded = true;

        let first = collect_report_targets(&mut state, &stats).unwrap();
        assert!(first.is_some());
        let processed = first.unwrap();
        assert_eq!(processed.targets.len(), 2);

        let second = collect_report_targets(&mut state, &stats).unwrap();
        assert!(second.is_none());
    }

    #[test]
    fn filters_friends_and_self() {
        let stats = json!({
            "gameId": 321_u64,
            "localPlayer": { "summonerId": 11_u64 },
            "teams": [
                {"players": [
                    {"summonerId": 11_u64, "summonerName": "Self", "puuid": "self"},
                    {"summonerId": 44_u64, "summonerName": "Friend", "puuid": "friend"}
                ]},
                {"players": [
                    {"summonerId": 55_u64, "summonerName": "Enemy", "puuid": "enemy"}
                ]}
            ]
        });

        let mut state = EndGameState::default();
        state.friends_loaded = true;
        state.friend_ids.insert(44);

        let processed = collect_report_targets(&mut state, &stats).unwrap().unwrap();

        assert_eq!(processed.targets.len(), 1);
        assert_eq!(processed.targets[0].summoner_id, 55);
    }

    #[test]
    fn parses_string_ids() {
        let stats = json!({
            "gameId": "999",
            "localPlayer": { "summonerId": "22" },
            "teams": [
                {"players": [
                    {"summonerId": "22", "summonerName": "Self", "puuid": "self"},
                    {"summonerId": "66", "summonerName": "Enemy", "puuid": "enemy"}
                ]}
            ]
        });

        let mut state = EndGameState::default();
        state.friends_loaded = true;

        let processed = collect_report_targets(&mut state, &stats).unwrap().unwrap();

        assert_eq!(processed.game_id, 999);
        assert_eq!(processed.targets.len(), 1);
        assert_eq!(processed.targets[0].summoner_id, 66);
    }

    #[test]
    fn report_payload_matches_csharp_schema() {
        let payload = EndOfGameReportPayload {
            game_id: 456,
            categories: REPORT_CATEGORIES.to_vec(),
            offender_summoner_id: 789,
            offender_puuid: "sample-puuid".to_string(),
        };

        let value = serde_json::to_value(&payload).unwrap();

        assert_eq!(value["gameId"], json!(456));
        assert_eq!(value["categories"], json!(REPORT_CATEGORIES));
        assert_eq!(value["offenderSummonerId"], json!(789));
        assert_eq!(value["offenderPuuid"], json!("sample-puuid"));
        assert!(value.get("reportSource").is_none());
    }
}
