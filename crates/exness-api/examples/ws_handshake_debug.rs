//! 基于 HTTP/1.1 的原始 WebSocket 握手，用于检查服务端响应头
//! （曾用于确认服务端返回的 `Sec-WebSocket-Protocol`）。只读。

use anyhow::Context;
use base64::Engine;
use exness_api::{Client, ClientConfig, Credentials, WsStream};
use url::Url;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let account_id = std::env::var("EXNESS_ACCOUNT_ID")?;
    let creds = Credentials::new(std::env::var("EXNESS_API_KEY")?, &std::env::var("EXNESS_PRIVATE_KEY")?)?;
    let cfg = ClientConfig {
        gateway_url: std::env::var("EXNESS_GATEWAY_URL").ok().map(|s| Url::parse(&s)).transpose()?,
        ..Default::default()
    };
    let client = Client::new(creds, account_id, cfg)?;
    client.ensure_access_point().await?;

    for stream in [WsStream::Ticks, WsStream::Events] {
        let mut url = client.ws_url(stream)?;
        url.set_scheme("https").unwrap();
        let headers = client.sign_ws_handshake(stream)?;
        let key = base64::engine::general_purpose::STANDARD.encode(rand::random::<[u8; 16]>());

        let http = reqwest::Client::builder().http1_only().use_rustls_tls().build()?;
        let mut rb = http
            .get(url.clone())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", key);
        for (n, v) in headers.iter() {
            rb = rb.header(n, v);
        }
        let resp = rb.send().await.with_context(|| format!("handshake {url}"))?;
        println!("\n== {stream:?} {url}\nHTTP {}", resp.status());
        for (n, v) in resp.headers() {
            println!("  {n}: {}", v.to_str().unwrap_or("<bin>"));
        }
        if !resp.status().is_informational() && resp.status().as_u16() != 101 {
            let body = resp.text().await.unwrap_or_default();
            println!("  body: {body}");
        }
    }
    Ok(())
}
