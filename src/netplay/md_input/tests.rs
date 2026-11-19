use super::*;
use zeff_md_core::{ControllerPort, Emulator, LoadConfig};

fn inputs(words: [u16; 2], kinds: [ControllerKind; 2]) -> Result<MdRollbackInputs> {
    let mut core = Emulator::new(
        crate::netplay::md_fixture::rom(),
        LoadConfig {
            concurrent_sound: true,
            ..LoadConfig::default()
        },
    )
    .unwrap();
    for (port, kind) in [ControllerPort::One, ControllerPort::Two]
        .into_iter()
        .zip(kinds)
    {
        core.input_mut().set_controller_kind(port, kind);
    }
    MdRollbackInputs::new(words, &MdRollbackConfig::capture(&core)?)
}
fn from_host(ports: [(u8, u8); 2], kinds: [ControllerKind; 2]) -> Result<MdRollbackInputs> {
    inputs(
        ports.map(|(buttons, dpad)| u16::from(buttons) | (u16::from(dpad) << 8)),
        kinds,
    )
}
fn hosts(words: [u16; 2]) -> [(u8, u8); 2] {
    words.map(|word| (word as u8, (word >> 8) as u8))
}
use zeff_netplay::datagram::InputChannel;
use zeff_netplay::lockstep::Player;
use zeff_netplay::wire::{Admission, BuildInfo, Identity, Message, PacketCodec};

fn identity() -> Identity {
    Identity {
        build: [1; 32],
        build_info: BuildInfo::default(),
        source: [2; 32],
        effective: [2; 32],
        media_len: 0x10000,
        config: [3; 32],
        initial: [4; 32],
        persistent: [5; 32],
        state_format: 1,
    }
}

