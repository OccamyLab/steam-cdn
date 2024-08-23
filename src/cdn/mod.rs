use depot::Depot;
use keyvalues_parser::Vdf;
use manifest::DepotManifest;
use reqwest::{Client, Response};
use std::{str, sync::Arc};
use steam_vent::{
    proto::{
        steammessages_clientserver_2::{
            CMsgClientGetDepotDecryptionKey, CMsgClientGetDepotDecryptionKeyResponse,
        },
        steammessages_clientserver_appinfo::{
            cmsg_client_picsproduct_info_request::AppInfo, CMsgClientPICSProductInfoRequest,
            CMsgClientPICSProductInfoResponse,
        },
        steammessages_contentsystem_steamclient::CContentServerDirectory_GetManifestRequestCode_Request,
    },
    Connection,
};

use crate::error::Error;

pub mod branch;
pub mod depot;
pub mod manifest;

pub const MANIFEST_VERSION: usize = 5;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ContentServer {
    pub r#type: String,
    pub source_id: u64,
    pub cell_id: Option<u32>,
    pub load: u32,
    pub weighted_load: u32,
    pub num_entries_in_client_list: u32,
    pub host: String,
    pub vhost: String,
    pub https_support: String,
    pub priority_class: u32,
}

#[derive(Debug, Deserialize)]
struct ContentServerDirectoryInner {
    pub servers: Vec<ContentServer>,
}

#[derive(Debug, Deserialize)]
struct ContentServerDirectory {
    pub response: ContentServerDirectoryInner,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct CDNServer {
    r#type: String,
    https: bool,
    host: String,
    vhost: String,
    port: u16,
    cell_id: u32,
    load: u32,
    weighted_load: u32,
}

pub struct CDNClient {
    connection: Arc<Connection>,
    web_client: Client,
    servers: Vec<CDNServer>,
}

impl CDNClient {
    pub fn new(connection: Arc<Connection>) -> Self {
        Self {
            connection,
            web_client: Client::new(),
            servers: Vec::new(),
        }
    }

    pub async fn discover(connection: Arc<Connection>) -> Result<Self, Error> {
        let mut cdn = Self::new(connection);
        cdn.servers = cdn.get_content_servers().await?;
        Ok(cdn)
    }

    async fn get_content_servers(&mut self) -> Result<Vec<CDNServer>, Error> {
        Ok(
            self.web_client.get("https://api.steampowered.com/IContentServerDirectoryService/GetServersForSteamPipe/v1/")
                .query(&[("cell_id", self.connection.cell_id())])
                .send().await?
                .json::<ContentServerDirectory>().await?
                .response
                .servers
                .into_iter()
                .map(|server| {
                    let https = server.https_support == "mandatory";
                    CDNServer {
                        r#type: server.r#type,
                        https,
                        host: server.host,
                        vhost: server.vhost,
                        port: if https { 443 } else { 80 },
                        cell_id: server.cell_id.unwrap_or(0),
                        load: server.load,
                        weighted_load: server.weighted_load,
                    }
                })
                .collect::<Vec<CDNServer>>()
        )
    }

    fn get_server(&self) -> Result<&CDNServer, Error> {
        match self
            .servers
            .iter()
            .find(|&server| server.cell_id == self.connection.cell_id())
        {
            Some(server) => Ok(server),
            None => self
                .servers
                .iter()
                .find(|server| server.r#type == "SteamCache" || server.r#type == "CDN")
                .ok_or(Error::Network("no available cdn servers".to_string())),
        }
    }

    async fn get_product_info(
        &self,
        app_ids: Vec<u32>,
    ) -> Result<CMsgClientPICSProductInfoResponse, Error> {
        let product_info: CMsgClientPICSProductInfoResponse = self
            .connection
            .job(CMsgClientPICSProductInfoRequest {
                apps: app_ids
                    .into_iter()
                    .map(|app_id| AppInfo {
                        appid: Some(app_id),
                        ..Default::default()
                    })
                    .collect::<Vec<AppInfo>>(),
                meta_data_only: Some(false),
                ..Default::default()
            })
            .await?;
        Ok(product_info)
    }

    pub async fn get_depots(&self, app_ids: Vec<u32>) -> Result<Vec<Depot>, Error> {
        let product_info = self.get_product_info(app_ids).await?;
        let mut depots = Vec::<Depot>::new();

        for app in product_info.apps {
            if let Ok(vdf) = str::from_utf8(app.buffer()) {
                let kv = Vdf::parse(vdf)?;
                let depots_map = &kv
                    .value
                    .get_obj()
                    .ok_or(Error::NoneOption)?
                    .get("depots")
                    .ok_or(Error::NoneOption)?
                    .get(0)
                    .ok_or(Error::NoneOption)?
                    .get_obj()
                    .ok_or(Error::NoneOption)?
                    .0;
                for (key, value) in depots_map {
                    if let Ok(depot_id) = key.parse::<u32>() {
                        let mut depot = Depot::new(app.appid(), depot_id);
                        depot.parse(value)?;
                        depots.push(depot);
                    }
                }
            }
        }

        Ok(depots)
    }

    pub async fn get_depot_decryption_key(
        &self,
        app_id: u32,
        depot_id: u32,
    ) -> Result<Option<[u8; 32]>, Error> {
        let response: CMsgClientGetDepotDecryptionKeyResponse = self
            .connection
            .job(CMsgClientGetDepotDecryptionKey {
                depot_id: Some(depot_id),
                app_id: Some(app_id),
                ..Default::default()
            })
            .await?;
        match response.depot_encryption_key {
            Some(bytes) if bytes.len() == 32 => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes[..]);
                Ok(Some(key))
            }
            Some(_) => Err(Error::Unexpected(
                "depot key has unexpected size".to_string(),
            )),
            None => Ok(None),
        }
    }

    pub async fn get_manifest_request_code(
        &self,
        app_id: u32,
        depot_id: u32,
        manifest_id: u64,
    ) -> Result<u64, Error> {
        self.connection
            .service_method(CContentServerDirectory_GetManifestRequestCode_Request {
                app_id: Some(app_id),
                depot_id: Some(depot_id),
                manifest_id: Some(manifest_id),
                ..Default::default()
            })
            .await?
            .manifest_request_code
            .ok_or(Error::Unexpected(
                "failed to get manifest request code".to_string(),
            ))
    }

    pub async fn remote_cmd<C: AsRef<str>, A: AsRef<str>>(
        &self,
        command: C,
        args: A,
        manifest_request_code: Option<u64>,
    ) -> Result<Response, Error> {
        let server = self.get_server()?;
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
        Ok(self.web_client.get(url).send().await?)
    }

    pub async fn get_manifest(
        &self,
        depot_id: u32,
        manifest_id: u64,
        request_code: Option<u64>,
        depot_key: Option<[u8; 32]>,
    ) -> Result<DepotManifest, Error> {
        let bytes = self
            .remote_cmd(
                "depot",
                format!("{depot_id}/manifest/{manifest_id}/{MANIFEST_VERSION}"),
                request_code,
            )
            .await?
            .bytes()
            .await?;

        let mut manifest = DepotManifest::try_from(&bytes[..])?;
        if manifest.filenames_encrypted() {
            if let Some(key) = depot_key {
                manifest.decrypt_filenames(key)?;
            }
        }

        Ok(manifest)
    }

    //pub async fn get_chunk(depot_id: u32, chunk_id: )
}
