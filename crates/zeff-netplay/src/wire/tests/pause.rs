use super::*;

#[test]
fn pause_round_trips_both_states_at_the_same_frame_in_both_directions() {
    let (mut host, mut client) = pair();
    for paused in [true, false, true, false] {
        let message = Message::Pause {
            frame: 0x0102_0304_0506_0708,
            paused,
        };
        let packet = host.encode(&message);
        assert_eq!(packet.len(), HEADER_LEN + 9 + TAG_LEN);
        assert_eq!(packet[47], 4);
        assert_eq!(
            &packet[HEADER_LEN..HEADER_LEN + 8],
            &[1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(packet[HEADER_LEN + 8], u8::from(paused));
        host.send(&message).unwrap();
        assert_eq!(client.receive().unwrap(), message);
        client.send(&message).unwrap();
        assert_eq!(host.receive().unwrap(), message);
    }
    host.send(&input(Player::One)).unwrap();
    assert_eq!(client.receive().unwrap(), input(Player::One));
}

#[test]
fn pause_requires_exact_payload_length_even_with_valid_mac() {
    for length in (0..9).chain([10]) {
        let (mut host, mut client) = pair();
        let message = Message::Pause {
            frame: 17,
            paused: true,
        };
        let mut packet = host.encode(&message);
        packet.truncate(HEADER_LEN);
        packet.resize(HEADER_LEN + length + TAG_LEN, 0);
        resign(&mut packet);
        write_packet(&mut host, &packet);
        let error = client.receive().unwrap_err().to_string();
        assert!(error.contains("pause length"), "length {length}: {error}");
        assert!(client.send(&message).is_err());
    }
}

#[test]
fn pause_rejects_nonboolean_flags_even_with_valid_mac() {
    for flag in [2, 0x80, 0xff] {
        let (mut host, mut client) = pair();
        let message = Message::Pause {
            frame: 17,
            paused: true,
        };
        let mut packet = host.encode(&message);
        packet[HEADER_LEN + 8] = flag;
        resign(&mut packet);
        write_packet(&mut host, &packet);
        let error = client.receive().unwrap_err().to_string();
        assert!(error.contains("pause flag"), "flag {flag}: {error}");
        assert!(client.send(&message).is_err());
    }
}

#[test]
fn pause_state_cannot_be_changed_without_authentication() {
    let (mut host, mut client) = pair();
    let mut packet = host.encode(&Message::Pause {
        frame: 17,
        paused: true,
    });
    packet[HEADER_LEN + 8] = 0;
    write_packet(&mut host, &packet);
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("authentication")
    );
}

#[test]
fn close_fences_pause_send_and_receive() {
    let close = Message::Close { frame: 18 };
    let pause = Message::Pause {
        frame: 18,
        paused: true,
    };
    let (mut host, mut client) = pair();
    host.send(&close).unwrap();
    client.send(&pause).unwrap();
    assert!(
        host.receive()
            .unwrap_err()
            .to_string()
            .contains("after local Close")
    );

    let (mut host, mut client) = pair();
    host.send(&close).unwrap();
    client.receive().unwrap();
    assert!(
        client
            .send(&pause)
            .unwrap_err()
            .to_string()
            .contains("acknowledgment")
    );

    let (mut host, _client) = pair();
    host.send(&close).unwrap();
    assert!(
        host.send(&pause)
            .unwrap_err()
            .to_string()
            .contains("already sent")
    );
}

#[test]
fn previous_version_admission_and_packets_are_rejected() {
    for version in [1u16, 2] {
        let (host, mut client) = streams();
        let worker = thread::spawn(move || admit(host, Player::One, &identity(), &[9; 32]));
        let mut remote = hello(Player::Two, &identity()).unwrap();
        assert_eq!(&remote[4..6], &VERSION.to_be_bytes());
        remote[4..6].copy_from_slice(&version.to_be_bytes());
        client.write_all(&remote).unwrap();
        assert!(
            worker
                .join()
                .unwrap()
                .err()
                .unwrap()
                .to_string()
                .contains("wire version")
        );

        let (mut host, mut client) = pair();
        let mut packet = host.encode(&input(Player::One));
        packet[4..6].copy_from_slice(&version.to_be_bytes());
        resign(&mut packet);
        write_packet(&mut host, &packet);
        assert!(
            client
                .receive()
                .unwrap_err()
                .to_string()
                .contains("packet version")
        );
    }
}
