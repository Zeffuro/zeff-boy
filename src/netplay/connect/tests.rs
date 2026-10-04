use super::*;
use std::io::Write;

fn completed(connector: &mut Connector) -> Result<Start> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(start) = connector.poll()? {
            return Ok(start);
        }
        assert!(Instant::now() < deadline, "connector completion deadline");
        thread::sleep(Duration::from_millis(1));
    }
}

fn invitation(address: &str) -> String {
    format!("{address}/{}", "ab".repeat(32))
}

#[test]
fn invitations_require_numeric_loopback_socket_and_full_secret() {
    for address in ["127.0.0.1:1", "127.10.20.30:65535", "[::1]:1234"] {
        let (parsed, secret, input_delay) =
            parse_invitation(&invitation(address), ConnectionScope::Loopback).unwrap();
        assert!(parsed.ip().is_loopback());
        assert_eq!(secret, [0xab; 32]);
        assert_eq!(input_delay, InputDelay::default());
    }
    for address in [
        "localhost:1234",
        "127.0.0.1",
        "127.0.0.1:0",
        "0.0.0.0:1234",
        "192.168.1.1:1234",
        "8.8.8.8:1234",
        "[::]:1234",
        "[::ffff:127.0.0.1]:1234",
        "[::1%1]:1234",
        "::1:1234",
        "127.0.0.1:65536",
        " 127.0.0.1:1234",
        "127.0.0.1:1234 ",
    ] {
        assert!(
            parse_invitation(&invitation(address), ConnectionScope::Loopback).is_err(),
            "{address}"
        );
    }
    for text in [
        String::new(),
        "127.0.0.1:1234".to_owned(),
        "127.0.0.1:1234/".to_owned(),
        format!("127.0.0.1:1234/{}", "a".repeat(63)),
        format!("127.0.0.1:1234/{}", "a".repeat(65)),
        format!("127.0.0.1:1234/{}", "z".repeat(64)),
        format!("127.0.0.1:1234/0x{}", "a".repeat(64)),
        format!("{}/extra", invitation("127.0.0.1:1234")),
        format!("{}/", invitation("127.0.0.1:1234")),
    ] {
        assert!(parse_invitation(&text, ConnectionScope::Loopback).is_err());
    }
}

#[test]
fn invitation_errors_do_not_echo_capability_or_address() {
    let capability = "ab".repeat(32);
    let text = format!("8.8.8.8:1234/{capability}");
    let error = Connector::join(&text, ConnectionScope::Loopback)
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains(&capability));
    assert!(!error.contains("8.8.8.8"));
}

#[test]
fn invitation_delay_accepts_every_supported_value_and_rejects_malformed_values() {
    let legacy = invitation("127.0.0.1:1234");
    assert_eq!(
        invitation_delay(&legacy, ConnectionScope::Loopback).unwrap(),
        InputDelay::default()
    );
    for frames in InputDelay::MIN..=InputDelay::MAX {
        let text = format!("{legacy}/{frames}");
        assert_eq!(
            invitation_delay(&text, ConnectionScope::Loopback)
                .unwrap()
                .frames(),
            frames
        );
    }
    for suffix in [
        "", "9", "256", "00", "01", "02", "+0", "+2", " 2", "2 ", "2/3", "2/", "two",
    ] {
        let text = format!("{legacy}/{suffix}");
        assert!(
            validate_invitation(&text, ConnectionScope::Loopback).is_err(),
            "{suffix}"
        );
        assert!(
            Connector::join(&text, ConnectionScope::Loopback).is_err(),
            "{suffix}"
        );
    }
}

