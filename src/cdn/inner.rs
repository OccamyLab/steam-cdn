use reqwest::{Client, Response};
use std::sync::Arc;
use steam_vent::{
    proto::steammessages_clientserver_appinfo::{
        cmsg_client_picsproduct_info_request::AppInfo, CMsgClientPICSAccessTokenRequest,
        CMsgClientPICSAccessTokenResponse, CMsgClientPICSProductInfoRequest,
        CMsgClientPICSProductInfoResponse,
    },
    Connection, ConnectionTrait,
};
use tokio::sync::Mutex;

use crate::{
    web_api::{self, content_service::CDNServer},
    Error,
};

use super::depot_chunk;

#[derive(Debug)]
pub(crate) struct InnerClient {
    pub connection: Arc<Connection>,
    web_client: Client,
    pub servers: Arc<Mutex<Vec<(CDNServer, u32)>>>,
}

impl InnerClient {
    pub fn new(connection: Arc<Connection>) -> Self {
        Self {
            connection,
            web_client: Client::new(),
            servers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn cell_id(&self) -> u32 {
        self.connection.cell_id()
    }

    async fn pick_server(&self) -> Result<CDNServer, Error> {
        let mut servers = self.servers.lock().await;
        if servers.is_empty() || servers.iter().all(|(_, penalty)| *penalty > 0) {
            *servers = web_api::content_service::get_servers_for_steam_pipe(self.cell_id())
                .await?
                .into_iter()
                .map(|s| (s, 0))
                .collect();
        }

        if let Some((server, _)) = servers
            .iter()
            .find(|(server, penalty)| server.cell_id == self.cell_id() && *penalty == 0)
        {
            return Ok(server.clone());
        }

        servers
            .iter()
            .filter(|(s, _)| s.r#type == "SteamCache" || s.r#type == "CDN")
            .min_by_key(|(s, penalty)| (*penalty, s.weighted_load))
            .ok_or(Error::Network("no available cdn servers".to_string()))
            .map(|(server, _)| server.clone())
    }

    async fn server_penalty(&self, server: &CDNServer) {
        let mut servers = self.servers.lock().await;
        if let Some((_, penalty)) = servers.iter_mut().find(|(s, _)| s == server) {
            *penalty += 1;
        }
    }

    pub async fn get_product_info(
        &self,
        app_ids: Vec<u32>,
    ) -> Result<CMsgClientPICSProductInfoResponse, Error> {
        let tokens: CMsgClientPICSAccessTokenResponse = self
            .connection
            .job(CMsgClientPICSAccessTokenRequest {
                appids: app_ids,
                ..Default::default()
            })
            .await?;

        let product_info: CMsgClientPICSProductInfoResponse = self
            .connection
            .job(CMsgClientPICSProductInfoRequest {
                apps: tokens
                    .app_access_tokens
                    .into_iter()
                    .map(|app_token| AppInfo {
                        appid: app_token.appid,
                        access_token: app_token.access_token,
                        ..Default::default()
                    })
                    .collect::<Vec<AppInfo>>(),
                meta_data_only: Some(false),
                ..Default::default()
            })
            .await?;
        Ok(product_info)
    }

    pub async fn remote_cmd<C: AsRef<str>, A: AsRef<str>>(
        &self,
        command: C,
        args: A,
        manifest_request_code: Option<u64>,
    ) -> Result<Response, Error> {
        let server = self.pick_server().await?;
        let mut url = format!(
            "{}://{}:{}/{}/{}",
            if server.https { "https" } else { "http" },
            server.host,
            server.port,
            command.as_ref(),
            args.as_ref()
        );
        if let Some(manifest_request_code) = manifest_request_code {
            url.push('/');
            url.push_str(manifest_request_code.to_string().as_str());
        }

        let response = self.web_client.get(url).send().await?;
        if !response.status().is_success() {
            self.server_penalty(&server).await;
        }

        Ok(response)
    }

    pub async fn get_chunk(
        &self,
        depot_id: u32,
        depot_key: [u8; 32],
        chunk_id: String,
    ) -> Result<Vec<u8>, Error> {
        let response = self
            .remote_cmd("depot", format!("{depot_id}/chunk/{chunk_id}"), None)
            .await?;
        if !response.status().is_success() {
            return Err(Error::HttpStatus(response.status()));
        }

        let mut bytes = response.bytes().await?.to_vec();
        depot_chunk::decrypt_and_decompress(&mut bytes[..], depot_key).await
    }
}
