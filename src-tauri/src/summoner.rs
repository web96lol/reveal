use serde::{Deserialize, Serialize};
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

pub async fn get_current_summoner(remoting_client: &RESTClient) -> Summoner {
    let summoner: Summoner = serde_json::from_value(
        remoting_client
            .get("/lol-summoner/v1/current-summoner".to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    summoner
}

#[derive(Deserialize)]
struct FriendEntry {
    #[serde(rename = "summonerId")]
    pub summoner_id: Option<u64>,
}

pub async fn get_friend_ids(remoting_client: &RESTClient) -> HashSet<u64> {
    match remoting_client
        .get("/lol-chat/v1/friends".to_string())
        .await
        .and_then(|value| serde_json::from_value::<Vec<FriendEntry>>(value).map_err(Into::into))
    {
        Ok(entries) => entries
            .into_iter()
            .filter_map(|entry| entry.summoner_id)
            .collect(),
        Err(err) => {
            println!("Failed to fetch friend ids: {:?}", err);
            HashSet::new()
        }
    }
}
