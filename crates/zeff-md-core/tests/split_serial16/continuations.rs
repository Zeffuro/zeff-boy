use super::*;
use zeff_emu_common::time::{MachineTiming, Reset};

struct Program {
    bytes: Vec<u8>,
    cuts: Vec<u32>,
    held: [u32; 3],
    dirty: u32,
    busy: u32,
    busy_nack: u32,
    busy_ack: usize,
    value: usize,
}

fn send_cut(g: &mut Guest, byte: u8, held: bool) -> (usize, Option<[u32; 3]>) {
    for bit in (0..8).rev() {
        let data = (byte >> bit) & 1;
        g.line(data);
        g.line(data | 2);
        if bit == 2 {
            g.mark();
        }
        g.line(data);
    }
    g.line(1);
    g.line(3);
    let ack = g.rom.sample();
    let cuts = held.then(|| {
        let a = g.mark();
        g.mapping(true);
        let b = g.mark();
        g.mapping(false);
        [a, b, g.mark()]
    });
    g.line(1);
    (ack, cuts)
}

fn program(active: bool) -> Program {
    let mut g = guest();
    if active {
        for value in [0x84, 1, 0x90] {
            g.rom.write(0x13fc, 0xc00011, value);
        }
        g.wait();
    }
    g.mapping(false);
    g.start();
    let (_, held) = send_cut(&mut g, 0xaa, true);
    send_cut(&mut g, 0xfd, false);
    for value in [0x96, 0x3a, 0x57] {
        g.send(value);
    }
    let dirty = g.mark();
    g.stop();
    let busy = g.mark();
    g.start();
    let busy_ack = g.send(0xaf);
    let busy_nack = g.mark();
    g.stop();
    g.wait();
    read(&mut g, 0x5fd);
    let value = g.receive(false, true);
    g.stop();
    let cuts = g.cuts.clone();
    let (bytes, _) = g.rom.finish();
    Program {
        bytes,
        cuts,
        held: held.unwrap(),
        dirty,
        busy,
        busy_nack,
        busy_ack,
        value,
    }
}

fn equal_future(mut actual: Emulator, mut expected: Emulator, pcm: bool) {
    advance(&mut actual, None);
    advance(&mut expected, None);
    if pcm {
        for _ in 0..4096 {
            assert_eq!(
                actual.step_instruction().unwrap(),
                expected.step_instruction().unwrap()
            );
        }
    }
    assert_eq!(
        actual.encode_state().unwrap(),
        expected.encode_state().unwrap()
    );
    assert_eq!(actual.framebuffer(), expected.framebuffer());
    assert_eq!(actual.dump_save_ram(), expected.dump_save_ram());
    if pcm {
        let a = audio(&mut actual);
        let b = audio(&mut expected);
        assert!(!a.is_empty() && a.iter().any(|value| value & 0x7fff_ffff != 0));
        assert_eq!(a, b);
    }
}

