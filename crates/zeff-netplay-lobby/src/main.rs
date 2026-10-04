use anyhow::Result;
use zeff_netplay_lobby::{
    Lobby,
    config::Config,
    listener::{BoundedListener, ConnectionGate},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--healthcheck") {
        use std::io::{Read, Write};
        let mut address: std::net::SocketAddr = std::env::var("ZEFF_LOBBY_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()?;
        address.set_ip(if address.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        });
        let budget = std::time::Duration::from_secs(2);
        let mut stream = std::net::TcpStream::connect_timeout(&address, budget)?;
        stream.set_read_timeout(Some(budget))?;
        stream.set_write_timeout(Some(budget))?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut response = [0; 13];
        stream.read_exact(&mut response)?;
        anyhow::ensure!(&response == b"HTTP/1.1 200 ", "lobby is unhealthy");
        return Ok(());
    }
    let config = Config::from_env()?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    println!(
        "zeff-netplay-lobby {} listening on {} (signaling only)",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?
    );
    let listener = BoundedListener::new(
        listener,
        config.max_connections,
        config.room_ttl + std::time::Duration::from_secs(10),
    );
    axum::serve(
        listener,
        Lobby::new(config)
            .router()
            .into_make_service_with_connect_info::<ConnectionGate>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
