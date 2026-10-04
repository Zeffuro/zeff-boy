use std::thread::{self, JoinHandle};
use std::time::Duration;
use zeff_netplay_lobby::{Lobby, config::Config};

pub(crate) struct TestLobby {
    pub(crate) url: String,
    pub(crate) lobby: Lobby,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl TestLobby {
    pub(crate) fn start() -> Self {
        let (ready, started) = std::sync::mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let mut config = Config::public();
                    config.ice_servers.clear();
                    let lobby = Lobby::new(config);
                    ready
                        .send((
                            format!("ws://{}/v1/ws", listener.local_addr().unwrap()),
                            lobby.clone(),
                        ))
                        .unwrap();
                    axum::serve(listener, lobby.router())
                        .with_graceful_shutdown(async {
                            let _ = stopped.await;
                        })
                        .await
                        .unwrap();
                });
        });
        let (url, lobby) = started.recv_timeout(Duration::from_secs(2)).unwrap();
        Self {
            url,
            lobby,
            stop: Some(stop),
            worker: Some(worker),
        }
    }
}

impl Drop for TestLobby {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.worker.take().unwrap().join().unwrap();
    }
}
