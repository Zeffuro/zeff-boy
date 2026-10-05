use super::*;
use std::net::TcpListener;
use std::thread;

mod chat;
mod compatibility;
mod pause;
mod split;
mod wider_input;

#[test]
fn explicit_private_scope_keeps_authenticated_loopback_interoperability() {
    let (host, peer) = streams();
    let worker = thread::spawn(move || {
        admit_scoped(
            host,
            Player::One,
            &identity(),
            &[9; 32],
            ConnectionScope::TrustedPrivate,
        )
        .unwrap()
    });
    let mut peer = admit(peer, Player::Two, &identity(), &[9; 32]).unwrap();
    let mut host = worker.join().unwrap();
    assert_eq!(host.transcript(), peer.transcript());
    host.send(&input(Player::One)).unwrap();
    assert_eq!(peer.receive().unwrap(), input(Player::One));
}

#[test]
fn private_admission_cancellation_closes_socket_before_handshake() {
    let (host, mut peer) = streams();
    peer.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let cancelled = Arc::new(AtomicBool::new(true));
    let start = Instant::now();
    let error = admit_cancellable_scoped(
        host,
        Player::One,
        &identity(),
        &[9; 32],
        ConnectionScope::TrustedPrivate,
        cancelled,
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("cancelled"));
    assert!(start.elapsed() < Duration::from_millis(250));
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

fn identity() -> Identity {
    Identity {
        build: [1; 32],
        build_info: Default::default(),
        source: [2; 32],
        effective: [3; 32],
        media_len: 123,
        config: [4; 32],
        initial: [5; 32],
        persistent: [6; 32],
        state_format: 7,
    }
}

fn streams() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (host, _) = listener.accept().unwrap();
    (host, client)
}

fn admission_pair(
    remote_identity: Identity,
    remote_player: Player,
    remote_secret: [u8; 32],
) -> (Result<Connection>, Result<Connection>) {
    admission_identities(identity(), remote_identity, remote_player, remote_secret)
}

fn admission_identities(
    local_identity: Identity,
    remote_identity: Identity,
    remote_player: Player,
    remote_secret: [u8; 32],
) -> (Result<Connection>, Result<Connection>) {
    let (host, client) = streams();
    let worker = thread::spawn(move || admit(host, Player::One, &local_identity, &[9; 32]));
    let remote = admit(client, remote_player, &remote_identity, &remote_secret);
    (worker.join().unwrap(), remote)
}

fn pair() -> (Connection, Connection) {
    let (host, client) = admission_pair(identity(), Player::Two, [9; 32]);
    (host.unwrap(), client.unwrap())
}

fn input(player: Player) -> Message {
    Message::Input {
        player,
        frame: 17,
        buttons: 0x07a5,
    }
}

fn write_packet(connection: &mut Connection, packet: &[u8]) {
    write_raw(connection, &(packet.len() as u32).to_be_bytes());
    write_raw(connection, packet);
}

fn write_raw(connection: &mut Connection, bytes: &[u8]) {
    connection
        .io
        .write(
            &mut connection.stream,
            bytes,
            Instant::now() + IO_BUDGET,
            None,
        )
        .unwrap();
}

fn resign(packet: &mut [u8]) {
    let offset = packet.len() - TAG_LEN;
    let signature = tag(&[9; 32], &[PACKET_DOMAIN, &packet[..offset]]);
    packet[offset..].copy_from_slice(&signature);
}

#[test]
fn admission_and_bidirectional_messages_round_trip() {
    let (mut host, mut client) = pair();
    assert_eq!(host.transcript(), client.transcript());
    let checkpoint = Message::Checkpoint {
        frame: 120,
        logical: [20; 32],
        video: [21; 32],
        audio: [22; 32],
        persistent: [23; 32],
    };
    for message in [input(Player::One), checkpoint] {
        host.send(&message).unwrap();
        assert_eq!(client.receive().unwrap(), message);
    }
    client.send(&input(Player::Two)).unwrap();
    assert_eq!(host.receive().unwrap(), input(Player::Two));
}

#[test]
fn admission_takes_ownership_of_previously_nonblocking_socket_mode() {
    let (host, client) = streams();
    host.set_nonblocking(true).unwrap();
    client.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || admit(host, Player::One, &identity(), &[9; 32]));
    let mut client = admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    let mut host = worker.join().unwrap().unwrap();
    host.send(&input(Player::One)).unwrap();
    assert_eq!(client.receive().unwrap(), input(Player::One));
}

#[test]
fn every_identity_field_is_admission_critical() {
    for field in 0..8 {
        let mut changed = identity();
        match field {
            0 => changed.build[0] ^= 1,
            1 => changed.source[0] ^= 1,
            2 => changed.effective[0] ^= 1,
            3 => changed.media_len += 1,
            4 => changed.config[0] ^= 1,
            5 => changed.initial[0] ^= 1,
            6 => changed.persistent[0] ^= 1,
            7 => changed.state_format += 1,
            _ => unreachable!(),
        }
        let (host, client) = admission_pair(changed, Player::Two, [9; 32]);
        assert!(host.is_err(), "identity field {field}");
        assert!(client.is_err(), "identity field {field}");
    }
}

