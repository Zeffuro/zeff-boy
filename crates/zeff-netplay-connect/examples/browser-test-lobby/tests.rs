use super::*;
use std::io::{Read, Write};

async fn request(
    address: std::net::SocketAddr,
    method: &str,
    path: &str,
    origin: Option<&str>,
    body: &str,
) -> String {
    let origin = origin
        .map(|value| format!("Origin: {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n{origin}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    tokio::task::spawn_blocking(move || {
        let mut stream =
            std::net::TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.take(8192).read_to_string(&mut response).unwrap();
        response
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn coordinator_rejects_foreign_origin_and_invalid_rooms_before_creating_a_job() {
    let lobby = Lobby::new(zeff_netplay_lobby::config::Config::public());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:47181";
    let router = router(
        lobby.clone(),
        format!("ws://{address}/v1/ws"),
        origin.into(),
        "owned-run".into(),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for origin in [
        None,
        Some("http://127.0.0.1:47182"),
        Some("https://example.invalid"),
    ] {
        for (method, path) in [
            ("POST", "/test/mixed/start"),
            ("GET", "/test/mixed/status/invalid"),
        ] {
            let response = request(address, method, path, origin, "").await;
            assert!(response.starts_with("HTTP/1.1 403"), "{response}");
        }
    }
    let response = request(
        address,
        "POST",
        "/test/mixed/start",
        Some(origin),
        "invalid",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    assert!(response.contains(&format!("access-control-allow-origin: {origin}")));
    let response = request(
        address,
        "GET",
        "/test/mixed/status/missing",
        Some(origin),
        "",
    )
    .await;
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    let response = request(address, "GET", "/test/mixed/health", None, "").await;
    assert!(
        response.starts_with("HTTP/1.1 200") && response.ends_with("owned-run"),
        "{response}"
    );
    assert_eq!(lobby.room_count(), 0);
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