fn audio(machine: &mut Emulator) -> Vec<u32> {
    let mut samples = Vec::new();
    machine.drain_audio_samples_into(&mut samples);
    samples.into_iter().map(f32::to_bits).collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn partial_banked_control_and_low_address_continue_after_restore() {
    let p = program(false);
    let initial = cells();
    let mut expected = initial.clone();
    expected[0x5fd..0x600].copy_from_slice(&[0x96, 0x3a, 0x57]);
    for config in configs() {
        let mut machine = Emulator::new(p.bytes.clone(), config).unwrap();
        machine.load_save_ram(&initial).unwrap();
        let mut held = None;
        for pc in &p.cuts {
            advance(&mut machine, Some(*pc));
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            if let Some(index) = p.held.iter().position(|held_pc| held_pc == pc) {
                let payload = raw[serial(&raw)].to_vec();
                assert_eq!((payload[2], payload[3], payload[4]), (0, 9, 9));
                if index == 0 {
                    held = Some(payload.clone());
                }
                assert_eq!(Some(&payload), held.as_ref());
                if index == 0 {
                    let mut bad = raw.clone();
                    bad[serial(&raw).start + 7] ^= 1;
                    let before = machine.clone();
                    assert!(machine.decode_state(&bad).is_err());
                    assert_eq!(machine, before);
                }
                assert_eq!(
                    machine.peek(0x200001),
                    Some(if index == 1 { 0x6b } else { 0 })
                );
            }
            if *pc == p.busy_nack {
                let payload = &raw[serial(&raw)];
                assert_eq!(u16::from_le_bytes(payload[6..8].try_into().unwrap()), 0x5f0);
            }
            for frontend in [false, true] {
                let mut copy = Emulator::new(p.bytes.clone(), config).unwrap();
                if frontend {
                    copy.decode_frontend_state(&front).unwrap();
                } else {
                    copy.decode_state(&raw).unwrap();
                }
                assert_eq!(copy.encode_state().unwrap(), raw);
                equal_future(copy, machine.clone(), false);
            }
        }
        advance(&mut machine, None);
        assert_eq!(received(&machine, p.value), 0x96);
        assert_eq!(sample(&machine, p.busy_ack), 1);
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn active_audio_frontend_future_and_explicitly_aligned_raw_restore() {
    let p = program(true);
    for config in configs() {
        let mut machine = Emulator::new(p.bytes.clone(), config).unwrap();
        machine.set_sample_rate(44_100);
        machine.load_save_ram(&cells()).unwrap();
        for pc in &p.cuts {
            advance(&mut machine, Some(*pc));
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            let queued = audio(&mut machine.clone());
            assert!(queued.iter().any(|value| value & 0x7fff_ffff != 0));
            let mut copy = Emulator::new(p.bytes.clone(), config).unwrap();
            copy.set_sample_rate(44_100);
            copy.decode_frontend_state(&front).unwrap();
            assert_eq!(copy.encode_frontend_state().unwrap(), front);
            assert_eq!(audio(&mut copy.clone()), queued);
            equal_future(copy, machine.clone(), true);
            let mut raw_copy = Emulator::new(p.bytes.clone(), config).unwrap();
            raw_copy.set_sample_rate(44_100);
            raw_copy.decode_state(&raw).unwrap();
            let mut aligned = machine.clone();
            // Raw states omit host audio; restart both output paths before comparing their future.
            aligned.set_sample_rate(44_100);
            raw_copy.set_sample_rate(44_100);
            equal_future(raw_copy, aligned, true);
            if *pc == p.busy {
                let before = machine.clone();
                assert!(
                    machine
                        .decode_frontend_state(&front[..front.len() - 1])
                        .is_err()
                );
                assert_eq!(machine, before);
                let mut wrong = Emulator::new(
                    p.bytes.clone(),
                    LoadConfig {
                        concurrent_sound: !config.concurrent_sound,
                        ..config
                    },
                )
                .unwrap();
                let untouched = wrong.clone();
                assert!(wrong.decode_frontend_state(&front).is_err());
                assert_eq!(wrong, untouched);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_cpu_and_frontend_reset_keep_profile_and_rebase_busy() {
    let p = program(false);
    for config in configs() {
        let mut machine = Emulator::new(p.bytes.clone(), config).unwrap();
        machine.load_save_ram(&cells()).unwrap();
        for pc in [p.held[0], p.dirty, p.busy] {
            advance(&mut machine, Some(pc));
            let raw = machine.encode_state().unwrap();
            let payload = &raw[serial(&raw)];
            let until = u64::from_le_bytes(payload[30..38].try_into().unwrap());
            if pc == p.dirty {
                assert_eq!(
                    u16::from_le_bytes(payload[28..30].try_into().unwrap()),
                    0xe000
                );
            }
            let mut copy = machine.clone();
            copy.reset_cpu().unwrap();
            let cpu = copy.encode_state().unwrap();
            assert_eq!(&cpu[serial(&cpu)], payload);
            copy = machine.clone();
            copy.reset();
            let full = copy.encode_state().unwrap();
            let after = &full[serial(&full)];
            assert_eq!(&after[..30], &payload[..30]);
            assert_eq!(
                u64::from_le_bytes(after[30..38].try_into().unwrap()),
                until.saturating_sub(machine.timing_snapshot().now().get())
            );
            assert_eq!(copy.dump_save_ram(), machine.dump_save_ram());
            assert_eq!(copy.peek(0x200001), Some(0x6b));
        }
    }
}

fn sample_at(g: &mut Guest, address: u32) -> usize {
    let index = g.rom.results;
    g.rom.word(0x1039);
    g.rom.long(address);
    g.rom.word(0x13c0);
    g.rom.long(0xff2000 + index as u32);
    g.rom.results += 1;
    index
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_alias_lanes_and_rom_window_boundaries() {
    let mut g = guest();
    g.rom.bytes[0x1fffff] = 0x42;
    g.rom.bytes[0x2fffff] = 0x6b;
    g.rom.bytes[0x300001] = 0x31;
    g.rom.write(0x33fc, 0x2ffffe, 0xa400);
    let released = sample_at(&mut g, 0x2fffff);
    g.rom.write(0x13fc, 0x2ffffe, 0xa4);
    g.rom.write(0x13fc, 0x2fffff, 0xa4);
    let low = sample_at(&mut g, 0x200001);
    g.rom.write(0x13fc, 0x2ffffe, 0xa5);
    g.rom.write(0x13fc, 0x2fffff, 0xa5);
    let high = sample_at(&mut g, 0x200001);
    let below = sample_at(&mut g, 0x1fffff);
    let above = sample_at(&mut g, 0x300001);
    g.rom.write(0x33fc, 0x2ffffe, 0xa401);
    let alias_rom = sample_at(&mut g, 0x2fffff);
    let base_rom = sample_at(&mut g, 0x200001);
    let (bytes, _) = g.rom.finish();
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        advance(&mut machine, None);
        for (index, expected) in [
            (released, 1),
            (low, 0),
            (high, 1),
            (below, 0x42),
            (above, 0x31),
            (alias_rom, 0x6b),
            (base_rom, 0x6b),
        ] {
            assert_eq!(sample(&machine, index), expected);
        }
        assert_eq!(machine.dump_save_ram().unwrap(), vec![0xff; 2048]);
        assert_eq!(machine.rom_bytes(), bytes);
    }
}
