use gl_server::{
    config::ServerConfig,
    crypto::CredentialCrypto,
    db::{init_pool, repository::AccountRepository},
};
use exness_api::{Client, ClientConfig, Credentials};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ServerConfig::from_file("config.toml")?;
    let db = init_pool(&config.database.url, 1).await?;
    let crypto = CredentialCrypto::new(&config.crypto.master_key)?;

    let account = AccountRepository::find_by_account_id(&db, "18000069657")
        .await?
        .expect("Account 18000069657 not found");

    let api_key = crypto.decrypt(&account.encrypted_api_key)?;
    let private_key = crypto.decrypt(&account.encrypted_private_key)?;

    let creds = Credentials::new(&api_key, &private_key)?;
    let cfg = ClientConfig {
        gateway_url: url::Url::parse(&account.gateway_url).ok(),
        ..Default::default()
    };
    let client = Arc::new(Client::new(creds, &account.account_id, cfg)?);
    client.ensure_access_point().await?;

    println!("=== RAW ACCOUNT DETAILS ===");
    match client.get_raw(&format!("/v1/configuration/accounts/{}/account", account.account_id)).await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(e) => println!("raw account error: {}", e),
    }

    println!("=== SNAPSHOT ===");
    match client.snapshot().await {
        Ok(snap) => println!("{:#?}", snap),
        Err(e) => println!("snapshot error: {}", e),
    }

    println!("=== RAW SNAPSHOT PATH ===");
    match client.get_raw(&format!("/v1/trading/accounts/{}/snapshot", account.account_id)).await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(e) => println!("raw snapshot error: {}", e),
    }

    println!("=== RAW POSITIONS PATH ===");
    match client.get_raw(&format!("/v1/trading/accounts/{}/positions", account.account_id)).await {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
        Err(e) => println!("raw positions error: {}", e),
    }

    Ok(())
}
