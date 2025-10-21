use serde_json::Value;
use shaco::rest::RESTClient;
use std::collections::HashSet;

pub async fn handle_end_of_game(client: &RESTClient) {
    let stats: Value = match client
        .get("/lol-end-of-game/v1/eog-stats-block".to_string())
        .await
    {
        Ok(value) => value,
        Err(_) => return,
    };

    let Some(game_id) = stats.get("gameId").and_then(|v| v.as_u64()) else {
        return;
    };

    let Some(local_id) = stats
        .get("localPlayer")
        .and_then(|player| player.get("summonerId"))
        .and_then(|value| value.as_u64())
    else {
        return;
    };

    let friends = fetch_friend_ids(client).await;

    if let Some(teams) = stats.get("teams").and_then(|value| value.as_array()) {
        for team in teams {
            if let Some(players) = team.get("players").and_then(|value| value.as_array()) {
                for player in players {
                    let Some(summoner_id) =
                        player.get("summonerId").and_then(|value| value.as_u64())
                    else {
                        continue;
                    };

                    if summoner_id == local_id || friends.contains(&summoner_id) {
                        continue;
                    }

                    let Some(puuid) = player.get("puuid").and_then(|value| value.as_str()) else {
                        continue;
                    };

                    let payload = serde_json::json!({
                        "gameId": game_id,
                        "categories": [
                            "NEGATIVE_ATTITUDE",
                            "VERBAL_ABUSE",
                            "LEAVING_AFK",
                            "ASSISTING_ENEMY_TEAM",
                            "HATE_SPEECH",
                            "THIRD_PARTY_TOOLS",
                            "INAPPROPRIATE_NAME"
                        ],
                        "offenderSummonerId": summoner_id,
                        "offenderPuuid": puuid
                    });

                    let _ = client
                        .post(
                            "/lol-player-report-sender/v1/end-of-game-reports".to_string(),
                            payload,
                        )
                        .await;
                }
            }
        }
    }
}

async fn fetch_friend_ids(client: &RESTClient) -> HashSet<u64> {
    match client.get("/lol-chat/v1/friends".to_string()).await {
        Ok(Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| entry.get("summonerId").and_then(|id| id.as_u64()))
            .collect(),
        _ => HashSet::new(),
    }
}