#[test]
fn every_host_delay_is_advertised_and_carried_by_both_connectors() {
    for frames in InputDelay::MIN..=InputDelay::MAX {
        let input_delay = InputDelay::new(frames).unwrap();
        let (mut host, text) = Connector::host_with_options(
            HostOptions {
                address: "127.0.0.1:0".parse().unwrap(),
                scope: ConnectionScope::Loopback,
                input_delay,
            },
            [1; 32],
            CONNECT_BUDGET,
        )
        .unwrap();
        let (address, secret, parsed_delay) =
            parse_invitation(&text, ConnectionScope::Loopback).unwrap();
        let mut join = Connector::join_with_build(
            address,
            secret,
            [1; 32],
            CONNECT_BUDGET,
            ConnectionScope::Loopback,
            parsed_delay,
        )
        .unwrap();
        assert_eq!(completed(&mut host).unwrap().input_delay, input_delay);
        assert_eq!(completed(&mut join).unwrap().input_delay, input_delay);
    }
}

#[test]
fn host_and_join_return_matching_owned_streams_and_random_capability() {
    let build = executable_build().unwrap();
    let (mut host, text) = Connector::host(HostOptions {
        address: "127.0.0.1:0".parse().unwrap(),
        scope: ConnectionScope::Loopback,
        input_delay: InputDelay::new(6).unwrap(),
    })
    .unwrap();
    let (address, secret, input_delay) =
        parse_invitation(&text, ConnectionScope::Loopback).unwrap();
    assert_eq!(
        address.ip(),
        "127.0.0.1".parse::<std::net::IpAddr>().unwrap()
    );
    assert_ne!(secret, [0; 32]);
    let mut join = Connector::join(&text, ConnectionScope::Loopback).unwrap();
    let mut one = completed(&mut host).unwrap();
    let mut two = completed(&mut join).unwrap();
    assert_eq!(one.player, Player::One);
    assert_eq!(two.player, Player::Two);
    assert_eq!(one.build, build);
    assert_eq!(two.build, build);
    assert_eq!(one.secret, secret);
    assert_eq!(two.secret, secret);
    assert_eq!(input_delay.frames(), 6);
    assert_eq!(one.input_delay, input_delay);
    assert_eq!(two.input_delay, input_delay);
    assert!(text.ends_with("/6"));
    assert!(one.stream.local_addr().unwrap().ip().is_loopback());
    assert!(two.stream.peer_addr().unwrap().ip().is_loopback());
    one.stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    two.stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    one.stream.write_all(&[42]).unwrap();
    let mut byte = [0];
    two.stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [42]);
    two.stream.write_all(&[19]).unwrap();
    one.stream.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [19]);
    assert!(host.poll().unwrap().is_none());
    assert!(join.poll().unwrap().is_none());
    assert!(host.worker.is_none());
    assert!(join.worker.is_none());
}

#[test]
fn pending_host_cancel_joins_and_closes_listener() {
    let (mut host, text) = Connector::host_with_build([1; 32], CONNECT_BUDGET).unwrap();
    let (address, _, _) = parse_invitation(&text, ConnectionScope::Loopback).unwrap();
    let start = Instant::now();
    host.cancel();
    assert!(start.elapsed() < Duration::from_millis(250));
    assert!(host.worker.is_none());
    assert!(host.result.is_none());
    assert!(TcpListener::bind(address).is_ok());
    assert!(host.poll().unwrap().is_none());
}

#[test]
fn pending_join_drop_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let join = Connector::join_with_build(
        address,
        [2; 32],
        [1; 32],
        CONNECT_BUDGET,
        ConnectionScope::Loopback,
        InputDelay::default(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(20));
    let start = Instant::now();
    drop(join);
    assert!(start.elapsed() < Duration::from_millis(250));
}

#[test]
fn host_and_refused_join_reach_deadline_and_close() {
    let budget = Duration::from_millis(40);
    let (mut host, text) = Connector::host_with_build([1; 32], budget).unwrap();
    let (address, _, _) = parse_invitation(&text, ConnectionScope::Loopback).unwrap();
    assert!(
        completed(&mut host)
            .err()
            .unwrap()
            .to_string()
            .contains("timed out")
    );
    assert!(TcpListener::bind(address).is_ok());

    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    let mut join = Connector::join_with_build(
        address,
        [2; 32],
        [1; 32],
        budget,
        ConnectionScope::Loopback,
        InputDelay::default(),
    )
    .unwrap();
    assert!(
        completed(&mut join)
            .err()
            .unwrap()
            .to_string()
            .contains("timed out")
    );
    assert!(join.worker.is_none());
}

