use std::net::SocketAddr;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let addr = test_server::spawn(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    println!("test-server listening on http://{addr}  (try /file/104857600?rate=1048576)");
    tokio::signal::ctrl_c().await
}
