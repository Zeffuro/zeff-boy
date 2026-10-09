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

mod worker;

#[derive(Default)]
pub(super) struct Control {
    progress: Mutex<Progress>,
    release: AtomicBool,
    stop: AtomicBool,
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
        let worker = std::thread::spawn(move || {
            if let Err(error) = worker::run(&options, &url, &lobby, &state, ready) {
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
        _ => return response(&fixture, StatusCode::BAD_REQUEST, "invalid action".into()),
    }
    response(&fixture, StatusCode::OK, "ok".into())
}

#[test]
#[ignore = "requires the owned mixed-gameplay browser runner"]
fn serve_mixed_gameplay_fixture() -> Result<()> {
    let id = std::env::var("ZEFF_MIXED_TEST_RUN_ID").context("fixture nonce required")?;
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
            .layer(DefaultBodyLimit::max(128))
            .with_state(fixture);
        axum::serve(listener, lobby.router().merge(router)).await?;
        Ok(())
    })
}

#[test]
fn mixed_options_are_bounded_and_require_matching_room_role() {
    assert!(worker::Options::parse("0:2:1:").is_ok());
    for bad in ["2:2:1:", "0:9:1:", "0:2:0:", "0:2:1:abcdef", "0:2:9:"] {
        assert!(worker::Options::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn mixed_http_rejects_foreign_origin_and_absent_jobs() -> Result<()> {
    let fixture = Fixture {
        lobby: Lobby::new(Config::public()),
        url: "ws://127.0.0.1:47180/v1/ws".into(),
        origin: "http://127.0.0.1:47181".into(),
        job: Default::default(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async {
        for origin in [None, Some("http://untrusted.invalid"), Some("null")] {
            let mut headers = HeaderMap::new();
            if let Some(origin) = origin {
                headers.insert("origin", origin.parse().unwrap());
            }
            assert_eq!(
                start(State(fixture.clone()), headers.clone(), "0:2:1:".into())
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                status(State(fixture.clone()), headers.clone())
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                control(State(fixture.clone()), headers, "stop".into())
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut headers = HeaderMap::new();
        headers.insert("origin", fixture.origin.parse().unwrap());
        assert_eq!(
            start(State(fixture.clone()), headers.clone(), "0:9:1:".into())
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status(State(fixture.clone()), headers.clone())
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            control(State(fixture), headers, "stop".into())
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    });
    Ok(())
}