#[test]
fn refused_join_retries_until_listener_is_available() {
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    let mut join = Connector::join_with_build(
        address,
        [2; 32],
        [1; 32],
        Duration::from_secs(2),
        ConnectionScope::Loopback,
        InputDelay::default(),
    )
    .unwrap();
    thread::sleep(Duration::from_millis(25));
    let listener = TcpListener::bind(address).unwrap();
    let start = completed(&mut join).unwrap();
    assert_eq!(start.stream.peer_addr().unwrap(), address);
    let (peer, _) = listener.accept().unwrap();
    assert_eq!(peer.local_addr().unwrap(), address);
}

#[test]
fn cancellation_discards_an_already_connected_socket() {
    let (mut host, text) = Connector::host_with_build([1; 32], CONNECT_BUDGET).unwrap();
    let (address, _, _) = parse_invitation(&text, ConnectionScope::Loopback).unwrap();
    let mut peer = TcpStream::connect(address).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !host.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    let start = Instant::now();
    host.cancel();
    assert!(start.elapsed() < Duration::from_millis(250));
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    assert!(host.poll().unwrap().is_none());
}

#[test]
fn private_invitation_requires_explicit_scope_and_never_accepts_public_endpoints() {
    for address in ["192.168.1.1:8766", "100.64.0.1:8766", "[fd00::1]:8766"] {
        let text = invitation(address);
        assert!(validate_invitation(&text, ConnectionScope::Loopback).is_err());
        validate_invitation(&text, ConnectionScope::TrustedPrivate).unwrap();
    }
    for address in [
        "0.0.0.0:8766",
        "8.8.8.8:8766",
        "[fe80::1]:8766",
        "192.168.1.1:0",
    ] {
        assert!(
            validate_invitation(&invitation(address), ConnectionScope::TrustedPrivate).is_err()
        );
    }
}

#[test]
fn private_connector_retains_selected_scope_and_random_capability() {
    let options = HostOptions {
        address: "127.0.0.1:0".parse().unwrap(),
        scope: ConnectionScope::TrustedPrivate,
        input_delay: InputDelay::new(8).unwrap(),
    };
    let (mut host, text) = Connector::host_with_options(options, [1; 32], CONNECT_BUDGET).unwrap();
    let (address, secret, input_delay) = parse_invitation(&text, options.scope).unwrap();
    let mut join = Connector::join_with_build(
        address,
        secret,
        [1; 32],
        CONNECT_BUDGET,
        options.scope,
        input_delay,
    )
    .unwrap();
    let one = completed(&mut host).unwrap();
    let two = completed(&mut join).unwrap();
    assert_eq!(one.input_delay, options.input_delay);
    assert_eq!(two.input_delay, options.input_delay);
    assert!(text.ends_with("/8"));
    assert_eq!(one.scope, options.scope);
    assert_eq!(two.scope, options.scope);
    assert_eq!(one.secret, two.secret);
    assert_ne!(one.secret, [0; 32]);
}

#[test]
fn private_timeout_has_reachability_guidance_and_no_invitation_data() {
    let options = HostOptions {
        address: "127.0.0.1:0".parse().unwrap(),
        scope: ConnectionScope::TrustedPrivate,
        input_delay: InputDelay::new(8).unwrap(),
    };
    let (mut host, invitation) =
        Connector::host_with_options(options, [1; 32], Duration::from_millis(40)).unwrap();
    let error = completed(&mut host).err().unwrap().to_string();
    assert!(error.contains("timed out") && error.contains("host firewall"));
    assert!(!error.contains(&invitation));
    assert!(host.worker.is_none());
}
