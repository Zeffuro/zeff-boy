use super::*;
use crate::constants::SOUND_CPU_DIVISOR;
use crate::hardware::bus::ownership::{Requester, Reservation};

const VALUES: [&[u8]; 4] = [
    &[0x55, 0x0f, 0xc9, 0x18],
    &[0x55, 0x0f, 0xaa, 0xf0],
    &[0x55, 0x0f],
    &[0x63, 0x98, 0xc9, 0x18],
];

fn machine(profile: usize, words: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    Emulator::new(
        crate::cartridge::protection::tests::rom(profile, words),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
    assert_eq!(machine.sound_fault(), None);
}

fn check_profile(machine: &Emulator, profile: usize) {
    assert!(machine.bus.cartridge.protection.is_some());
    assert!(machine.bus.cartridge.registers.is_none());
    assert!(machine.bus.cartridge.save.is_none());
    for (index, &value) in VALUES[profile].iter().enumerate() {
        assert_eq!(machine.peek(0x400000 + index as u32 * 2), Some(value));
    }
}

fn compare_core(source: &Emulator, other: &Emulator, legacy_clock: bool) {
    let expected = source.encode_state().unwrap();
    let actual = other.encode_state().unwrap();
    if legacy_clock {
        assert_eq!((&actual), (&expected));
    } else {
        assert_eq!(actual, expected);
    }
    assert_eq!(other.registers(), source.registers());
    assert_eq!(other.queued_words(), source.queued_words());
    assert_eq!(other.framebuffer(), source.framebuffer());
    assert_eq!(
        other.framebuffer_dimensions(),
        source.framebuffer_dimensions()
    );
    assert_eq!(other.video_error(), source.video_error());
}

fn restore_copies(source: &Emulator, profile: usize, words: &[u16]) -> [Emulator; 3] {
    let raw = source.encode_state().unwrap();
    let front = source.encode_frontend_state().unwrap();
    assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
    assert_eq!(&front[8..12], &1_u32.to_le_bytes());
    let mut raw_copy = machine(profile, words, source.region(), source.concurrent_sound);
    raw_copy.decode_state(&raw).unwrap();
    let mut front_copy = machine(profile, words, source.region(), source.concurrent_sound);
    front_copy.decode_frontend_state(&front).unwrap();
    assert_eq!((&raw_copy.encode_state().unwrap()), (&raw));
    assert_eq!(front_copy.encode_frontend_state().unwrap(), front);
    [source.clone(), raw_copy, front_copy]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_main_reads_writes_state_and_reset() {
    let mut inputs = 0;
    let mut addresses = 0;
    for (profile, values) in VALUES.into_iter().enumerate() {
        let mut words = Vec::new();
        for index in 0..values.len() {
            let offset = index as u16 * 2;
            words.extend_from_slice(&[0x1039, 0x0040, offset, 0x3239, 0x0040, offset]);
            words.extend_from_slice(&[0x13fc, 0x00a6, 0x0040, offset]);
            words.extend_from_slice(&[0x33fc, 0xa637, 0x0040, offset]);
            words.extend_from_slice(&[0x1439, 0x0040, offset, 0x3639, 0x0040, offset]);
        }
        words.push(0x60fe);
        for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
            for concurrent in [false, true] {
                let mut source = machine(profile, &words, region, concurrent);
                check_profile(&source, profile);
                source.step_instruction().unwrap();
                assert_eq!(source.registers().d[0], u32::from(values[0]));
                let mut copies = restore_copies(&source, profile, &words);
                for (index, &value) in values.iter().enumerate() {
                    for phase in usize::from(index == 0)..6 {
                        let result = source.step_instruction().unwrap();
                        for copy in &mut copies {
                            assert_eq!(copy.step_instruction().unwrap(), result);
                            assert_eq!(copy.registers(), source.registers());
                        }
                        match phase {
                            0 => assert_eq!(source.registers().d[0] & 0xff, u32::from(value)),
                            1 => assert_eq!(source.registers().d[1] & 0xffff, u32::from(value)),
                            4 => assert_eq!(source.registers().d[2] & 0xff, u32::from(value)),
                            5 => assert_eq!(source.registers().d[3] & 0xffff, u32::from(value)),
                            _ => (),
                        }
                        check_profile(&source, profile);
                    }
                    addresses += 1;
                }
                for (index, copy) in copies.iter().enumerate() {
                    compare_core(&source, copy, index == 1);
                }
                assert_eq!(
                    copies[0].encode_frontend_state().unwrap(),
                    source.encode_frontend_state().unwrap()
                );
                assert_eq!(
                    copies[2].encode_frontend_state().unwrap(),
                    source.encode_frontend_state().unwrap()
                );
                let expected_pcm: Vec<_> = source
                    .drain_audio_samples()
                    .into_iter()
                    .map(f32::to_bits)
                    .collect();
                assert_eq!(
                    copies[0]
                        .drain_audio_samples()
                        .into_iter()
                        .map(f32::to_bits)
                        .collect::<Vec<_>>(),
                    expected_pcm
                );
                assert_eq!(
                    copies[2]
                        .drain_audio_samples()
                        .into_iter()
                        .map(f32::to_bits)
                        .collect::<Vec<_>>(),
                    expected_pcm
                );
                let before = source.timing_snapshot().now();
                source.reset_cpu().unwrap();
                assert!(source.timing_snapshot().now() > before);
                check_profile(&source, profile);
                source.reset();
                check_profile(&source, profile);
                let fresh = machine(profile, &words, region, concurrent);
                assert_eq!(
                    source.encode_state().unwrap(),
                    fresh.encode_state().unwrap()
                );
                inputs += 1;
            }
        }
    }
    assert_eq!((inputs, addresses), (24, 84));
    sparse_upper_pair_byte_consumers_state_and_reset();
}

fn sound_program(index: usize, indirect: bool) -> Vec<u8> {
    let low = index as u8 * 2;
    if indirect {
        vec![0x21, low, 0x80, 0x7e, 0x47, 0x7e, 0x76]
    } else {
        vec![0x3a, low, 0x80, 0x47, 0x3a, low, 0x80, 0x76]
    }
}

fn prepare_sound(machine: &mut Emulator, index: usize, indirect: bool) {
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x80;
    let program = sound_program(index, indirect);
    machine.bus.sound.ram[..program.len()].copy_from_slice(&program);
    for _ in 0..400 {
        advance(machine, 1);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| pending.any_bank_access)
        {
            let pending = machine.sound_schedule.pending.as_ref().unwrap();
            assert_eq!(
                pending.bank_access.map(|access| access.address),
                Some(0x400000 + index as u32 * 2)
            );
            assert_eq!(
                pending.reservation().map(|slot| slot.requester),
                Some(Requester::SOUND)
            );
            assert_eq!(
                (pending.instruction.opcode, pending.instruction.cycles),
                (if indirect { 0x7e } else { 0x3a }, 3)
            );
            assert_eq!(pending.register_writes, Default::default());
            assert_eq!(machine.sound_cpu.regs().a, 0);
            return;
        }
    }
    panic!("fixed read phase was not reached");
}