fn codecs() -> (PacketCodec, PacketCodec) {
    let mut a = Admission::new(Player::One, identity(), [9; 32]).unwrap();
    let mut b = Admission::new(Player::Two, identity(), [9; 32]).unwrap();
    let ah = a.hello().to_vec();
    let bh = b.hello().to_vec();
    let at = a.receive(&bh).unwrap().unwrap();
    let bt = b.receive(&ah).unwrap().unwrap();
    let ar = a.receive(&bt).unwrap().unwrap();
    let br = b.receive(&at).unwrap().unwrap();
    assert!(a.receive(&br).unwrap().is_none());
    assert!(b.receive(&ar).unwrap().is_none());
    assert!(a.complete() && b.complete());
    (a.into_codec().unwrap(), b.into_codec().unwrap())
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_keeps_every_host_button_direction_and_both_ports() {
    let rows = [
        ((0x01, 0), 0x0001),
        ((0x02, 0), 0x0002),
        ((0x04, 0), 0x0004),
        ((0x08, 0), 0x0008),
        ((0x10, 0), 0x0010),
        ((0x20, 0), 0x0020),
        ((0x40, 0), 0x0040),
        ((0x80, 0), 0x0080),
        ((0, 0x01), 0x0100),
        ((0, 0x02), 0x0200),
        ((0, 0x04), 0x0400),
        ((0, 0x08), 0x0800),
        ((0xff, 0x0f), 0x0fff),
        ((0, 0), 0),
    ];
    for (host, literal) in rows {
        let inputs = from_host([host, (0x80, 0x04)], [ControllerKind::SixButton; 2]).unwrap();
        assert_eq!(inputs.words(), [literal, 0x0480]);
        assert_eq!(hosts(inputs.words()), [host, (0x80, 0x04)]);
    }
    for kind in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
        assert_eq!(project_local_host(kind, 0x40, 0).unwrap(), 0x0040);
        assert_eq!(
            project_local_host(kind, 0xff, 0x0f).unwrap(),
            MdRollbackInputs::word_mask(kind)
        );
        assert!(project_local_host(kind, 0, 0x80).is_err());
    }
    assert_eq!(
        project_local_host(ControllerKind::ThreeButton, 0xb4, 0).unwrap(),
        0
    );
    let mixed = inputs(
        [0x0f4b, 0x0fff],
        [ControllerKind::ThreeButton, ControllerKind::SixButton],
    )
    .unwrap();
    assert_eq!(hosts(mixed.words()), [(0x4b, 0x0f), (0xff, 0x0f)]);
    assert_eq!(
        inputs(
            [0x0fff, 0x0f4b],
            [ControllerKind::SixButton, ControllerKind::ThreeButton]
        )
        .unwrap()
        .words(),
        [0x0fff, 0x0f4b]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_refuses_reserved_and_three_button_extras_before_constructing_a_pair() {
    for (kind, invalid) in [
        (
            ControllerKind::ThreeButton,
            vec![0x04, 0x10, 0x20, 0x80, 0x1000, 0x2000, 0x4000, 0x8000],
        ),
        (
            ControllerKind::SixButton,
            vec![0x1000, 0x2000, 0x4000, 0x8000],
        ),
    ] {
        for bit in invalid {
            for port in 0..2 {
                let mut words = [0x0101, 0x0202];
                words[port] |= bit;
                assert_eq!(words[port] & bit, bit);
                assert_eq!(
                    inputs(words, [kind; 2]).unwrap_err().to_string(),
                    format!("MD port {} has unsupported input bits", port + 1)
                );
            }
        }
    }
    assert_eq!(
        from_host([(0, 0), (0, 0x80)], [ControllerKind::SixButton; 2])
            .unwrap_err()
            .to_string(),
        "MD port 2 has unsupported input bits"
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_words_survive_existing_authenticated_packets_and_datagram_acknowledgment() {
    let (mut a, mut b) = codecs();
    let message = Message::Input {
        player: Player::One,
        frame: 7,
        buttons: 0x0f4b,
    };
    let packet = a.encode(&message).unwrap();
    assert_eq!(b.decode(&packet).unwrap(), message);
    let reply = Message::Input {
        player: Player::Two,
        frame: 8,
        buttons: 0x0fff,
    };
    assert_eq!(a.decode(&b.encode(&reply).unwrap()).unwrap(), reply);
    let mut one = InputChannel::new(Player::One, 0);
    let mut two = InputChannel::new(Player::Two, 0);
    one.push(0, 0x0f4b).unwrap();
    one.push(1, 0x0fff).unwrap();
    let batch = one.encode(&a).unwrap().unwrap();
    assert_eq!(
        two.receive(&b, &batch).unwrap(),
        [
            Message::Input {
                player: Player::One,
                frame: 0,
                buttons: 0x0f4b
            },
            Message::Input {
                player: Player::One,
                frame: 1,
                buttons: 0x0fff
            },
        ]
    );
    assert!(two.receive(&b, &batch).unwrap().is_empty());
    let ack = two.encode(&b).unwrap().unwrap();
    assert!(one.receive(&a, &ack).unwrap().is_empty());
    assert_eq!((one.acknowledged(), one.unacknowledged()), (2, 0));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_specific_validation_does_not_weaken_existing_packet_failure_rules() {
    let (mut a, mut b) = codecs();
    let reserved = Message::Input {
        player: Player::One,
        frame: 0,
        buttons: 0x8000,
    };
    assert_eq!(b.decode(&a.encode(&reserved).unwrap()).unwrap(), reserved);
    assert_eq!(
        inputs([0x8000, 0], [ControllerKind::SixButton; 2])
            .unwrap_err()
            .to_string(),
        "MD port 1 has unsupported input bits"
    );
    let (mut a, mut b) = codecs();
    let message = Message::Input {
        player: Player::One,
        frame: 0,
        buttons: 0x0fff,
    };
    let packet = a.encode(&message).unwrap();
    assert_eq!(b.decode(&packet).unwrap(), message);
    assert!(b.decode(&packet).is_err());
    let (mut a, mut b) = codecs();
    let mut packet = a.encode(&message).unwrap();
    let last = packet.len() - 1;
    packet[last] ^= 1;
    assert!(b.decode(&packet).is_err());
    assert!(b.decode(&a.encode(&message).unwrap()).is_err());
    let (mut a, _) = codecs();
    assert!(
        a.encode(&Message::Input {
            player: Player::Two,
            frame: 0,
            buttons: 0
        })
        .is_err()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn three_button_physical_extras_are_guest_inactive_without_claiming_raw_state_equality() {
    let mut ordinary = Emulator::new(
        crate::netplay::md_fixture::rom(),
        LoadConfig {
            concurrent_sound: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut canonical = ordinary.clone();
    for port in [ControllerPort::One, ControllerPort::Two] {
        ordinary.input_mut().set_host_pad(port, 0xff, 0x0f);
        let projected = project_local_host(ControllerKind::ThreeButton, 0xff, 0x0f).unwrap();
        canonical
            .input_mut()
            .set_host_pad(port, projected as u8, (projected >> 8) as u8);
        assert_eq!(ordinary.input().controller_extra_raw(port), 0);
        assert_eq!(canonical.input().controller_extra_raw(port), 0x0f);
    }
    ordinary.step_field().unwrap();
    canonical.step_field().unwrap();
    assert_eq!(&ordinary.work_ram()[0x100..0x104], &[0x40, 0x40, 0, 0]);
    assert_eq!(
        &ordinary.work_ram()[0x100..0x108],
        &canonical.work_ram()[0x100..0x108]
    );
    assert_eq!(
        crate::netplay::md_fixture::bits(&ordinary.drain_audio_samples()),
        crate::netplay::md_fixture::bits(&canonical.drain_audio_samples())
    );
    assert_ne!(
        ordinary.encode_state().unwrap(),
        canonical.encode_state().unwrap()
    );
}
