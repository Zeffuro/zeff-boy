use super::*;

struct Program {
    bytes: Vec<u8>,
    cuts: Vec<u32>,
    held: Vec<(Vec<u32>, u8, bool)>,
    dirty: u32,
    busy: u32,
    values: Vec<usize>,
    expected: Vec<u8>,
}

fn send_cut(g: &mut Guest, byte: u8, phase: u8, ack: bool, held: &mut Vec<(Vec<u32>, u8, bool)>) {
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
    let mut cuts = vec![g.mark()];
    g.mapping(true);
    cuts.push(g.mark());
    g.mapping(false);
    cuts.push(g.mark());
    if phase == 4 {
        g.line(2);
        g.line(3);
        cuts.push(g.mark());
    }
    held.push((cuts, phase, ack));
    g.line(1);
}

fn program(college: bool) -> Program {
    let mut g = guest(college);
    for value in [0x84, 1, 0x90] {
        g.rom.write(0x13fc, 0xc00011, value);
    }
    g.wait();
    g.mapping(false);
    address(&mut g, 0x1ffd);
    g.start();
    g.send(0xa0);
    let mut held = Vec::new();
    send_cut(&mut g, 0x20, 13, false, &mut held);
    g.stop();
    g.start();
    send_cut(&mut g, 0xa0, 9, true, &mut held);
    send_cut(&mut g, 0x1f, 13, true, &mut held);
    send_cut(&mut g, 0xfd, 11, true, &mut held);
    g.send(0x96);
    g.send(0x3a);
    send_cut(&mut g, 0x57, 4, true, &mut held);
    for n in 3..63 {
        g.send(0xa0 + n);
    }
    send_cut(&mut g, 0xdf, 4, true, &mut held);
    let dirty = g.mark();
    g.stop();
    let busy = g.mark();
    wait(&mut g);
    read(&mut g, 0x1ffd);
    let values = (0..4).map(|n| g.receive(n != 3, n == 1)).collect();
    g.stop();
    let cuts = g.cuts.clone();
    let (bytes, _) = g.rom.finish();
    let mut expected = cells();
    for n in 0..64_u16 {
        let value = match n {
            0 => 0x96,
            1 => 0x3a,
            2 => 0x57,
            _ => 0xa0 + n as u8,
        };
        expected[usize::from((0x1ff8 + ((5 + n) & 63)) & 0x1fff)] = value;
    }
    Program {
        bytes,
        cuts,
        held,
        dirty,
        busy,
        values,
        expected,
    }
}

fn audio(machine: &mut Emulator) -> Vec<u32> {
    let mut pcm = Vec::new();
    machine.drain_audio_samples_into(&mut pcm);
    pcm.into_iter().map(f32::to_bits).collect()
}

