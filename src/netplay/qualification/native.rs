use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use zeff_netplay_lobby::{Lobby, config::Config};

use super::Progress;
use std::path::PathBuf;

mod worker;

#[derive(Default)]
pub(super) struct Control {
    progress: Mutex<Progress>,
    release: AtomicBool,
    stop: AtomicBool,
    cancel: AtomicBool,
}

struct Job {
    control: Arc<Control>,
    worker: JoinHandle<()>,
}

#[derive(Clone)]
struct Fixture {
    lobby: Lobby,
    url: String,
    origin: String,
    job: Arc<Mutex<Option<Job>>>,
    root: PathBuf,
}

fn allowed(fixture: &Fixture, headers: &HeaderMap) -> bool {
    headers
        .get("origin")
        .and_then(|header| header.to_str().ok())
        == Some(&fixture.origin)
}

fn response(fixture: &Fixture, status: StatusCode, text: String) -> Response {
    (
        status,
        [
            ("access-control-allow-origin", fixture.origin.as_str()),
            ("cache-control", "no-store"),
        ],
        text,
    )
        .into_response()
}

async fn start(State(fixture): State<Fixture>, headers: HeaderMap, body: String) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let options = match worker::Options::parse(&body) {
        Ok(options) => options,
        Err(error) => return response(&fixture, StatusCode::BAD_REQUEST, error.to_string()),
    };
    let (ready, started) = tokio::sync::oneshot::channel();
    {
        let mut job = fixture.job.lock().unwrap();
        if job.as_ref().is_some_and(|job| !job.worker.is_finished()) {
            return response(
                &fixture,
                StatusCode::CONFLICT,
                "native job still active".into(),
            );
        }
        if let Some(previous) = job.take() {
            previous.worker.join().unwrap();
        }
        let control = Arc::new(Control::default());
        let state = control.clone();
        let lobby = fixture.lobby.clone();
        let url = fixture.url.clone();
        let root = fixture.root.clone();
        let worker = std::thread::spawn(move || {
            if let Err(error) = worker::run(&options, &url, &lobby, &root, &state, ready) {
                state.progress.lock().unwrap().error = Some(format!("{error:#}"));
            }
            state.progress.lock().unwrap().finished = true;
        });
        *job = Some(Job { control, worker });
    }
    match tokio::time::timeout(Duration::from_secs(5), started).await {
        Ok(Ok(room)) => response(&fixture, StatusCode::OK, room),
        _ => response(
            &fixture,
            StatusCode::BAD_GATEWAY,
            "native start failed".into(),
        ),
    }
}

async fn status(State(fixture): State<Fixture>, headers: HeaderMap) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let job = fixture.job.lock().unwrap();
    let Some(job) = job.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut progress = job.control.progress.lock().unwrap().clone();
    progress.finished &= job.worker.is_finished();
    response(
        &fixture,
        StatusCode::OK,
        serde_json::to_string(&progress).unwrap(),
    )
}

async fn facts(State(fixture): State<Fixture>, headers: HeaderMap) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = (|| -> Result<String> {
        let mut facts = super::facts(crate::netplay::connect::executable_build()?);
        facts["certificate"] =
            serde_json::to_value(crate::netplay::compatibility::certificate_status()?)?;
        facts["games"] = serde_json::json!([0, 1, 3].map(|timing| {
            let bytes = super::media(timing);
            serde_json::json!({"source":const_hex::encode(zeff_firmware::sha256_bytes(&bytes)),
                "mapper":u16::from(bytes[6] >> 4) | u16::from(bytes[7] & 0xf0),
                "submapper":bytes[8] >> 4, "timing":if timing == 3 {2} else {timing}})
        }));
        Ok(serde_json::to_string(&facts)?)
    })();
    match result {
        Ok(text) => response(&fixture, StatusCode::OK, text),
        Err(error) => response(&fixture, StatusCode::BAD_REQUEST, format!("{error:#}")),
    }
}

async fn quiet(State(fixture): State<Fixture>, headers: HeaderMap, body: String) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut quiet = super::quiet::Quiet::new(super::quiet::options(&body)?)?;
        while quiet.step()? {}
        Ok(serde_json::to_string(&quiet.finish()?)?)
    })
    .await;
    match result {
        Ok(Ok(text)) => response(&fixture, StatusCode::OK, text),
        Ok(Err(error)) => response(&fixture, StatusCode::BAD_REQUEST, format!("{error:#}")),
        Err(error) => response(&fixture, StatusCode::BAD_GATEWAY, error.to_string()),
    }
}

async fn control(State(fixture): State<Fixture>, headers: HeaderMap, action: String) -> Response {
    if !allowed(&fixture, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let job = fixture.job.lock().unwrap();
    let Some(job) = job.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match action.as_str() {
        "release" => job.control.release.store(true, Ordering::Release),
        "stop" => job.control.stop.store(true, Ordering::Release),
        "cancel" => job.control.cancel.store(true, Ordering::Release),
        _ => return response(&fixture, StatusCode::BAD_REQUEST, "invalid action".into()),
    }
    response(&fixture, StatusCode::OK, "ok".into())
}

pub(crate) fn run_if_requested() -> Result<bool> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_none_or(|arg| arg != "--netplay-production-proof")
    {
        return Ok(false);
    }
    ensure!(
        args.len() == 3,
        "usage: --netplay-production-proof NEW_ROOT NONCE"
    );
    ensure!(
        std::env::var("ZEFF_MUTE_AUDIO").as_deref() == Ok("1"),
        "set ZEFF_MUTE_AUDIO=1"
    );
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir(&root).context("proof root must be new")?;
    let config = PathBuf::from(
        std::env::var_os("ZEFF_CONFIG_DIR").context("isolated configuration required")?,
    );
    std::fs::create_dir_all(&config)?;
    ensure!(
        std::fs::canonicalize(config)?.starts_with(std::fs::canonicalize(&root)?),
        "configuration must be inside proof root"
    );
    let id = args[2].to_str().context("invalid nonce")?.to_owned();
    ensure!(
        zeff_netplay_connect::protocol::valid_hex(&id, 32),
        "invalid fixture nonce"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:47180").await?;
        let mut config = Config::public();
        config.ice_servers.clear();
        config.origins = vec!["http://127.0.0.1:47181".into()];
        let lobby = Lobby::new(config);
        let fixture = Fixture {
            lobby: lobby.clone(),
            url: "ws://127.0.0.1:47180/v1/ws".into(),
            origin: "http://127.0.0.1:47181".into(),
            job: Default::default(),
            root,
        };
        let router = Router::new()
            .route(
                "/test/mixed/health",
                get(move || {
                    let id = id.clone();
                    async move { id }
                }),
            )
            .route("/test/gameplay/start", post(start))
            .route("/test/gameplay/status", get(status))
            .route("/test/gameplay/control", post(control))
            .route("/test/gameplay/facts", get(facts))
            .route("/test/gameplay/quiet", post(quiet))
            .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
            .with_state(fixture);
        axum::serve(listener, lobby.router().merge(router)).await?;
        Ok::<(), anyhow::Error>(())
    })?;
    Ok(true)
}
