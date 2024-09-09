use std::{error::Error, sync::Arc};
use steam_cdn::CDNClient;
use steam_vent::{Connection, ServerList};
use tokio::fs::OpenOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_list = ServerList::discover().await?;
    let connection = Arc::new(Connection::anonymous(&server_list).await?);
    let cdn = CDNClient::discover(connection).await?;
    
    let app_id = 730;
    let depot_id = 2347771;
    let manifest_id = 9071851182114336641;
    
    //let depots = cdn.get_depots(vec![app_id]).await?;
    //println!("{:?}", depots);

    let depot_key = cdn.get_depot_decryption_key(app_id, depot_id).await?;
    let request_code = cdn
        .get_manifest_request_code(app_id, depot_id, manifest_id)
        .await?;
    let manifest = cdn
        .get_manifest(depot_id, manifest_id, Some(request_code), depot_key)
        .await?;

    for manifest_file in manifest.files() {
        if manifest_file.filename() != "client.dll" {
            continue;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(manifest_file.filename())
            .await?;
        manifest_file
            .download(depot_key.unwrap(), None, &mut file)
            .await?;
        break;
    }
    Ok(())
}