pub(super) fn assert_futures(machine: &Emulator, bytes: &[u8], config: LoadConfig, pcm: bool) {
    let raw = machine.encode_state().unwrap();
    let front = machine.encode_frontend_state().unwrap();
    for frontend in [false, true] {
        let mut copy = Emulator::new(bytes.to_vec(), config).unwrap();
        if pcm {
            copy.set_sample_rate(44_100);
        }
        let mut control = machine.clone();
        if frontend {
            copy.decode_frontend_state(&front).unwrap();
            assert_eq!(copy.encode_frontend_state().unwrap(), front);
        } else {
            copy.decode_state(&raw).unwrap();
            // Raw states omit host output; align both restart paths before comparing PCM.
            if pcm {
                control.set_sample_rate(44_100);
                copy.set_sample_rate(44_100);
            }
        }
        assert_eq!(copy.encode_state().unwrap(), raw);
        advance(&mut copy, None);
        advance(&mut control, None);
        if pcm {
            for _ in 0..4096 {
                assert_eq!(
                    copy.step_instruction().unwrap(),
                    control.step_instruction().unwrap()
                );
            }
            let actual = audio(&mut copy);
            assert!(!actual.is_empty() && actual.iter().any(|n| n & 0x7fff_ffff != 0));
            assert_eq!(actual, audio(&mut control));
        }
        assert_eq!(
            copy.encode_state().unwrap(),
            control.encode_state().unwrap()
        );
        assert_eq!(copy.framebuffer(), control.framebuffer());
        assert_eq!(copy.dump_save_ram(), control.dump_save_ram());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_partial_high_low_held_ack_and_active_future_restore() {
    for college in [false, true] {
        let p = program(college);
        for config in configs() {
            let mut machine = Emulator::new(p.bytes.clone(), config).unwrap();
            machine.set_sample_rate(44_100);
            let initial = cells();
            machine.load_save_ram(&initial).unwrap();
            for cut in &p.cuts {
                advance(&mut machine, Some(*cut));
                let raw = machine.encode_state().unwrap();
                assert_eq!(state_layout::sections(&raw).version, 1);
                for (cuts, phase, ack) in &p.held {
                    if let Some(index) = cuts.iter().position(|pc| pc == cut) {
                        let payload = &raw[serial(&raw)];
                        assert_eq!(
                            (payload[3], payload[4], payload[9]),
                            (*phase, 9, u8::from(*ack))
                        );
                        assert_eq!(payload[2], u8::from(!*ack));
                        if *phase != 4 {
                            assert_eq!(word(payload, 6), 0x1ffd);
                        }
                        assert_eq!(machine.dump_save_ram().unwrap(), initial);
                        assert_eq!(
                            machine.peek(0x200001),
                            Some(if index == 1 { 0x6b } else { u8::from(!*ack) })
                        );
                        let mut beginning = Emulator::new(p.bytes.clone(), config).unwrap();
                        beginning.set_sample_rate(44_100);
                        beginning.load_save_ram(&initial).unwrap();
                        advance(&mut beginning, Some(cuts[0]));
                        let first = beginning.encode_state().unwrap();
                        assert_eq!(payload, &first[serial(&first)]);
                    }
                }
                if *cut == p.dirty {
                    let payload = &raw[serial(&raw)];
                    assert_eq!(
                        u64::from_le_bytes(payload[76..84].try_into().unwrap()),
                        u64::MAX
                    );
                    assert_eq!(word(payload, 6), 0x1ffd);
                    assert_eq!(word(payload, 74), 0x1ff8);
                }
                assert_futures(&machine, &p.bytes, config, true);
            }
            advance(&mut machine, None);
            for (first, value) in p.values.iter().zip([0x96, 0x3a, 0x57, 0xa3]) {
                assert_eq!(received(&machine, *first), value);
            }
            assert_eq!(machine.dump_save_ram().unwrap(), p.expected);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_malformed_codec_is_transactional_and_reset_rebases_busy() {
    for college in [false, true] {
        let p = program(college);
        for config in configs() {
            let mut machine = Emulator::new(p.bytes.clone(), config).unwrap();
            machine.set_sample_rate(44_100);
            machine.load_save_ram(&cells()).unwrap();
            for cut in [p.held[2].0[0], p.dirty, p.busy] {
                advance(&mut machine, Some(cut));
                let raw = machine.encode_state().unwrap();
                let range = serial(&raw);
                for (offset, value) in [
                    (0, 2),
                    (3, 14),
                    (4, 255),
                    (7, 0x20),
                    (74, 1),
                    (75, 0x20),
                    (91, 255),
                ] {
                    let mut bad = raw.clone();
                    bad[range.start + offset] = value;
                    let before = machine.clone();
                    assert!(machine.decode_state(&bad).is_err());
                    assert_eq!(machine, before);
                }
                if cut == p.dirty {
                    let mut bad = raw.clone();
                    let base = word(&raw[range.clone()], 74);
                    bad[range.start + 6..range.start + 8]
                        .copy_from_slice(&((base + 64) & 0x1fff).to_le_bytes());
                    let before = machine.clone();
                    assert!(machine.decode_state(&bad).is_err());
                    assert_eq!(machine, before);
                } else if cut == p.held[2].0[0] {
                    let mut bad = raw.clone();
                    bad[range.start + 7] ^= 1;
                    let before = machine.clone();
                    assert!(machine.decode_state(&bad).is_err());
                    assert_eq!(machine, before);
                }
                for length in [raw.len() - 1, range.start + 7, range.start + 83] {
                    let before = machine.clone();
                    assert!(machine.decode_state(&raw[..length]).is_err());
                    assert_eq!(machine, before);
                }
                for legacy in [23, 27, 38] {
                    let save = state_layout::save_range(&raw);
                    let mut bad = raw.clone();
                    bad.drain(range.start + legacy..range.end);
                    bad[save.start - 4..save.start]
                        .copy_from_slice(&((save.len() - (92 - legacy)) as u32).to_le_bytes());
                    let before = machine.clone();
                    assert!(machine.decode_state(&bad).is_err());
                    assert_eq!(machine, before);
                }
                let front = machine.encode_frontend_state().unwrap();
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
                let mut wrong_rom = p.bytes.clone();
                wrong_rom[0x3fffff] ^= 1;
                let mut foreign = Emulator::new(wrong_rom, config).unwrap();
                let untouched = foreign.clone();
                assert!(foreign.decode_state(&raw).is_err());
                assert_eq!(foreign, untouched);
                let payload = &raw[range];
                let mut cpu_reset = machine.clone();
                cpu_reset.reset_cpu().unwrap();
                let after = cpu_reset.encode_state().unwrap();
                assert_eq!(&after[serial(&after)], payload);
                let mut reset = machine.clone();
                reset.reset();
                let after = reset.encode_state().unwrap();
                let state = &after[serial(&after)];
                assert_eq!(&state[..84], &payload[..84]);
                let until = u64::from_le_bytes(payload[84..92].try_into().unwrap());
                assert_eq!(
                    u64::from_le_bytes(state[84..92].try_into().unwrap()),
                    until.saturating_sub(machine.timing_snapshot().now().get())
                );
                assert_eq!(reset.dump_save_ram(), machine.dump_save_ram());
                assert_eq!(reset.peek(0x200001), Some(0x6b));
            }
        }
    }
}