#[test]
fn wrong_secret_and_same_role_reject_both_peers() {
    for (player, secret) in [(Player::Two, [8; 32]), (Player::One, [9; 32])] {
        let (host, client) = admission_pair(identity(), player, secret);
        assert!(host.is_err());
        assert!(client.is_err());
    }
}

#[test]
fn fresh_admissions_have_distinct_sessions() {
    let (first, _) = pair();
    let (second, _) = pair();
    assert_ne!(first.transcript(), second.transcript());
}

#[test]
fn invalid_hello_magic_version_and_role_fail_before_authentication() {
    for offset in [0, 5, 6] {
        let (host, mut client) = streams();
        let worker = thread::spawn(move || admit(host, Player::One, &identity(), &[9; 32]));
        let mut remote = hello(Player::Two, &identity()).unwrap();
        remote[offset] ^= 0xff;
        client.write_all(&remote).unwrap();
        assert!(worker.join().unwrap().is_err());
    }
}

#[test]
fn ready_barrier_cannot_be_replaced_by_authentication_tag() {
    let (host, mut client) = streams();
    let worker = thread::spawn(move || admit(host, Player::One, &identity(), &[9; 32]));
    let remote = hello(Player::Two, &identity()).unwrap();
    client.write_all(&remote).unwrap();
    let mut local = [0; HELLO_LEN];
    client.read_exact(&mut local).unwrap();
    let auth = tag(
        &[9; 32],
        &[AUTH_DOMAIN, &local, &remote, &[role(Player::Two)]],
    );
    client.write_all(&auth).unwrap();
    let mut host_auth = [0; TAG_LEN];
    client.read_exact(&mut host_auth).unwrap();
    client.write_all(&auth).unwrap();
    assert!(worker.join().unwrap().is_err());
}

#[test]
fn replay_is_terminal() {
    let (mut host, mut client) = pair();
    let packet = host.encode(&input(Player::One));
    write_packet(&mut host, &packet);
    assert_eq!(client.receive().unwrap(), input(Player::One));
    write_packet(&mut host, &packet);
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("sequence")
    );
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("terminal")
    );
}

#[test]
fn another_sessions_authenticated_packet_is_rejected() {
    let (old_host, _) = pair();
    let (mut host, mut client) = pair();
    write_packet(&mut host, &old_host.encode(&input(Player::One)));
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("session")
    );
}

#[test]
fn malformed_packet_fields_fail_closed_even_with_valid_mac() {
    for (offset, message) in [
        (0, "magic"),
        (5, "version"),
        (6, "session"),
        (38, "role"),
        (46, "sequence"),
        (47, "kind"),
        (48, "port"),
    ] {
        let (mut host, mut client) = pair();
        let mut packet = host.encode(&input(Player::One));
        packet[offset] ^= 0xff;
        resign(&mut packet);
        write_packet(&mut host, &packet);
        let error = client.receive().unwrap_err().to_string();
        assert!(error.contains(message), "offset {offset}: {error}");
        assert!(client.send(&input(Player::Two)).is_err());
    }
}

#[test]
fn altered_payload_or_tag_fails_authentication() {
    for offset in [49, 89] {
        let (mut host, mut client) = pair();
        let mut packet = host.encode(&input(Player::One));
        packet[offset] ^= 1;
        write_packet(&mut host, &packet);
        assert!(
            client
                .receive()
                .unwrap_err()
                .to_string()
                .contains("authentication")
        );
    }
}

#[test]
fn invalid_lengths_reject_without_waiting_for_payload() {
    for length in [
        0,
        (HEADER_LEN + TAG_LEN - 1) as u32,
        (MAX_PACKET + 1) as u32,
        u32::MAX,
    ] {
        let (mut host, mut client) = pair();
        write_raw(&mut host, &length.to_be_bytes());
        assert!(client.receive().unwrap_err().to_string().contains("length"));
    }
}

#[test]
fn canonical_payload_lengths_are_enforced() {
    for message in [
        input(Player::One),
        Message::Checkpoint {
            frame: 0,
            logical: [0; 32],
            video: [0; 32],
            audio: [0; 32],
            persistent: [0; 32],
        },
        Message::Close { frame: 0 },
    ] {
        let (mut host, mut client) = pair();
        let mut packet = host.encode(&message);
        packet.insert(HEADER_LEN, 0);
        resign(&mut packet);
        write_packet(&mut host, &packet);
        assert!(client.receive().unwrap_err().to_string().contains("length"));
    }
}

#[test]
fn send_enforces_local_port_ownership() {
    let (mut host, mut client) = pair();
    assert!(
        host.send(&input(Player::Two))
            .unwrap_err()
            .to_string()
            .contains("port")
    );
    assert!(host.send(&input(Player::One)).is_err());
    assert!(client.receive().is_err());
}

