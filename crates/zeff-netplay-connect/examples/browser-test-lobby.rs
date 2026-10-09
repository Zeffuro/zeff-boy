use anyhow::{Result, ensure};
use zeff_netplay_lobby::{Lobby, config::Config};

#[path = "browser-test-lobby/mixed.rs"]
mod mixed;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let bind = args.next().unwrap_or_else(|| "127.0.0.1:47180".into());
    let origin = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:47181".into());
    let run_id = args.next().unwrap_or_else(|| "local".into());
    ensure!(run_id.len() <= 32, "test run identity too long");
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
    config.origins = vec![origin.clone()];
    config.ice_servers.clear();
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let url = format!("ws://{}/v1/ws", listener.local_addr()?);
    let lobby = Lobby::new(config);
    let router = lobby
        .router()
        .merge(mixed::router(lobby.clone(), url, origin, run_id));
    axum::serve(listener, router).await?;
    Ok(())
}
