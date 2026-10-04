use super::*;

#[test]
fn authenticated_unicode_chat_interleaves_with_gameplay_in_both_directions() {
    let (mut host, mut peer) = pair();
    for text in [
        "Ready?",
        "Oui, c'est parti 🎮",
        &"é".repeat(CHAT_MAX_BYTES / 2),
    ] {
        let chat = Message::Chat {
            text: text.to_owned(),
        };
        host.send(&chat).unwrap();
        assert_eq!(peer.receive().unwrap(), chat);
        peer.send(&input(Player::Two)).unwrap();
        assert_eq!(host.receive().unwrap(), input(Player::Two));
        peer.send(&chat).unwrap();
        assert_eq!(host.receive().unwrap(), chat);
    }
}

#[test]
fn chat_validation_rejects_empty_oversized_control_and_bidi_text() {
    for text in [
        "",
        "   ",
        "a\nb",
        "a\tb",
        "\0",
        "abc\u{202e}",
        "\u{2066}abc",
        &"é".repeat(257),
    ] {
        assert!(validate_chat(text).is_err(), "{text:?}");
    }
    validate_chat(&"a".repeat(CHAT_MAX_BYTES)).unwrap();
}

#[test]
fn hostile_authenticated_invalid_chat_payload_is_terminal() {
    for payload in [
        vec![0xff],
        vec![b'a'; CHAT_MAX_BYTES + 1],
        vec![],
        b"a\nb".to_vec(),
    ] {
        let (mut host, mut peer) = pair();
        let mut packet = host.encode(&Message::Chat { text: "ok".into() });
        packet.truncate(HEADER_LEN);
        packet.extend_from_slice(&payload);
        packet.extend_from_slice(&tag(&host.secret, &[PACKET_DOMAIN, &packet]));
        write_packet(&mut host, &packet);
        assert!(peer.receive().is_err());
        assert!(peer.receive().is_err());
    }
}

#[test]
fn chat_cannot_follow_close() {
    let (mut host, mut peer) = pair();
    host.send(&Message::Close { frame: 0 }).unwrap();
    assert!(matches!(peer.receive().unwrap(), Message::Close { .. }));
    assert!(
        peer.send(&Message::Chat {
            text: "late".into()
        })
        .is_err()
    );
}
