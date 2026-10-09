pub const ROUNDS: u8 = 64;

pub fn channel(round: u8) -> u8 {
    round % 2
}

pub fn native_sender(round: u8) -> bool {
    round % 4 < 2
}

pub fn payload(round: u8) -> Vec<u8> {
    let len = if (round / 4).is_multiple_of(2) {
        8
    } else {
        1024
    };
    let mut packet: Vec<_> = (0..len)
        .map(|i| (i as u8).wrapping_mul(37) ^ round)
        .collect();
    packet[..4].copy_from_slice(&[0xd1, round, channel(round), u8::from(native_sender(round))]);
    packet
}

pub fn ack(round: u8) -> Vec<u8> {
    vec![0xa1, round]
}

pub fn validate(kind: u8, packet: &[u8], round: u8, native: bool) -> Result<bool, &'static str> {
    let sequence = *packet.get(1).ok_or("short proof packet")?;
    if sequence > round || sequence >= ROUNDS {
        return Err("future proof packet");
    }
    match packet[0] {
        0xa1 if kind == 0 && native_sender(sequence) == native && packet == ack(sequence) => {}
        0xd1 if kind == channel(sequence)
            && native_sender(sequence) != native
            && packet == payload(sequence) => {}
        _ => return Err("proof packet mismatch"),
    }
    Ok(sequence == round)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_direction_channel_size_and_sequence_are_required() {
        for round in 0..ROUNDS {
            let native = native_sender(round);
            let packet = payload(round);
            assert_eq!(validate(channel(round), &packet, round, !native), Ok(true));
            assert_eq!(validate(0, &ack(round), round, native), Ok(true));
            assert!(validate(channel(round) ^ 1, &packet, round, !native).is_err());
            assert!(validate(channel(round), &packet, round, native).is_err());
            assert!(validate(0, &ack(round), round, !native).is_err());
            let mut damaged = packet.clone();
            *damaged.last_mut().unwrap() ^= 1;
            assert!(validate(channel(round), &damaged, round, !native).is_err());
            assert!(validate(channel(round), &packet[..packet.len() - 1], round, !native).is_err());
            if round > 0 {
                assert!(validate(channel(round), &packet, round - 1, !native).is_err());
                assert_eq!(
                    validate(
                        channel(round - 1),
                        &payload(round - 1),
                        round,
                        !native_sender(round - 1)
                    ),
                    Ok(false)
                );
            }
        }
    }
}
