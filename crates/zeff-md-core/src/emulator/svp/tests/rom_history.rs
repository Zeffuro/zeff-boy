use super::*;
use crate::svp::{FunctionalProfile, ServiceRole};
use sha2::{Digest, Sha256};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn fixture(region: Region, sound: bool) -> Emulator {
    let mut bytes = vec![0; 0x20000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0x10, 0, 0, 0, 2, 0]);
    bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    bytes[0x800..0x808].copy_from_slice(&[8, 0x10, 0x11, 0x11, 0x4c, 0, 4, 0]);
    bytes[0x1000..0x1003].copy_from_slice(&[0x3e, 0x11, 0x76]);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    Emulator::new_functional_svp(
        bytes,
        LoadConfig {
            region,
            concurrent_sound: sound,
        },
        irom,
        FunctionalProfile::new(0x507, 1, 3, [1, 1, 64, 2, 1, 1]).unwrap(),
    )
    .unwrap()
}

fn advance(machine: &mut Emulator, ticks: u64) {
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .advance_machine(ticks)
        .unwrap();
    } else {
        machine.bus.advance_machine(ticks).unwrap();
    }
}

fn historical(region: Region, sound: bool) -> Emulator {
    let mut machine = fixture(region, sound);
    machine
        .install_rom_patches(&[patch(0x801, 0x20), patch(0x802, 0xa5), patch(0x803, 0xa5)])
        .unwrap();
    for _ in 0..2000 {
        advance(&mut machine, 1);
        if machine
            .synthetic_svp()
            .unwrap()
            .pending_service()
            .unwrap()
            .is_some_and(|request| request.role == ServiceRole::InstructionWord)
        {
            assert!(
                machine
                    .encode_state()
                    .unwrap()
                    .windows(8)
                    .any(|part| part == b"ZSINST02")
            );
            return machine;
        }
    }
    panic!("real functional historical opcode grant was not reached");
}

fn install_current(machine: &mut Emulator, current: Option<u8>) {
    machine.clear_rom_patches();
    if let Some(value) = current {
        machine
            .install_rom_patches(&[
                patch(0x802, value),
                patch(0x803, value),
                patch(0x1001, value),
            ])
            .unwrap();
    }
}

fn restore(receiver: &mut Emulator, donor: &Emulator, wrapper: u8) {
    match wrapper {
        0 => {
            let saved = donor.encode_state().unwrap();
            receiver.decode_state(&saved).unwrap();
            assert_eq!(receiver.encode_state().unwrap(), saved);
        }
        1 => {
            let saved = donor.encode_frontend_state().unwrap();
            receiver.decode_frontend_state(&saved).unwrap();
            assert_eq!(receiver.encode_frontend_state().unwrap(), saved);
        }
        _ => {
            let saved = donor.encode_debugger_checkpoint().unwrap();
            receiver.decode_debugger_checkpoint(&saved).unwrap();
            assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), saved);
        }
    }
}

