use super::*;

#[test]
fn packet_codec_carries_all_input_bits_and_rejects_narrow_payloads() {
    let mut sender = PacketCodec::new(Player::One, [9; 32], [7; 32]);
    let mut receiver = PacketCodec::new(Player::Two, [9; 32], [7; 32]);
    for buttons in [0x0100, 0x07ff, 0x8000, u16::MAX] {
        let message = Message::Input {
            player: Player::One,
            frame: 17,
            buttons,
        };
        assert_eq!(
            receiver.decode(&sender.encode(&message).unwrap()).unwrap(),
            message
        );
    }
    let mut narrow = sender.encode(&input(Player::One)).unwrap();
    narrow.remove(HEADER_LEN + 9);
    resign(&mut narrow);
    assert!(
        receiver
            .decode(&narrow)
            .unwrap_err()
            .to_string()
            .contains("length")
    );
    assert!(
        receiver
            .decode(&sender.encode(&input(Player::One)).unwrap())
            .is_err()
    );
}

#[test]
fn wide_input_uses_two_big_endian_bytes() {
    let (mut host, mut client) = pair();
    for buttons in [0x0100, 0x07ff, 0x8000, u16::MAX] {
        let message = Message::Input {
            player: Player::One,
            frame: 17,
            buttons,
        };
        let packet = host.encode(&message);
        assert_eq!(packet.len(), HEADER_LEN + 11 + TAG_LEN);
        assert_eq!(
            &packet[HEADER_LEN + 9..HEADER_LEN + 11],
            &buttons.to_be_bytes()
        );
        host.send(&message).unwrap();
        assert_eq!(client.receive().unwrap(), message);
    }
}

#[test]
fn previous_wire_version_and_narrow_input_payload_fail_closed() {
    for old_version in [true, false] {
        let (mut host, mut client) = pair();
        let mut packet = host.encode(&input(Player::One));
        if old_version {
            packet[4..6].copy_from_slice(&6u16.to_be_bytes());
        } else {
            packet.remove(HEADER_LEN + 9);
        }
        resign(&mut packet);
        write_packet(&mut host, &packet);
        let error = client.receive().unwrap_err().to_string();
        assert!(error.contains(if old_version { "version" } else { "length" }));
        assert!(client.send(&input(Player::Two)).is_err());
    }
}
