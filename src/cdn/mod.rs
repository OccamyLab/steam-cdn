use depot::AppDepots;
use inner::InnerClient;
use manifest::DepotManifest;
use std::sync::Arc;
use steam_vent::{
    proto::{
        steammessages_clientserver_2::{
            CMsgClientGetDepotDecryptionKey, CMsgClientGetDepotDecryptionKeyResponse,
        },
        steammessages_contentsystem_steamclient::CContentServerDirectory_GetManifestRequestCode_Request,
    },
    Connection, ConnectionTrait,
};

use crate::{error::Error, web_api};

pub mod depot;
pub mod depot_chunk;
pub mod inner;
pub mod manifest;

pub const MANIFEST_VERSION: usize = 5;

#[derive(Debug)]
pub struct CDNClient {
    inner: Arc<InnerClient>,
}

impl CDNClient {
    pub async fn discover(connection: Arc<Connection>) -> Result<Self, Error> {
        let mut inner = InnerClient::new(connection);
        inner.servers =
            web_api::content_service::get_servers_for_steam_pipe(inner.cell_id()).await?;
        inner
            .servers
            .sort_by(|a, b| a.weighted_load.cmp(&b.weighted_load));
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    // tbd: should be renamed
    pub async fn get_depots(&self, app_ids: Vec<u32>) -> Result<Vec<AppDepots>, Error> {
        let product_info = self.inner.get_product_info(app_ids).await?;
        let mut apps_depots: Vec<AppDepots> = Vec::new();

        for app in product_info.apps {
            let mut app_depots = AppDepots::new(app.appid());
            app_depots.vdf_parse(app.buffer())?;
            apps_depots.push(app_depots);
        }

        Ok(apps_depots)
    }

    pub async fn get_depot_decryption_key(
        &self,
        app_id: u32,
        depot_id: u32,
    ) -> Result<Option<[u8; 32]>, Error> {
        let response: CMsgClientGetDepotDecryptionKeyResponse = self
            .inner
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
        self.inner
            .connection
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

    pub async fn get_manifest(
        &self,
        depot_id: u32,
        manifest_id: u64,
        request_code: Option<u64>,
        depot_key: Option<[u8; 32]>,
    ) -> Result<DepotManifest, Error> {
        let bytes = self
            .inner
            .remote_cmd(
                "depot",
                format!("{depot_id}/manifest/{manifest_id}/{MANIFEST_VERSION}"),
                request_code,
            )
            .await?
            .bytes()
            .await?;

        let mut manifest = DepotManifest::deserialize(self.inner.clone(), &bytes[..])?;
        if manifest.filenames_encrypted() {
            if let Some(key) = depot_key {
                manifest.decrypt_filenames(key)?;
            }
        }

        Ok(manifest)
    }
}