fn continuation_oracle(donor: &Emulator, wrapper: u8) -> Emulator {
    let mut reference = donor.clone();
    if wrapper == 0 {
        // RAW restarts frontend output.
        reference.set_sample_rate(reference.sample_rate());
    }
    reference
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_historical_opcode_restore_preserves_receiver_mask_and_remaining_cost() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for sound in [false, true] {
            let donor = historical(region, sound);
            let request = donor.synthetic_svp().unwrap().pending_service().unwrap();
            let timing = donor.synthetic_svp().unwrap().functional_timing();
            for current in [Some(0xb6), None] {
                for wrapper in 0..3 {
                    let mut receiver = fixture(region, sound);
                    install_current(&mut receiver, current);
                    let mask = receiver.bus.cartridge.rom_overlay.clone();
                    restore(&mut receiver, &donor, wrapper);
                    assert_eq!(receiver.bus.cartridge.rom_overlay, mask);
                    assert_eq!(
                        receiver.synthetic_svp().unwrap().pending_service().unwrap(),
                        request
                    );
                    assert_eq!(
                        receiver.synthetic_svp().unwrap().functional_timing(),
                        timing
                    );
                    let mut reference = continuation_oracle(&donor, wrapper);
                    install_current(&mut reference, current);
                    for _ in 0..192 {
                        advance(&mut receiver, 1);
                        advance(&mut reference, 1);
                    }
                    assert_eq!(receiver.synthetic_svp(), reference.synthetic_svp());
                    assert_eq!(
                        receiver.synthetic_svp().unwrap().registers().y,
                        current.map_or(0x1111, |value| u16::from_be_bytes([value; 2]))
                    );
                    assert!(
                        receiver.encode_frontend_state().unwrap()
                            == reference.encode_frontend_state().unwrap(),
                        "frontend continuation: {region:?} sound={sound} wrapper={wrapper} current={current:?}"
                    );
                    assert!(!receiver.supports_rom_breakpoints());
                    assert_eq!(receiver.rom_offset_for_cpu_address(0x802), None);
                }
            }
        }
    }
    let synthetic = fresh();
    assert!(!synthetic.supports_rom_cheats());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_sound_history_roundtrips_all_wrappers_and_generates_same_nonzero_pcm() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut donor = fixture(region, true);
        donor.install_rom_patches(&[patch(0x1001, 0xa5)]).unwrap();
        donor.bus.sound.set_reset(false);
        donor.sound_cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
        donor.sound_schedule.pending = Some(
            crate::hardware::sound::schedule::PendingSound::prepare(
                &donor.sound_cpu,
                &donor.bus.sound,
                &donor.bus.cartridge,
                donor.bus.now.get(),
            )
            .unwrap(),
        );
        assert!(donor.sound_schedule.has_svp_history());
        assert_eq!(
            donor.sound_schedule.pending.as_ref().unwrap().cpu.regs().a,
            0xa5
        );
        for value in [0x80, 0x10, 0x90] {
            donor.bus.audio.write_psg(donor.bus.now.get(), value);
        }
        for current in [Some(0xb6), None] {
            for wrapper in 0..3 {
                let mut receiver = fixture(region, true);
                install_current(&mut receiver, current);
                let mask = receiver.bus.cartridge.rom_overlay.clone();
                restore(&mut receiver, &donor, wrapper);
                assert_eq!(receiver.bus.cartridge.rom_overlay, mask);
                let mut reference = continuation_oracle(&donor, wrapper);
                install_current(&mut reference, current);
                for _ in 0..2048 {
                    advance(&mut receiver, 15);
                    advance(&mut reference, 15);
                }
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                assert_eq!(receiver.sound_schedule, reference.sound_schedule);
                assert_eq!(receiver.synthetic_svp(), reference.synthetic_svp());
                let pcm = receiver.drain_audio_samples();
                assert!(pcm.iter().any(|sample| *sample != 0.0));
                assert!(
                    pcm == reference.drain_audio_samples(),
                    "PCM continuation: {region:?} wrapper={wrapper} current={current:?}"
                );
            }
        }
    }
}

fn bad_front(raw: &[u8], frontend: &[u8]) -> Vec<u8> {
    let mut r = StateReader::new(frontend);
    r.set_position(12);
    r.read_slice(crate::save_state::MAX_STATE_BYTES).unwrap();
    let audio = r
        .read_slice(crate::hardware::audio::MAX_FRONTEND_AUDIO_BYTES)
        .unwrap();
    let mut w = StateWriter::new();
    w.write_bytes(b"ZMDFRONT");
    w.write_u32(1);
    w.write_vec(raw);
    w.write_vec(audio);
    w.into_bytes()
}

