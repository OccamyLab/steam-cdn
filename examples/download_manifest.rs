use std::{error::Error, sync::Arc};
use steam_cdn::CDNClient;
use steam_vent::{Connection, ServerList};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let server_list = ServerList::discover().await?;
    let connection = Arc::new(Connection::anonymous(server_list).await?);
    let cdn = CDNClient::discover(connection).await?;
    // let depots = cdn.get_depots(vec![730]).await?;
    // println!("{:?}", depots);

    let depot_key = cdn.get_depot_decryption_key(730, 2347771).await?;
    let request_code = cdn
        .get_manifest_request_code(730, 2347771, 9071851182114336641)
        .await?;
    let manifest = cdn
        .get_manifest(2347771, 9071851182114336641, Some(request_code), depot_key)
        .await?;

    for file in manifest.files() {
        println!("{} {:#?}", file.filename(), file.sha_content());
    }
    Ok(())
}
