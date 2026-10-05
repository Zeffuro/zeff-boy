use super::*;
use crate::wire::{Admission, BuildInfo, Identity};

fn pair(first: u64) -> (InputChannel, PacketCodec, InputChannel, PacketCodec) {
    let identity = Identity {
        build: [1; 32],
        build_info: BuildInfo::default(),
        source: [2; 32],
        effective: [2; 32],
        media_len: 16,
        config: [3; 32],
        initial: [4; 32],
        persistent: [5; 32],
        state_format: 11,
    };
    let mut a = Admission::new(Player::One, identity.clone(), [9; 32]).unwrap();
    let mut b = Admission::new(Player::Two, identity, [9; 32]).unwrap();
    let ah = a.hello().to_vec();
    let bh = b.hello().to_vec();
    let at = a.receive(&bh).unwrap().unwrap();
    let bt = b.receive(&ah).unwrap().unwrap();
    let ar = a.receive(&bt).unwrap().unwrap();
    let br = b.receive(&at).unwrap().unwrap();
    a.receive(&br).unwrap();
    b.receive(&ar).unwrap();
    (
        InputChannel::new(Player::One, first),
        a.into_codec().unwrap(),
        InputChannel::new(Player::Two, first),
        b.into_codec().unwrap(),
    )
}

#[test]
fn lost_last_input_and_lost_ack_recover_without_new_frames() {
    let (mut a, ac, mut b, bc) = pair(2);
    a.push(2, 0x07a5).unwrap();
    let dropped = a.encode(&ac).unwrap().unwrap();
    assert_eq!(a.unacknowledged(), 1);
    let retry = a.encode(&ac).unwrap().unwrap();
    assert_eq!(dropped, retry);
    assert_eq!(
        b.receive(&bc, &retry).unwrap(),
        vec![Message::Input {
            player: Player::One,
            frame: 2,
            buttons: 0x07a5
        }]
    );
    let _lost_ack = b.encode(&bc).unwrap().unwrap();
    assert!(
        b.receive(&bc, &a.encode(&ac).unwrap().unwrap())
            .unwrap()
            .is_empty()
    );
    a.receive(&ac, &b.encode(&bc).unwrap().unwrap()).unwrap();
    assert_eq!(a.unacknowledged(), 0);
    assert!(a.encode(&ac).unwrap().is_none());
    assert!(b.encode(&bc).unwrap().is_none());
}

#[test]
fn reversed_duplicates_and_burst_loss_preserve_contiguous_inputs() {
    for first in 0..=8 {
        let (mut a, ac, mut b, bc) = pair(first);
        let mut packets = Vec::new();
        for frame in first..first + 40 {
            a.push(frame, 0x8000 | frame as u16).unwrap();
            packets.push(a.encode(&ac).unwrap().unwrap());
        }
        let mut observed = Vec::new();
        for (i, packet) in packets.iter().enumerate().rev() {
            if i % 3 == 0 {
                continue;
            }
            observed.extend(b.receive(&bc, packet).unwrap());
            observed.extend(b.receive(&bc, packet).unwrap());
        }
        for _ in 0..4 {
            a.receive(&ac, &b.encode(&bc).unwrap().unwrap()).unwrap();
            if let Some(packet) = a.encode(&ac).unwrap() {
                observed.extend(b.receive(&bc, &packet).unwrap());
            }
            if a.unacknowledged() == 0 {
                break;
            }
        }
        assert_eq!(
            observed,
            (first..first + 40)
                .map(|frame| Message::Input {
                    player: Player::One,
                    frame,
                    buttons: 0x8000 | frame as u16
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(a.unacknowledged(), 0);
    }
}

#[test]
fn malicious_packets_leave_history_and_unacked_inputs_intact() {
    let (mut a, ac, mut b, bc) = pair(0);
    a.push(0, 1).unwrap();
    b.push(0, 2).unwrap();
    let packet = a.encode(&ac).unwrap().unwrap();
    b.receive(&bc, &packet).unwrap();
    for change in 0..5 {
        let mut modified = packet.clone();
        match change {
            0 => modified[4] ^= 1,
            1 => modified[36] = 2,
            2 => modified[37..45].copy_from_slice(&2u64.to_be_bytes()),
            3 => modified[45..53].copy_from_slice(&100u64.to_be_bytes()),
            _ => modified[HEADER] = 3,
        }
        let split = modified.len() - SIGNATURE;
        let signature = ac.authenticate_datagram(&modified[..split]);
        modified[split..].copy_from_slice(&signature);
        assert!(b.receive(&bc, &modified).is_err());
        assert_eq!(b.unacknowledged(), 1);
        assert!(b.receive(&bc, &packet).unwrap().is_empty());
    }
    let mut corrupt = packet;
    corrupt[HEADER] ^= 0xff;
    assert!(b.receive(&bc, &corrupt).is_err());
}

#[test]
fn unacked_window_and_sequence_never_allocate_without_bounds() {
    let (mut a, ac, _, _) = pair(0);
    assert!(a.push(1, 0).is_err());
    for frame in 0..WINDOW as u64 {
        a.push(frame, 0).unwrap();
    }
    assert!(a.push(WINDOW as u64, 0).is_err());
    assert!(a.encode(&ac).unwrap().unwrap().len() <= 1024);
    let mut overflow = InputChannel::new(Player::One, u64::MAX);
    assert!(overflow.push(u64::MAX, 0).is_err());
    assert_eq!(overflow.unacknowledged(), 0);
}

#[test]
fn wider_batches_reject_old_version_truncation_and_bad_counts_atomically() {
    let (mut a, ac, mut b, bc) = pair(0);
    a.push(0, 0x07a5).unwrap();
    b.push(0, 0x8001).unwrap();
    let packet = a.encode(&ac).unwrap().unwrap();
    assert_eq!(&packet[HEADER..HEADER + INPUT_BYTES], &[0x07, 0xa5]);
    for length in 0..packet.len() {
        assert!(b.receive(&bc, &packet[..length]).is_err());
        assert_eq!(b.received, 0);
        assert_eq!(b.unacknowledged(), 1);
    }
    for change in 0..5 {
        let mut body = packet[..packet.len() - SIGNATURE].to_vec();
        body[37..45].copy_from_slice(&1u64.to_be_bytes());
        match change {
            0 => body[..4].copy_from_slice(b"ZNDI"),
            1 => {
                body.pop();
            }
            2 => body[53] = (BATCH + 1) as u8,
            3 => body[53] = 0,
            _ => body.push(0),
        }
        let signature = ac.authenticate_datagram(&body);
        body.extend_from_slice(&signature);
        assert!(b.receive(&bc, &body).is_err());
        assert_eq!(b.received, 0);
        assert_eq!(b.unacknowledged(), 1);
    }
    assert_eq!(
        b.receive(&bc, &packet).unwrap()[0],
        Message::Input {
            player: Player::One,
            frame: 0,
            buttons: 0x07a5,
        }
    );
    let mut rewritten = packet[..packet.len() - SIGNATURE].to_vec();
    rewritten[HEADER] ^= 0x04;
    let signature = ac.authenticate_datagram(&rewritten);
    rewritten.extend_from_slice(&signature);
    assert!(b.receive(&bc, &rewritten).is_err());
    assert!(b.receive(&bc, &packet).unwrap().is_empty());
}
