#[path = "eeprom/fixture.rs"]
mod fixture;
#[path = "split_serial/held_ack.rs"]
mod held_ack;
#[path = "split_serial/guest.rs"]
mod legacy_guest;
#[path = "support/state_layout.rs"]
mod state_layout;

use fixture::{advance, sample};
use legacy_guest::{Guest, received};
use zeff_md_core::{Emulator, LoadConfig, Region};

fn configs() -> impl Iterator<Item = LoadConfig> {
    [Region::NtscUs, Region::NtscJapan, Region::Pal]
        .into_iter()
        .flat_map(|region| {
            [false, true].map(move |concurrent_sound| LoadConfig {
                region,
                concurrent_sound,
            })
        })
}

fn guest() -> Guest {
    let mut g = Guest::new();
    g.rom.bytes.resize(0x400000, 0);
    g.rom.bytes[0x180..0x18e].copy_from_slice(b"GM T-081586-00");
    g
}

fn cells() -> Vec<u8> {
    (0..2048_u16)
        .map(|n| (n as u8).wrapping_mul(73) ^ ((n >> 8) as u8).wrapping_mul(29) ^ 0x59)
        .collect()
}

fn address(g: &mut Guest, address: u16) -> (usize, usize) {
    g.start();
    let control = g.send(0xa0 | ((address >> 7) as u8 & 0x0e));
    let low = g.send(address as u8);
    (control, low)
}

fn read(g: &mut Guest, target: u16) {
    address(g, target);
    g.start();
    g.send(0xa1 | ((target >> 7) as u8 & 0x0e));
}

