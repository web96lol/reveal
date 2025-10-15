use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shaco::rest::RESTClient;

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

pub async fn get_current_summoner(remoting_client: &RESTClient) -> Summoner {
    try_get_current_summoner(remoting_client)
        .await
        .expect("failed to load current summoner")
}

pub async fn try_get_current_summoner(remoting_client: &RESTClient) -> Result<Summoner> {
    let response = remoting_client
        .get("/lol-summoner/v1/current-summoner".to_string())
        .await
        .context("failed to fetch current summoner")?;

    serde_json::from_value(response).context("failed to deserialize current summoner")
}

pub async fn get_friend_puuids(remoting_client: &RESTClient) -> Vec<String> {
    try_get_friend_puuids(remoting_client)
        .await
        .expect("failed to load friend list")
}

pub async fn try_get_friend_puuids(remoting_client: &RESTClient) -> Result<Vec<String>> {
    let value = remoting_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .context("failed to fetch friends list")?;

    Ok(extract_friend_puuids(&value))
}

fn extract_friend_puuids(value: &Value) -> Vec<String> {
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
