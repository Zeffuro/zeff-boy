use super::*;
use zeff_netplay::rollback::InputDelay;

fn parse(values: &[(&str, &str)]) -> Result<Route> {
    Route::parse(InputDelay::new(2)?, |name| {
        Ok(values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned()))
    })
}

#[test]
fn lan_is_default_and_lobby_requires_explicit_url_and_token() {
    assert!(!parse(&[]).unwrap().is_lobby());
    assert!(
        !parse(&[("ZEFF_NETPLAY_APP_ROUTE", "lan")])
            .unwrap()
            .is_lobby()
    );
    for route in ["", "true", "LAN", "rtc"] {
        assert!(parse(&[("ZEFF_NETPLAY_APP_ROUTE", route)]).is_err());
    }
    assert!(parse(&[("ZEFF_NETPLAY_APP_ROUTE", "lobby")]).is_err());
    assert!(
        parse(&[
            ("ZEFF_NETPLAY_APP_ROUTE", "lobby"),
            ("ZEFF_NETPLAY_APP_LOBBY_URL", "wss://lobby.example/v1/ws"),
        ])
        .is_err()
    );
    assert!(
        parse(&[
            ("ZEFF_NETPLAY_APP_ROUTE", "lobby"),
            ("ZEFF_NETPLAY_APP_LOBBY_URL", "wss://lobby.example/v1/ws"),
            ("ZEFF_NETPLAY_APP_LOBBY_TOKEN", ""),
        ])
        .unwrap()
        .is_lobby()
    );
}

#[test]
fn lobby_route_rejects_insecure_urls_and_keeps_key_errors_value_free() {
    let key = "private-key-marker".repeat(32);
    let error = parse(&[
        ("ZEFF_NETPLAY_APP_ROUTE", "lobby"),
        ("ZEFF_NETPLAY_APP_LOBBY_URL", "wss://lobby.example/v1/ws"),
        ("ZEFF_NETPLAY_APP_LOBBY_TOKEN", &key),
    ])
    .err()
    .unwrap();
    assert!(!format!("{error:#}").contains("private-key-marker"));
    for url in [
        "ws://remote.example/v1/ws",
        "wss://key@lobby.example/v1/ws",
        "wss://lobby.example/v1/ws?key=secret",
    ] {
        assert!(
            parse(&[
                ("ZEFF_NETPLAY_APP_ROUTE", "lobby"),
                ("ZEFF_NETPLAY_APP_LOBBY_URL", url),
                ("ZEFF_NETPLAY_APP_LOBBY_TOKEN", ""),
            ])
            .is_err()
        );
    }
}

#[test]
fn lobby_invitation_binds_url_and_delay_and_cannot_use_tcp_evidence() {
    let route = parse(&[
        ("ZEFF_NETPLAY_APP_ROUTE", "lobby"),
        ("ZEFF_NETPLAY_APP_LOBBY_URL", "wss://lobby.example/v1/ws"),
        ("ZEFF_NETPLAY_APP_LOBBY_TOKEN", "local-key"),
    ])
    .unwrap();
    let room = "a".repeat(24);
    let invite = lobby::invitation(
        "wss://lobby.example/v1/ws",
        &room,
        [8; 32],
        InputDelay::new(2).unwrap(),
    );
    route.validate_invitation(&invite).unwrap();
    for (url, delay) in [
        ("wss://other.example/v1/ws", 2),
        ("wss://lobby.example/v1/ws", 0),
    ] {
        let other = lobby::invitation(url, &room, [8; 32], InputDelay::new(delay).unwrap());
        assert!(route.validate_invitation(&other).is_err());
    }
    assert!(Route::Lan.endpoints(None).is_err());
    assert_eq!(
        route.endpoints(None).unwrap(),
        (None, None, "direct-dtls-sctp")
    );
    let endpoint: SocketAddr = "127.0.0.1:1".parse().unwrap();
    assert!(
        route
            .endpoints(Some((endpoint, endpoint, ConnectionScope::TrustedPrivate)))
            .is_err()
    );
}
