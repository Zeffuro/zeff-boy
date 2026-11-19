use super::*;

fn fresh_control() -> Emulator {
    let (mut rom, irom) = images();
    for (i, word) in [
        0x33fc_u16, 0x000a, 0xa1, 0x5006, 0x33fc, 0, 0xa1, 0x5006, 0x4e71, 0x60fe,
    ]
    .into_iter()
    .enumerate()
    {
        rom[0x100 + i * 2..0x102 + i * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new_synthetic_svp_with_control(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
    )
    .unwrap()
}

fn reject(machine: &mut Emulator, value: u16, lanes: ByteLanes) {
    let before = machine.clone();
    let start = machine.bus.cpu_clock;
    assert!(
        machine
            .bus
            .write_cycle(access(0xa15006, lanes), value, start)
            .result
            .is_err()
    );
    assert!(*machine == before, "control rejection mutated direct bus");
    let mut bus = ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    );
    assert!(
        bus.write_cycle(access(0xa15006, lanes), value, start)
            .result
            .is_err()
    );
    assert!(
        *machine == before,
        "control rejection mutated scheduled bus"
    );
}

pub(super) fn contract() {
    let mut machine = fresh_control();
    let initial = machine.encode_state().unwrap();
    let mut copy = fresh_control();
    copy.decode_state(&initial).unwrap();
    assert!(copy.encode_state().unwrap() == initial);
    let (rom, irom) = images();
    let mut other = Emulator::new_synthetic_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
    )
    .unwrap();
    let original = other.encode_state().unwrap();
    assert!(other.decode_state(&initial).is_err());
    assert!(other.encode_state().unwrap() == original);
    for _ in 0..4 {
        machine.advance_synthetic_svp(true).unwrap();
    }
    for value in [1, 0xb, 0xffff] {
        reject(&mut machine, value, ByteLanes::Both);
    }
    for lanes in [ByteLanes::Upper, ByteLanes::Lower] {
        reject(&mut machine, 0, lanes);
        reject(&mut machine, 0xa, lanes);
    }
    machine.step_instruction().unwrap();
    let halted = machine.encode_state().unwrap();
    assert_eq!(halted.last(), Some(&1));
    for _ in 0..8 {
        assert_eq!(machine.advance_synthetic_svp(true), Ok(Progress::Waiting));
    }
    assert!(machine.encode_state().unwrap() == halted);
    let mut copy = fresh_control();
    copy.decode_state(&halted).unwrap();
    assert!(copy.encode_state().unwrap() == halted);
    assert_eq!(copy.advance_synthetic_svp(true), Ok(Progress::Waiting));
    for value in [&mut machine, &mut copy] {
        value.step_instruction().unwrap();
    }
    assert!(machine.encode_state().unwrap() == copy.encode_state().unwrap());
    assert!(matches!(
        machine.advance_synthetic_svp(true),
        Ok(Progress::Fetched { .. })
    ));
    reject(&mut machine, 0, ByteLanes::Both);
    reject(&mut machine, 0xa, ByteLanes::Both);
    let pending = machine.encode_state().unwrap();
    let mut forged = pending.clone();
    *forged.last_mut().unwrap() = 1;
    assert!(machine.decode_state(&forged).is_err());
    assert!(machine.encode_state().unwrap() == pending);
    let mut malformed = pending.clone();
    *malformed.last_mut().unwrap() = 2;
    assert!(machine.decode_state(&malformed).is_err());
    assert!(machine.encode_state().unwrap() == pending);
    machine.reset();
    assert_eq!(machine.encode_state().unwrap().last(), Some(&0));
    assert!(matches!(
        machine.advance_synthetic_svp(true),
        Ok(Progress::ResetVector { .. })
    ));
}
