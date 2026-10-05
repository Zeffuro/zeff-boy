use super::*;
use std::sync::mpsc;

#[test]
fn pause_controls_round_trip_independently_of_gameplay() {
    let (host, mut client) = pair();
    let (mut sender, mut receiver) = host.split().unwrap();
    for message in [
        Message::PauseChange {
            request: 1,
            frame: 12,
            paused: true,
        },
        Message::PauseChange {
            request: 2,
            frame: 12,
            paused: false,
        },
        Message::PauseChange {
            request: u64::MAX,
            frame: u64::MAX,
            paused: true,
        },
        Message::PauseAck { request: u64::MAX },
    ] {
        sender.send(&message).unwrap();
        assert_eq!(client.receive().unwrap(), message);
    }
    client.send(&Message::PauseAck { request: 2 }).unwrap();
    assert_eq!(
        receiver.receive().unwrap(),
        Message::PauseAck { request: 2 }
    );
}

#[test]
fn pause_control_lengths_flags_and_zero_ids_fail_closed() {
    for message in [
        Message::PauseChange {
            request: 0,
            frame: 12,
            paused: true,
        },
        Message::PauseAck { request: 0 },
    ] {
        let (mut host, _client) = pair();
        assert!(host.send(&message).is_err());
        assert!(host.send(&input(Player::One)).is_err());
    }
    for field in 0..5 {
        let (mut host, mut client) = pair();
        let message = if field >= 3 {
            Message::PauseAck {
                request: if field == 3 { 0 } else { 1 },
            }
        } else {
            Message::PauseChange {
                request: if field == 0 { 0 } else { 1 },
                frame: 12,
                paused: true,
            }
        };
        let mut packet = host.encode(&message);
        match field {
            1 => packet[HEADER_LEN + 16] = 2,
            2 | 4 => {
                packet.remove(HEADER_LEN);
            }
            _ => {}
        }
        let length = packet.len() - TAG_LEN;
        let signature = tag(&host.secret, &[PACKET_DOMAIN, &packet[..length]]);
        packet[length..].copy_from_slice(&signature);
        write_raw(&mut host, &(packet.len() as u32).to_be_bytes());
        write_raw(&mut host, &packet);
        assert!(client.receive().is_err(), "field {field}");
        assert!(
            client
                .receive()
                .unwrap_err()
                .to_string()
                .contains("terminal")
        );
    }
}

#[test]
fn independent_sender_pipelines_inputs_before_any_reply() {
    let (host, mut client) = pair();
    let (mut sender, mut receiver) = host.split().unwrap();
    for frame in 2..10 {
        sender
            .send(&Message::Input {
                player: Player::One,
                frame,
                buttons: 0x8000 | frame as u16,
            })
            .unwrap();
    }
    for frame in 2..10 {
        assert_eq!(
            client.receive().unwrap(),
            Message::Input {
                player: Player::One,
                frame,
                buttons: 0x8000 | frame as u16
            }
        );
    }
    client
        .send(&Message::Progress {
            frame: 8,
            confirmed: 6,
        })
        .unwrap();
    assert_eq!(
        receiver.receive().unwrap(),
        Message::Progress {
            frame: 8,
            confirmed: 6
        }
    );
}

#[test]
fn split_receive_advances_while_sender_is_idle_and_preserves_sequences() {
    let (mut host, mut client) = pair();
    host.send(&input(Player::One)).unwrap();
    client.receive().unwrap();
    client.send(&input(Player::Two)).unwrap();
    host.receive().unwrap();
    let (mut sender, mut receiver) = host.split().unwrap();
    let reader = thread::spawn(move || {
        let message = receiver.receive().unwrap();
        (receiver, message)
    });
    client
        .send(&Message::Progress {
            frame: 10,
            confirmed: 7,
        })
        .unwrap();
    let (_receiver, message) = reader.join().unwrap();
    assert_eq!(
        message,
        Message::Progress {
            frame: 10,
            confirmed: 7
        }
    );
    sender
        .send(&Message::Progress {
            frame: 12,
            confirmed: 10,
        })
        .unwrap();
    assert_eq!(
        client.receive().unwrap(),
        Message::Progress {
            frame: 12,
            confirmed: 10
        }
    );
}

#[test]
fn dropping_sender_shutdown_interrupts_owned_receiver_without_peer_activity() {
    let (host, _client) = pair();
    let (sender, mut receiver) = host.split().unwrap();
    let (started, waiting) = mpsc::channel();
    let reader = thread::spawn(move || {
        started.send(()).unwrap();
        assert!(receiver.receive().is_err());
        assert!(
            receiver
                .receive()
                .unwrap_err()
                .to_string()
                .contains("terminal")
        );
    });
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    let start = Instant::now();
    drop(sender);
    reader.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn close_can_cross_a_pending_receive_and_never_allows_late_gameplay() {
    let (host, mut client) = pair();
    let (mut sender, mut receiver) = host.split().unwrap();
    let reader = thread::spawn(move || receiver.receive());
    sender.send(&Message::Close { frame: 12 }).unwrap();
    assert!(matches!(
        client.receive().unwrap(),
        Message::Close { frame: 12 }
    ));
    client.send(&Message::Close { frame: 12 }).unwrap();
    assert!(matches!(
        reader.join().unwrap().unwrap(),
        Message::Close { frame: 12 }
    ));
    assert!(sender.send(&input(Player::One)).is_err());
}

#[test]
fn progress_round_trip_boundary_and_invalid_bounds_are_terminal() {
    let (mut host, mut client) = pair();
    for message in [
        Message::Progress {
            frame: 0,
            confirmed: 0,
        },
        Message::Progress {
            frame: u64::MAX,
            confirmed: u64::MAX,
        },
    ] {
        host.send(&message).unwrap();
        assert_eq!(client.receive().unwrap(), message);
    }
    assert!(
        host.send(&Message::Progress {
            frame: 9,
            confirmed: 10
        })
        .is_err()
    );
    assert!(host.send(&input(Player::One)).is_err());
    let (mut host, mut client) = pair();
    let packet = host.encode(&Message::Progress {
        frame: 9,
        confirmed: 10,
    });
    write_raw(&mut host, &(packet.len() as u32).to_be_bytes());
    write_raw(&mut host, &packet);
    assert!(
        client
            .receive()
            .unwrap_err()
            .to_string()
            .contains("confirmed progress")
    );
}

#[test]
fn split_fences_role_mac_sequence_and_progress_length_before_delivery() {
    for field in 0..4 {
        let (mut host, client) = pair();
        let (_sender, mut receiver) = client.split().unwrap();
        let mut packet = host.encode(&Message::Progress {
            frame: 9,
            confirmed: 8,
        });
        match field {
            0 => packet[38] = 2,
            1 => packet[46] = 1,
            2 => packet[HEADER_LEN] ^= 1,
            3 => {
                packet.remove(HEADER_LEN);
                let length = packet.len() - TAG_LEN;
                let signature = tag(&host.secret, &[PACKET_DOMAIN, &packet[..length]]);
                packet[length..].copy_from_slice(&signature);
            }
            _ => unreachable!(),
        }
        write_raw(&mut host, &(packet.len() as u32).to_be_bytes());
        write_raw(&mut host, &packet);
        assert!(receiver.receive().is_err(), "field {field}");
        assert!(
            receiver
                .receive()
                .unwrap_err()
                .to_string()
                .contains("terminal")
        );
    }
}
