use super::*;
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Default)]
struct Witness {
    hash: Sha256,
    bytes: u64,
    cuts: u64,
    futures: u64,
    observations: u64,
    nonzero: u64,
    flags: u8,
    held_csm: u64,
    pending_keys: u64,
}

impl Witness {
    fn record(&mut self, tag: u8, bytes: &[u8]) {
        self.hash.update([tag]);
        self.hash.update((bytes.len() as u64).to_le_bytes());
        self.hash.update(bytes);
        self.bytes += 9 + bytes.len() as u64;
    }

    fn numbers(&mut self, tag: u8, values: &[u64]) {
        let bytes: Vec<_> = values.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.record(tag, &bytes);
    }

    fn state(&mut self, chip: &Opn2) {
        self.record(2, &encode(chip));
    }

    fn observe(&mut self, chip: &Opn2, elapsed: u64) {
        let mut bytes = elapsed.to_le_bytes().to_vec();
        for port in 0..4 {
            bytes.push(chip.status(port));
        }
        for mutes in [[false; 6], [false, true, false, true, false, true]] {
            for sample in chip.sample_with_mutes(mutes) {
                bytes.extend(sample.to_le_bytes());
                self.nonzero += u64::from(sample != 0);
            }
        }
        bytes.push(u8::from(chip.timer_a.csm()));
        bytes.push(u8::from(chip.scheduler.pending.is_some()));
        self.flags |= chip.status(0) & 3;
        self.held_csm += u64::from(chip.timer_a.csm());
        self.pending_keys += u64::from(chip.scheduler.pending.is_some());
        self.record(3, &bytes);
        self.observations += 1;
    }
}

fn encode(chip: &Opn2) -> Vec<u8> {
    let mut writer = StateWriter::new();
    chip.write_state(&mut writer);
    writer.into_bytes()
}

fn restore(chip: &Opn2) -> Opn2 {
    let bytes = encode(chip);
    let mut reader = StateReader::new(&bytes);
    let restored = Opn2::read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(*chip, restored);
    assert_eq!(encode(&restored), bytes);
    restored
}

fn write(chip: &mut Opn2, register: u16, value: u8) {
    let bank = ((register >> 8) as u8) * 2;
    chip.write_port(bank, register as u8);
    chip.write_port(bank + 1, value);
}

fn reload(chip: &mut Opn2, value: u16) {
    write(chip, 0x24, (value >> 2) as u8);
    write(chip, 0x25, (value & 3) as u8);
}

fn tone(chip: &mut Opn2) {
    for channel in 0..6u16 {
        let address = (channel / 3) * 0x100 + channel % 3;
        for slot in [0, 4, 8, 12] {
            for (group, value) in [(0x30, 1), (0x40, 0), (0x50, 31), (0x80, 15)] {
                write(chip, address + slot + group, value);
            }
            write(chip, address + slot + 0x60, 0x80);
        }
        write(chip, address + 0xb0, 7);
        write(chip, address + 0xb4, 0xf7);
        write(chip, address + 0xa4, 0x22);
        write(chip, address + 0xa0, 0x69 + channel as u8);
        write(
            chip,
            0x28,
            0xf0 | (channel as u8 % 3) | ((channel / 3) as u8 * 4),
        );
        chip.advance(144);
    }
    for (address, value) in [
        (0xad, 0x19),
        (0xa9, 0x45),
        (0xae, 0x2b),
        (0xaa, 0x78),
        (0xac, 0x32),
        (0xa8, 0xab),
        (0x22, 15),
    ] {
        write(chip, address, value);
    }
    chip.advance(144 * 8);
}

fn continuation(w: &mut Witness, start: Opn2, clocks: u64) {
    let mut whole = start;
    let predicted = start.status_after(clocks);
    assert_eq!(start, restore(&start));
    whole.advance(clocks);
    assert_eq!(predicted, whole.status(0));
    let mut partitioned = restore(&start);
    let mut elapsed = 0;
    w.numbers(4, &[clocks]);
    w.observe(&partitioned, 0);
    while elapsed < clocks {
        let hop = if clocks <= 307 { 1 } else { 6 }.min(clocks - elapsed);
        let before = partitioned;
        let status = partitioned.status_after(hop);
        assert_eq!(before, partitioned);
        partitioned.advance(hop);
        assert_eq!(partitioned.status(0), status);
        elapsed += hop;
        w.observe(&partitioned, elapsed);
    }
    assert_eq!(whole, partitioned);
    let mut scalar = start;
    for _ in 0..clocks {
        scalar.advance(1);
    }
    assert_eq!(whole, scalar);
    assert_eq!(whole, restore(&whole));
    w.state(&whole);
    w.futures += 1;
}

