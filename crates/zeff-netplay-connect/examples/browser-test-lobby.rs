use anyhow::{Result, ensure};
use zeff_netplay_lobby::{Lobby, config::Config};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let bind = args.next().unwrap_or_else(|| "127.0.0.1:47180".into());
    let origin = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:47181".into());
    let mut config = Config::public();
    config.bind = bind.parse()?;
    ensure!(
        config.bind.ip().is_loopback(),
        "test lobby must use loopback"
    );
    ensure!(
        origin.starts_with("http://127.0.0.1:") || origin.starts_with("http://localhost:"),
        "test origin must use loopback"
    );
    config.origins = vec![origin];
    config.ice_servers.clear();
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, Lobby::new(config).router()).await?;
    Ok(())
}
