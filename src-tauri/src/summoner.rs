use serde::{Deserialize, Serialize};
use serde_json::Value;
use shaco::rest::RESTClient;
use std::collections::HashSet;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summoner {
    pub account_id: i64,
    pub display_name: String,
    pub game_name: String,
    pub internal_name: String,
    pub name_change_flag: bool,
    pub percent_complete_for_next_level: i64,
    pub privacy: String,
    pub profile_icon_id: i64,
    pub puuid: String,
    pub reroll_points: RerollPoints,
    pub summoner_id: i64,
    pub summoner_level: i64,
    pub tag_line: String,
    pub unnamed: bool,
    pub xp_since_last_level: i64,
    pub xp_until_next_level: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RerollPoints {
    pub current_points: i64,
    pub max_rolls: i64,
    pub number_of_rolls: i64,
    pub points_cost_to_roll: i64,
    pub points_to_reroll: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Friend {
    #[serde(rename = "summonerId")]
    summoner_id: u64,
}

pub async fn get_current_summoner(app_client: &RESTClient) -> Summoner {
    let summoner: Summoner = serde_json::from_value(
        app_client
            .get("/lol-summoner/v1/current-summoner".to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    summoner
}

pub async fn get_friend_summoner_ids(remoting_client: &RESTClient) -> HashSet<u64> {
    let resp = remoting_client
        .get("/lol-chat/v1/friends".to_string())
        .await;

    match resp {
        Ok(value) => match serde_json::from_value::<Vec<Friend>>(value) {
            Ok(friends) => friends.into_iter().map(|f| f.summoner_id).collect(),
            Err(err) => {
                println!("Failed to parse friends list: {err:?}");
                HashSet::new()
            }
        },
        Err(err) => {
            println!("Failed to fetch friends list: {err:?}");
            HashSet::new()
        }
    }
}

pub async fn get_friend_puuids(remoting_client: &RESTClient) -> Vec<String> {
    let value: Value = remoting_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .unwrap_or_else(|err| {
            println!("Failed to fetch friends list: {err:?}");
            Value::Null
        });

    extract_friend_puuids(&value)
}

pub fn extract_friend_puuids(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|friends| {
            friends
                .iter()
                .filter_map(|friend| friend.get("puuid").and_then(|v| v.as_str()))
                .map(|puuid| puuid.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}