fn cuts(w: &mut Witness, seed: Opn2, id: u64, mutation: u8) {
    let before_nonzero = w.nonzero;
    for slot in 0..24u64 {
        for fraction in 0..6u64 {
            let mut chip = seed;
            chip.advance(slot * 6 + fraction);
            let actual = u64::from(chip.scheduler.step % 24);
            assert_eq!(actual, slot);
            assert_eq!(chip.phase % 6, fraction);
            let mode = chip.mode;
            match mutation {
                1 => write(&mut chip, 0x27, mode | 0x30),
                2 => reload(&mut chip, 511),
                3 => write(&mut chip, 0x28, 2),
                4 => write(&mut chip, 0x28, 0xf2),
                5 => write(&mut chip, 0x27, mode & !0xc0),
                6 => write(&mut chip, 0x27, mode & !3),
                _ => {}
            }
            write(&mut chip, 0xb6, if slot & 1 == 0 { 0xb7 } else { 0x77 });
            w.numbers(1, &[id, slot, fraction, actual, mutation as u64]);
            w.state(&chip);
            w.observe(&chip, 0);
            assert_eq!(chip, restore(&chip));
            for future in [7, 307, 4609] {
                continuation(w, chip, future);
            }
            w.cuts += 1;
        }
    }
    if id >= 18 {
        assert!(w.nonzero > before_nonzero, "active control {id}");
    }
}

fn reject_invalid_timers(w: &mut Witness) {
    for (counter, bits) in [(1024u16, 0u8), (65535, 0), (1, 4), (0, 32), (0, 128)] {
        let mut writer = StateWriter::new();
        writer.write_u16(1023);
        writer.write_u16(counter);
        writer.write_u8(bits);
        let bytes = writer.into_bytes();
        assert!(TimerA::read_state(&mut StateReader::new(&bytes)).is_err());
        w.record(5, &bytes);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn whole_opn2_timer_a_state_pcm_status_future_witness() {
    let mut w = Witness::default();
    w.record(0, b"whole-opn2-timera-v1/raw20/le/tag-u8-length-u64");
    reject_invalid_timers(&mut w);
    let mut id = 0;
    for settled in [false, true] {
        let mut seed = Opn2::default();
        if settled {
            seed.advance(144 * 12);
        }
        cuts(&mut w, seed, id, 0);
        id += 1;
    }
    for active in [false, true] {
        for value in [0u16, 511, 1022, 1023] {
            for enabled in [false, true] {
                for timer_b in [false, true] {
                    let mut seed = Opn2::default();
                    if active {
                        tone(&mut seed);
                    }
                    reload(&mut seed, value);
                    write(&mut seed, 0x26, 255);
                    write(
                        &mut seed,
                        0x27,
                        1 | (u8::from(enabled) * 4) | (u8::from(timer_b) * 10),
                    );
                    seed.advance(u64::from(1023 - value) * 144);
                    cuts(&mut w, seed, id, 0);
                    id += 1;
                }
            }
        }
    }
    for period in [1u16, 2] {
        for manual in [0, 0xf0] {
            for enabled in [false, true] {
                let mut seed = Opn2::default();
                tone(&mut seed);
                reload(&mut seed, 1024 - period);
                write(&mut seed, 0x26, 255);
                write(&mut seed, 0x28, manual | 2);
                write(&mut seed, 0x27, 0x8b | (u8::from(enabled) * 4));
                seed.advance(144 * 8);
                for mutation in 0..=6 {
                    cuts(&mut w, seed, id, mutation);
                    id += 1;
                }
            }
        }
    }
    assert_eq!(id, 90);
    assert_eq!(w.cuts, id * 24 * 6);
    assert_eq!(w.futures, w.cuts * 3);
    assert_eq!(w.flags, 3);
    assert!(w.held_csm > 0 && w.pending_keys > 0 && w.nonzero > 0);
    let digest: String = w
        .hash
        .finalize()
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect();
    let line = format!(
        "WHOLE_OPN2_WITNESS {{\"schema\":1,\"scenarios\":{id},\"slots\":24,\"fractions\":6,\"cuts\":{},\"futures\":{},\"observations\":{},\"nonzeroIntegerSamples\":{},\"flagsSeen\":{},\"heldCsm\":{},\"pendingKeys\":{},\"hashedBytes\":{},\"sha256\":\"{digest}\"}}",
        w.cuts, w.futures, w.observations, w.nonzero, w.flags, w.held_csm, w.pending_keys, w.bytes
    );
    #[cfg(not(target_arch = "wasm32"))]
    println!("{line}");
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{}", line);
}