fn bad_owned(frontend: &[u8], owned: &[u8]) -> Vec<u8> {
    let mut r = StateReader::new(owned);
    r.set_position(76);
    let old_front = r.read_slice(MAX_FRONTEND_STATE_BYTES).unwrap();
    assert_eq!(old_front.len(), frontend.len());
    let mut out = owned.to_vec();
    out[80..80 + frontend.len()].copy_from_slice(frontend);
    out[44..76].copy_from_slice(&Sha256::digest(frontend));
    let end = out.len() - 32;
    let checksum = Sha256::digest(&out[..end]);
    out[end..].copy_from_slice(&checksum);
    out
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn historical_bad_nested_tags_and_semantics_reject_atomically_after_outer_rebinding() {
    let mut donor = historical(Region::NtscUs, true);
    donor.install_rom_patches(&[patch(0x1001, 0xa5)]).unwrap();
    donor.bus.sound.set_reset(false);
    donor.sound_cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    donor.sound_schedule.pending = Some(
        crate::hardware::sound::schedule::PendingSound::prepare(
            &donor.sound_cpu,
            &donor.bus.sound,
            &donor.bus.cartridge,
            donor.bus.now.get(),
        )
        .unwrap(),
    );
    let raw = donor.encode_state().unwrap();
    let front = donor.encode_frontend_state().unwrap();
    let owned = donor.encode_debugger_checkpoint().unwrap();
    let mut offsets = Vec::new();
    for tag in [
        b"ZMDSVPA2",
        b"ZSINST02",
        b"ZSVPROM2",
        b"ZSNDROM2",
        b"ZSVPSVC1",
        b"ZSVPFNC1",
    ] {
        let at = raw.windows(8).position(|part| part == tag).unwrap();
        offsets.extend([at, at + 7, at + 8]);
    }
    let at = raw.windows(8).position(|part| part == b"ZSVPSVC1").unwrap();
    offsets.extend([0, 8, 10, 11, 12, 13, 14, 18, 20, 22, 23, 25].map(|offset| at + 9 + offset));
    for offset in offsets {
        let mut corrupt = raw.clone();
        corrupt[offset] ^= 0xff;
        let corrupt_front = bad_front(&corrupt, &front);
        let corrupt_owned = bad_owned(&corrupt_front, &owned);
        for wrapper in 0..3 {
            let mut receiver = donor.clone();
            install_current(&mut receiver, Some(0xb6));
            let before = receiver.clone();
            let result = match wrapper {
                0 => receiver.decode_state(&corrupt),
                1 => receiver.decode_frontend_state(&corrupt_front),
                _ => receiver.decode_debugger_checkpoint(&corrupt_owned),
            };
            assert!(result.is_err(), "wrapper {wrapper}, offset {offset}");
            assert_eq!(receiver, before);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_legacy_state_keeps_exact_grammar_under_receiver_mask() {
    let mut donor = fixture(Region::NtscUs, false);
    for _ in 0..2000 {
        if donor
            .synthetic_svp()
            .unwrap()
            .pending_service()
            .unwrap()
            .is_some_and(|request| request.role == ServiceRole::InstructionWord)
        {
            break;
        }
        advance(&mut donor, 1);
    }
    assert!(
        donor
            .synthetic_svp()
            .unwrap()
            .pending_service()
            .unwrap()
            .is_some_and(|request| request.role == ServiceRole::InstructionWord)
    );
    let raw = donor.encode_state().unwrap();
    assert!(raw.windows(8).any(|part| part == b"ZMDSVPS1"));
    assert!(!raw.windows(8).any(|part| part == b"ZMDSVPA2"));
    for wrapper in 0..3 {
        let mut receiver = fixture(Region::NtscUs, false);
        receiver.install_rom_patches(&[patch(0x801, 0x20)]).unwrap();
        let overlay = receiver.bus.cartridge.rom_overlay.clone();
        restore(&mut receiver, &donor, wrapper);
        assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
        assert_eq!(receiver.encode_state().unwrap(), raw);
    }
}

fn main_fixture() -> Emulator {
    let original = fixture(Region::NtscUs, false);
    let mut rom = original.rom_bytes().to_vec();
    let words = [0x7001_u16, 0x3039, 0, 0x1000, 0x4e71, 0x60fe];
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
        FunctionalProfile::new(0x507, 1, 3, [1, 1, 64, 2, 1, 1]).unwrap(),
    )
    .unwrap()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_main_reset_prefetch_and_data_reads_share_current_cartridge_mask() {
    use zeff_emu_common::time::Reset;
    let mut donor = main_fixture();
    donor
        .install_rom_patches(&[patch(0x201, 7), patch(0x1000, 0xa5), patch(0x1001, 0x5a)])
        .unwrap();
    donor.reset();
    assert_eq!(donor.queued_words(), Some([0x7007, 0x3039]));
    for wrapper in 0..3 {
        let mut receiver = main_fixture();
        receiver
            .install_rom_patches(&[patch(0x201, 9), patch(0x1000, 0xc7), patch(0x1001, 0x7c)])
            .unwrap();
        let overlay = receiver.bus.cartridge.rom_overlay.clone();
        restore(&mut receiver, &donor, wrapper);
        assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
        assert_eq!(receiver.queued_words(), Some([0x7007, 0x3039]));
        receiver.step_instruction().unwrap();
        assert_eq!(receiver.registers().d[0], 7);
        receiver.step_instruction().unwrap();
        assert_eq!(receiver.registers().d[0] & 0xffff, 0xc77c);
        assert_eq!(receiver.peek(0x1000), Some(0xc7));
        crate::VdpDmaSource::from_cartridge(&receiver.bus.cartridge, &receiver.bus.ram)
            .assert_word(0x1000, 0xc77c);
        receiver.reset();
        assert_eq!(receiver.queued_words(), Some([0x7009, 0x3039]));
        receiver.clear_rom_patches();
        receiver.reset();
        assert_eq!(receiver.queued_words(), Some([0x7001, 0x3039]));
        crate::VdpDmaSource::from_cartridge(&receiver.bus.cartridge, &receiver.bus.ram)
            .assert_word(0x1000, 0x3e11);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn functional_dma_fifo_keeps_sample_while_next_source_uses_receiver_mask() {
    let mut donor = fixture(Region::NtscUs, false);
    donor
        .install_rom_patches(&[
            patch(0x1000, 0xa5),
            patch(0x1001, 0x5a),
            patch(0x1002, 0xb6),
            patch(0x1003, 0x6b),
        ])
        .unwrap();
    for word in [0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9700] {
        donor.bus.vdp.write_control(word, donor.region()).unwrap();
        if let Some(pending) = donor.bus.vdp.registers().pending() {
            let ticks = pending.publish_at.get() - donor.bus.now.get();
            advance(&mut donor, ticks);
        }
    }
    donor.bus.vdp.write_control(0x4400, donor.region()).unwrap();
    donor
        .bus
        .vdp
        .write_control_from(
            0x0080,
            donor.region(),
            crate::VdpDmaSource::from_cartridge(&donor.bus.cartridge, &donor.bus.ram),
        )
        .unwrap();
    for _ in 0..2000 {
        if donor
            .vdp()
            .fifo()
            .front()
            .is_some_and(|entry| entry.data == 0xa55a)
        {
            break;
        }
        advance(&mut donor, 1);
    }
    let first = donor
        .vdp()
        .fifo()
        .front()
        .expect("actual sampled SVP DMA source");
    assert_eq!(first.data, 0xa55a);
    for wrapper in 0..3 {
        let mut receiver = fixture(Region::NtscUs, false);
        receiver
            .install_rom_patches(&[
                patch(0x1000, 0xc7),
                patch(0x1001, 0x7c),
                patch(0x1002, 0xd8),
                patch(0x1003, 0x8d),
            ])
            .unwrap();
        restore(&mut receiver, &donor, wrapper);
        assert_eq!(receiver.vdp().fifo().front(), Some(first));
        advance(&mut receiver, 2000);
        assert_eq!(&receiver.vdp().memory().vram()[0x400..0x402], &[0xa5, 0x5a]);
        assert_eq!(&receiver.vdp().memory().vram()[0x402..0x404], &[0xd8, 0x8d]);
    }
}

#[path = "rom_history/owners.rs"]
mod owners;

#[path = "rom_history/original_replay.rs"]
mod original_replay;