fn serial(raw: &[u8]) -> std::ops::Range<usize> {
    state_layout::serial_payload_range(raw, 2048, 38)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_all_cells_carry_and_control_selection() {
    let mut g = guest();
    g.mapping(false);
    read(&mut g, 0);
    let all: Vec<_> = (0..2048).map(|n| g.receive(n != 2047, false)).collect();
    g.stop();
    let mut banks = Vec::new();
    for bank in 0..8 {
        let target = bank * 256 + 0x35;
        read(&mut g, target);
        banks.push((target, g.receive(false, false)));
        g.stop();
    }
    let mut carries = Vec::new();
    for target in [0xff, 0x1ff, 0x3ff, 0x7ff] {
        read(&mut g, target);
        carries.push((target, g.receive(true, false), g.receive(false, false)));
        g.stop();
    }
    address(&mut g, 0x123);
    g.start();
    let selected = g.send(0xa7);
    let changed_bank = g.receive(false, false);
    g.stop();
    g.start();
    let invalid = g.send(0xb0);
    g.send(0);
    g.send(0x12);
    g.stop();
    let (bytes, _) = g.rom.finish();
    let initial = cells();
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.load_save_ram(&initial).unwrap();
        advance(&mut machine, None);
        assert_eq!(machine.dump_save_ram().unwrap(), initial);
        for (index, first) in all.iter().enumerate() {
            assert_eq!(received(&machine, *first), initial[index]);
        }
        for (target, first) in &banks {
            assert_eq!(received(&machine, *first), initial[usize::from(*target)]);
        }
        for (target, first, next) in &carries {
            assert_eq!(received(&machine, *first), initial[usize::from(*target)]);
            assert_eq!(
                received(&machine, *next),
                initial[usize::from((*target + 1) & 0x7ff)]
            );
        }
        assert_eq!(sample(&machine, selected), 0);
        assert_eq!(received(&machine, changed_bank), initial[0x323]);
        assert_eq!(sample(&machine, invalid), 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_page_wrap_busy_and_every_bank_write() {
    let mut g = guest();
    g.mapping(false);
    let mut checkpoints = Vec::new();
    let mut expected = cells();
    let mut acks = Vec::new();
    let mut busy = Vec::new();
    for bank in 0..8_u16 {
        let target = bank * 256 + 0xfd;
        let (control, low) = address(&mut g, target);
        acks.extend([control, low]);
        for n in 0..19_u16 {
            let value = (bank as u8).wrapping_mul(31).wrapping_add(n as u8);
            acks.push(g.send(value));
            let cell = (target & !15) | ((target + n) & 15);
            expected[usize::from(cell)] = value;
        }
        let before = g.mark();
        g.stop();
        let committed = g.mark();
        g.start();
        busy.push(g.send(0xa0 | ((target >> 7) as u8 & 14)));
        g.stop();
        g.wait();
        let ready = g.mark();
        checkpoints.push((before, committed, ready, expected.clone()));
    }
    let (bytes, _) = g.rom.finish();
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        let mut previous = cells();
        machine.load_save_ram(&previous).unwrap();
        for (before, committed, ready, next) in &checkpoints {
            advance(&mut machine, Some(*before));
            assert_eq!(machine.dump_save_ram().unwrap(), previous);
            advance(&mut machine, Some(*committed));
            assert_eq!(machine.dump_save_ram().unwrap(), *next);
            advance(&mut machine, Some(*ready));
            previous = next.clone();
        }
        advance(&mut machine, None);
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
        for first in &acks {
            assert_eq!(sample(&machine, *first), 0);
        }
        for first in &busy {
            assert_eq!(sample(&machine, *first), 1);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_held_ack_codec_future_and_exact_profile() {
    let mut g = guest();
    g.mapping(false);
    g.start();
    for bit in (2..8).rev() {
        let value = (0xae >> bit) & 1;
        g.line(value);
        g.line(value | 2);
        g.line(value);
    }
    g.mark();
    g.start();
    g.send_held(0xae);
    g.send(0xff);
    g.send(0x96);
    g.mark();
    g.stop();
    g.mark();
    g.wait();
    read(&mut g, 0x7ff);
    let value = g.receive(false, true);
    g.stop();
    let cuts = g.cuts.clone();
    let (bytes, _) = g.rom.finish();
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.load_save_ram(&cells()).unwrap();
        for pc in &cuts {
            advance(&mut machine, Some(*pc));
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            assert_eq!(state_layout::sections(&raw).version, 1);
            let range = serial(&raw);
            let mut control = machine.clone();
            advance(&mut control, None);
            let future = control.encode_state().unwrap();
            for frontend in [false, true] {
                let mut copy = Emulator::new(bytes.clone(), config).unwrap();
                if frontend {
                    copy.decode_frontend_state(&front).unwrap();
                    assert_eq!(copy.encode_frontend_state().unwrap(), front);
                } else {
                    copy.decode_state(&raw).unwrap();
                    assert_eq!(copy.encode_state().unwrap(), raw);
                }
                advance(&mut copy, None);
                assert_eq!(copy.encode_state().unwrap(), future);
                assert_eq!(copy.framebuffer(), control.framebuffer());
                assert_eq!(copy.dump_save_ram(), control.dump_save_ram());
            }
            for (offset, value) in [
                (0, 2),
                (3, 255),
                (4, 255),
                (7, 8),
                (26, 1),
                (27, 8),
                (37, 255),
            ] {
                let mut bad = raw.clone();
                bad[range.start + offset] = value;
                assert!(machine.decode_state(&bad).is_err());
                assert_eq!(machine.encode_state().unwrap(), raw);
            }
            for length in [raw.len() - 1, range.start + 7, range.start + 29] {
                assert!(machine.decode_state(&raw[..length]).is_err());
                assert_eq!(machine.encode_state().unwrap(), raw);
            }
            for legacy_len in [23, 27] {
                let save = state_layout::save_range(&raw);
                let mut bad = raw.clone();
                bad.drain(range.start + legacy_len..range.end);
                let length = (save.len() - (38 - legacy_len)) as u32;
                bad[save.start - 4..save.start].copy_from_slice(&length.to_le_bytes());
                assert!(machine.decode_state(&bad).is_err());
                assert_eq!(machine.encode_state().unwrap(), raw);
            }
        }
        advance(&mut machine, None);
        assert_eq!(received(&machine, value), 0x96);
        let raw = machine.encode_state().unwrap();
        for size in [0, 128, 256, 2047, 2049, 8192] {
            assert!(machine.load_save_ram(&vec![0; size]).is_err());
            assert_eq!(machine.encode_state().unwrap(), raw);
        }
        let mut wrong_rom = bytes.clone();
        wrong_rom[0x3fffff] ^= 1;
        let mut wrong = Emulator::new(wrong_rom, config).unwrap();
        let before = wrong.encode_state().unwrap();
        assert!(wrong.decode_state(&raw).is_err());
        assert_eq!(wrong.encode_state().unwrap(), before);
        assert_eq!(machine.rom_bytes(), bytes);
    }
    for size in [0x200000, 0x300000, 0x3fffff] {
        let mut wrong = bytes.clone();
        wrong.resize(size, 0);
        assert!(Emulator::new(wrong, LoadConfig::default()).is_err());
    }
    for offset in (0x180..0x18e).chain(0x1b2..0x1bc) {
        let mut wrong = bytes.clone();
        wrong[offset] ^= 1;
        assert!(Emulator::new(wrong, LoadConfig::default()).is_err());
    }
}

#[path = "split_serial16/continuations.rs"]
mod continuations;