#[test]
fn close_acknowledgment_makes_both_endpoints_terminal() {
    let (mut host, mut client) = pair();
    let close = Message::Close { frame: 18 };
    host.send(&close).unwrap();
    assert_eq!(client.receive().unwrap(), close);
    client.send(&close).unwrap();
    assert_eq!(host.receive().unwrap(), close);
    for connection in [&mut host, &mut client] {
        assert!(
            connection
                .receive()
                .unwrap_err()
                .to_string()
                .contains("terminal")
        );
        assert!(
            connection
                .send(&close)
                .unwrap_err()
                .to_string()
                .contains("terminal")
        );
    }
}

#[test]
fn close_rejects_late_gameplay_in_both_directions() {
    let (mut host, mut client) = pair();
    host.send(&Message::Close { frame: 18 }).unwrap();
    client.send(&input(Player::Two)).unwrap();
    assert!(
        host.receive()
            .unwrap_err()
            .to_string()
            .contains("after local Close")
    );

    let (mut host, mut client) = pair();
    host.send(&Message::Close { frame: 18 }).unwrap();
    client.receive().unwrap();
    assert!(
        client
            .send(&input(Player::Two))
            .unwrap_err()
            .to_string()
            .contains("acknowledgment")
    );
}

#[test]
fn truncated_packet_times_out_with_bounded_read() {
    let (mut host, mut client) = pair();
    client
        .stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    write_raw(&mut host, &((HEADER_LEN + TAG_LEN) as u32).to_be_bytes());
    write_raw(&mut host, &[0; 5]);
    assert!(client.receive().is_err());
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("terminal")
    );
}

#[test]
fn trickle_progress_cannot_renew_a_shared_io_deadline() {
    let (mut host, mut client) = streams();
    host.set_nodelay(true).unwrap();
    client.set_read_timeout(Some(IO_BUDGET)).unwrap();
    host.write_all(&[1; 3]).unwrap();
    let worker = thread::spawn(move || {
        for _ in 0..64 {
            thread::sleep(Duration::from_millis(10));
            if host.write_all(&[1]).is_err() {
                break;
            }
        }
    });
    {
        let mut driver = Driver::new(&client).unwrap();
        let start = Instant::now();
        let deadline = start + Duration::from_millis(40);
        let mut first = [0; 1];
        read_deadline(&mut driver, &mut client, &mut first, deadline).unwrap();
        assert_eq!(first, [1]);
        let mut rest = [0; 64];
        assert!(read_deadline(&mut driver, &mut client, &mut rest, deadline).is_err());
        assert_eq!(&rest[..2], &[1; 2]);
        assert!(start.elapsed() < Duration::from_millis(500));
        assert_eq!(client.read_timeout().unwrap(), Some(IO_BUDGET));
    }
    drop(client);
    worker.join().unwrap();
}

#[test]
fn already_expired_deadline_rejects_even_buffered_data() {
    let (mut host, mut client) = streams();
    host.write_all(&[1]).unwrap();
    let mut reader = Driver::new(&client).unwrap();
    let mut writer = Driver::new(&host).unwrap();
    let deadline = Instant::now() - Duration::from_millis(1);
    assert!(read_deadline(&mut reader, &mut client, &mut [0; 1], deadline).is_err());
    assert!(write_deadline(&mut writer, &mut host, &[1], deadline).is_err());
}

#[test]
fn timeout_and_disconnect_are_terminal() {
    let (host, mut client) = pair();
    assert_eq!(
        client.stream.read_timeout().unwrap(),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        client.stream.write_timeout().unwrap(),
        Some(Duration::from_secs(2))
    );
    assert!(client.stream.nodelay().unwrap());
    client
        .stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    assert!(client.receive().is_err());
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("terminal")
    );
    drop(host);
    let (host, mut client) = pair();
    drop(host);
    assert!(client.receive().is_err());
}

#[test]
fn sequence_exhaustion_rejects_instead_of_wrapping() {
    let (mut host, mut client) = pair();
    host.send_sequence = u64::MAX;
    client.receive_sequence = u64::MAX;
    assert!(
        host.send(&input(Player::One))
            .unwrap_err()
            .to_string()
            .contains("exhausted")
    );
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("exhausted")
    );
}

#[test]
fn hmac_matches_rfc4231_sha256_example() {
    let mut known = Hmac::<Sha256>::new_from_slice(&[0x0b; 20]).unwrap();
    known.update(b"Hi There");
    let signature: [u8; 32] = known.finalize().into_bytes().into();
    assert_eq!(
        signature,
        [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ]
    );
    let secret = [9; 32];
    let signature = tag(&secret, &[b"Hi There"]);
    verify(&secret, &[b"Hi There"], &signature).unwrap();
    assert!(verify(&secret, &[b"different"], &signature).is_err());
}