fn wait_sound(machine: &mut Emulator, indirect: bool, requester: Requester) -> u64 {
    let pending = machine.sound_schedule.pending.as_mut().unwrap();
    if indirect {
        assert_eq!(
            (pending.instruction.opcode, pending.instruction.cycles),
            (0x7e, 3)
        );
    }
    let start = pending.start;
    let deadline = pending.deadline;
    let release = deadline + 20 * SOUND_CPU_DIVISOR;
    let wait = (release - start).div_ceil(SOUND_CPU_DIVISOR) * SOUND_CPU_DIVISOR;
    pending
        .reserve_after(&[Reservation {
            requester,
            ticks: start..release,
        }])
        .unwrap();
    assert_eq!(
        (pending.start, pending.deadline),
        (start + wait, deadline + wait)
    );
    machine.bus.sound.bank = 0x1ff;
    deadline + wait
}

fn sound_case(
    profile: usize,
    index: usize,
    indirect: bool,
    requester: Requester,
    legacy: bool,
    region: Region,
) {
    let words = [0x60fe];
    let mut source = machine(profile, &words, region, true);
    let protection = source.bus.cartridge.protection;
    if legacy {
        source.bus.cartridge.protection = None;
    }
    prepare_sound(&mut source, index, indirect);
    source.bus.cartridge.protection = protection;
    let expected = if legacy { 0xff } else { VALUES[profile][index] };
    let access = source
        .sound_schedule
        .pending
        .as_ref()
        .unwrap()
        .bank_access
        .unwrap();
    assert_eq!(access.direction, zeff_m68k::bus::Direction::Read);
    assert_eq!(access.value, expected);
    let deadline = wait_sound(&mut source, indirect, requester);
    let mut copies = restore_copies(&source, profile, &words);
    for machine in std::iter::once(&mut source).chain(copies.iter_mut()) {
        let now = machine.timing_snapshot().now().get();
        advance(machine, deadline - now - 1);
        assert_eq!(machine.sound_cpu.regs().a, 0);
        assert_eq!(machine.sound_deadline().unwrap().get(), deadline);
        advance(machine, 1);
        assert_eq!(machine.sound_cpu.regs().a, expected);
        assert_eq!(machine.bus.sound.bank, 0x80);
        assert!(machine.sound_schedule.pending.is_none());
        advance(machine, 600);
        assert_eq!(machine.sound_cpu.regs().b, expected);
        assert_eq!(machine.sound_cpu.regs().a, VALUES[profile][index]);
        check_profile(machine, profile);
    }
    for (index, copy) in copies.iter().enumerate() {
        compare_core(&source, copy, index == 1);
    }
    assert_eq!(
        copies[0].encode_frontend_state().unwrap(),
        source.encode_frontend_state().unwrap()
    );
    assert_eq!(
        copies[2].encode_frontend_state().unwrap(),
        source.encode_frontend_state().unwrap()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_sound_reads_restore_constants_and_bank_ownership() {
    let mut inputs = 0;
    for (profile, values) in VALUES.into_iter().enumerate() {
        for index in 0..values.len() {
            for indirect in [false, true] {
                for requester in [Requester::MAIN, Requester::VDP] {
                    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
                        sound_case(profile, index, indirect, requester, false, region);
                        inputs += 1;
                    }
                }
            }
        }
    }
    assert_eq!(inputs, 168);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_historical_pending_bytes_finish_before_new_reads() {
    let mut inputs = 0;
    for (profile, values) in VALUES.into_iter().enumerate() {
        for index in 0..values.len() {
            for indirect in [false, true] {
                for requester in [Requester::MAIN, Requester::VDP] {
                    sound_case(profile, index, indirect, requester, true, Region::NtscUs);
                    inputs += 1;
                }
            }
        }
    }
    assert_eq!(inputs, 56);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_decode_failure_preserves_derived_board() {
    for profile in 0..VALUES.len() {
        let words = [0x60fe, 0x4e71];
        let mut source = machine(profile, &words, Region::NtscUs, true);
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        let before = source.clone();
        assert!(source.decode_state(&raw[..raw.len() - 1]).is_err());
        assert_eq!(source, before);
        assert!(
            source
                .decode_frontend_state(&front[..front.len() - 1])
                .is_err()
        );
        assert_eq!(source, before);
        let wrong = machine((profile + 1) % VALUES.len(), &words, Region::NtscUs, true)
            .encode_state()
            .unwrap();
        assert!(source.decode_state(&wrong).is_err());
        assert_eq!(source, before);
        check_profile(&source, profile);
        source.bus.cartridge.protection = None;
        assert!(source.encode_state().is_err());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_sound_noop_writes_and_disconnected_boundaries() {
    let mut inputs = 0;
    for profile in 0..VALUES.len() {
        for target in [0x400000_u32, 0x400001, 0x400008, 0x40479c, 0x40ffff] {
            let words = [0x60fe, 0x4e71];
            let mut source = machine(profile, &words, Region::NtscUs, true);
            source.bus.sound.set_reset(false);
            let bank = (target >> 15) as u16;
            source.bus.sound.bank = bank;
            let address = (0x8000 | (target & 0x7fff)) as u16;
            let [low, high] = address.to_le_bytes();
            let value = if target == 0x40479c { 1 } else { 0x7b };
            let program = if target == 0x40479c {
                assert_eq!((bank, address), (0x80, 0xc79c));
                vec![0x21, low, high, 0x3e, value, 0x77, 0x3a, low, high, 0x76]
            } else {
                vec![0x3e, value, 0x32, low, high, 0x3a, low, high, 0x76]
            };
            source.bus.sound.ram[..program.len()].copy_from_slice(&program);
            for _ in 0..400 {
                advance(&mut source, 1);
                if source
                    .sound_schedule
                    .pending
                    .as_ref()
                    .is_some_and(|pending| {
                        pending.bank_access.is_some_and(|access| {
                            access.direction == zeff_m68k::bus::Direction::Write
                        })
                    })
                {
                    break;
                }
            }
            let pending = source.sound_schedule.pending.as_ref().unwrap();
            let access = pending.bank_access.unwrap();
            assert_eq!((access.address, access.value), (target, value));
            if target == 0x40479c {
                assert_eq!(
                    (pending.instruction.opcode, pending.instruction.cycles),
                    (0x77, 3)
                );
            }
            assert_eq!(pending.register_writes, Default::default());
            assert_eq!(pending.reservation().unwrap().requester, Requester::SOUND);
            let cartridge = source.bus.cartridge.clone();
            let expected = cartridge.protection.unwrap().read(target).unwrap_or(0xff);
            let deadline = wait_sound(&mut source, false, Requester::MAIN);
            let mut copies = restore_copies(&source, profile, &words);
            for machine in std::iter::once(&mut source).chain(copies.iter_mut()) {
                let now = machine.timing_snapshot().now().get();
                advance(machine, deadline - now - 1);
                assert_eq!(machine.bus.cartridge, cartridge);
                advance(machine, 1);
                assert_eq!(machine.bus.sound.bank, bank);
                assert_eq!(machine.bus.cartridge, cartridge);
                advance(machine, 600);
                assert_eq!(machine.sound_cpu.regs().a, expected);
                assert_eq!(machine.bus.cartridge, cartridge);
            }
            for (index, copy) in copies.iter().enumerate() {
                compare_core(&source, copy, index == 1);
            }
            inputs += 1;
        }
    }
    assert_eq!(inputs, 20);
    sparse_upper_pair_sound_boundaries();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_stopped_main_allows_unbounded_sound_reads_and_noop_writes() {
    let mut inputs = 0;
    for (profile, values) in VALUES.iter().enumerate() {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            for indirect in [false, true] {
                let mut source = machine(profile, &[0x4e72, 0x2700, 0x4e71], region, false);
                source.step_instruction().unwrap();
                assert!(source.main_cpu_stopped());
                assert_eq!(
                    source.step_instruction().unwrap().cpu,
                    TimedOutcome::Stopped
                );
                source.bus.sound.set_reset(false);
                source.bus.sound.bank = 0x80;
                let program: &[u8] = if indirect {
                    &[0x21, 0, 0x80, 0x36, 0x7b, 0x7e, 0x76]
                } else {
                    &[0x3e, 0x7b, 0x32, 0, 0x80, 0x3a, 0, 0x80, 0x76]
                };
                source.bus.sound.ram[..program.len()].copy_from_slice(program);
                let cartridge = source.bus.cartridge.clone();
                for _ in 0..3 {
                    source.step_sound_instruction().unwrap();
                }
                assert_eq!(source.sound_cpu.regs().a, values[0]);
                assert_eq!(source.bus.cartridge, cartridge);
                inputs += 1;
            }
        }
    }
    assert_eq!(inputs, 24);
}

fn sparse_upper_pair_byte_consumers_state_and_reset() {
    let words = [
        0x13f9, 0x0040, 0x0000, 0x00ff, 0xff0c, 0x13f9, 0x0040, 0x1000, 0x00ff, 0xff0e, 0x60fe,
    ];
    for region in [Region::NtscUs, Region::Pal, Region::NtscJapan] {
        for concurrent in [false, true] {
            let mut source = machine(4, &words, region, concurrent);
            source.step_instruction().unwrap();
            assert_eq!(source.peek(0xffff0c), Some(0x90));
            assert_eq!(source.peek(0xffff0e), Some(0));
            let mut copies = restore_copies(&source, 4, &words);
            for _ in 0..64 {
                let result = source.step_instruction().unwrap();
                for copy in &mut copies {
                    assert_eq!(copy.step_instruction().unwrap(), result);
                    assert_eq!(copy.registers(), source.registers());
                }
            }
            assert_eq!(source.peek(0xffff0c), Some(0x90));
            assert_eq!(source.peek(0xffff0e), Some(0xd3));
            for (index, copy) in copies.iter().enumerate() {
                compare_core(&source, copy, index == 1);
                assert_eq!(copy.peek(0xffff0c), Some(0x90));
                assert_eq!(copy.peek(0xffff0e), Some(0xd3));
            }
            for copy in [&copies[0], &copies[2]] {
                assert_eq!(
                    copy.encode_frontend_state().unwrap(),
                    source.encode_frontend_state().unwrap()
                );
            }
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            let before = source.clone();
            assert!(source.decode_state(&raw[..raw.len() - 1]).is_err());
            assert_eq!(source, before);
            assert!(
                source
                    .decode_frontend_state(&front[..front.len() - 1])
                    .is_err()
            );
            assert_eq!(source, before);
            let wrong = machine(0, &words, region, concurrent)
                .encode_state()
                .unwrap();
            assert!(source.decode_state(&wrong).is_err());
            assert_eq!(source, before);
            let board = source.bus.cartridge.protection;
            source.bus.cartridge.protection = None;
            assert!(source.encode_state().is_err());
            source.bus.cartridge.protection = board;
            source.reset_cpu().unwrap();
            assert_eq!(source.peek(0x400000), Some(0x90));
            assert_eq!(source.peek(0x401000), Some(0xd3));
            source.reset();
            assert_eq!(
                source.encode_state().unwrap(),
                machine(4, &words, region, concurrent)
                    .encode_state()
                    .unwrap()
            );
        }
    }
}

fn sparse_upper_pair_sound_boundaries() {
    use crate::hardware::sound::SoundFault;
    use zeff_m68k::bus::Direction;

    for target in [
        0x400000_u32,
        0x401000,
        0x400001,
        0x400002,
        0x401001,
        0x401002,
        0x40ffff,
    ] {
        for write in [false, true] {
            let mut source = machine(4, &[0x4e72, 0x2700, 0x60fe], Region::NtscUs, false);
            source.step_instruction().unwrap();
            assert!(source.main_cpu_stopped());
            assert_eq!(
                source.step_instruction().unwrap().cpu,
                TimedOutcome::Stopped
            );
            source.bus.sound.set_reset(false);
            source.bus.sound.bank = (target >> 15) as u16;
            let address = (0x8000 | (target & 0x7fff)) as u16;
            let [low, high] = address.to_le_bytes();
            source.bus.sound.ram[..4].copy_from_slice(&[
                if write { 0x32 } else { 0x3a },
                low,
                high,
                0x76,
            ]);
            let before = source.clone();
            if !write && matches!(target, 0x400000 | 0x401000) {
                source.step_sound_instruction().unwrap();
                assert_eq!(
                    source.sound_cpu.regs().a,
                    if target == 0x400000 { 0x90 } else { 0xd3 }
                );
                assert_eq!(source.bus.cartridge, before.bus.cartridge);
            } else {
                assert_eq!(
                    source.step_sound_instruction(),
                    Err(SoundFault::UnsupportedAccess {
                        address,
                        direction: if write {
                            Direction::Write
                        } else {
                            Direction::Read
                        },
                    })
                );
                assert_eq!(source, before);
            }
        }
    }
}
