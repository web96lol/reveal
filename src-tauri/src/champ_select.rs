use crate::{lobby, region::RegionInfo, utils::display_champ_select, Config};
use shaco::rest::RESTClient;
use tauri::{AppHandle, Manager};

pub async fn handle_champ_select_start(
    app_client: &RESTClient,
    _remoting_client: &RESTClient,
    config: &Config,
    app_handle: &AppHandle,
) {
    let team = lobby::get_lobby_info(app_client).await;
    let region_info: RegionInfo = serde_json::from_value(
        app_client
            .get("/riotclient/region-locale".to_string())
            .await
            .unwrap(),
    )
    .unwrap();

    app_handle.emit_all("champ_select_started", &team).unwrap();

    if config.auto_open {
        let region = match region_info.web_region.as_str() {
            "SG2" => "SG",
            _ => &region_info.web_region,
        };

        display_champ_select(&team, region, &config.multi_provider);
    }
}
