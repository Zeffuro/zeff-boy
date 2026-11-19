#[path = "split_serial/byte_address.rs"]
mod byte_address;
#[path = "eeprom/fixture.rs"]
mod fixture;
#[path = "split_serial/guest.rs"]
mod guest;
#[path = "split_serial/held_ack.rs"]
mod held_ack;
#[path = "support/state_layout.rs"]
mod state_layout;

use byte_address::{cells, serial};
use fixture::{advance, sample};
use guest::{Guest, received};
use zeff_emu_common::time::{MachineTiming, Reset};
use zeff_md_core::{Emulator, LoadConfig, LoadError, Region};

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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_serial_gpio_protocol_page_and_aliases() {
    let mut g = Guest::new();
    let rom_byte = g.rom.sample();
    g.mapping(false);
    let released = g.rom.sample();
    let upper_prefetch = g.sample_upper();
    g.line(2);
    let low = g.rom.sample();
    g.line(3);
    let high = g.rom.sample();
    g.address(3);
    let mut writes = Vec::new();
    for value in [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88] {
        writes.push(g.send(value));
    }
    let before_stop = g.mark();
    g.stop();
    let committed = g.mark();
    g.start();
    let busy = g.send(0xa0);
    g.stop();
    g.wait();
    g.start();
    let ready = g.send(0xa0);
    g.send(0xff);
    g.start();
    let repeated = g.send(0xa1);
    let ff = g.receive(true, false);
    let zero = g.receive(false, false);
    let after_nack = g.receive(false, false);
    g.stop();
    let mut aliases = Vec::new();
    let mut values = Vec::new();
    for control in 0xa0..=0xaf {
        if control & 1 == 0 {
            g.start();
            aliases.push(g.send(control));
            aliases.push(g.send(0x80));
            g.stop();
        } else {
            g.address(0x80);
            g.start();
            aliases.push(g.send(control));
            values.push(g.receive(false, false));
            g.stop();
        }
    }
    g.start();
    let invalid = g.send(0xb0);
    g.send(0x80);
    g.send(0x12);
    g.stop();
    g.mapping(true);
    let final_rom = g.rom.sample();
    let (bytes, _) = g.rom.finish();
    let initial = cells();
    let mut expected = initial.clone();
    expected[..8].copy_from_slice(&[0x66, 0x77, 0x88, 0x11, 0x22, 0x33, 0x44, 0x55]);
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.load_save_ram(&initial).unwrap();
        advance(&mut machine, Some(before_stop));
        assert_eq!(machine.dump_save_ram().unwrap(), initial);
        advance(&mut machine, Some(committed));
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
        advance(&mut machine, None);
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
        for (index, value) in [
            (rom_byte, 0x6b),
            (released, 1),
            (upper_prefetch, 0x13),
            (low, 0),
            (high, 1),
            (busy, 1),
            (ready, 0),
            (repeated, 0),
            (invalid, 1),
            (final_rom, 0x6b),
        ] {
            assert_eq!(sample(&machine, index), value);
        }
        for index in writes.iter().chain(&aliases) {
            assert_eq!(sample(&machine, *index), 0);
        }
        assert_eq!(received(&machine, ff), initial[255]);
        assert_eq!(received(&machine, zero), expected[0]);
        assert_eq!(received(&machine, after_nack), 0xff);
        for index in &values {
            assert_eq!(received(&machine, *index), initial[128]);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_serial_checkpoints_mapping_reset_and_transactional_codec() {
    let mut g = Guest::new();
    g.mapping(false);
    g.start();
    let ack = g.send_held(0xa0);
    g.send(3);
    g.send(0x96);
    let dirty = g.mark();
    g.mapping(true);
    g.mark();
    g.mapping(false);
    g.mark();
    g.stop();
    let busy = g.mark();
    g.wait();
    g.read(3);
    let value = g.receive(false, true);
    g.stop();
    let cuts = g.cuts.clone();
    let (bytes, _) = g.rom.finish();
    for config in configs() {
        let mut machine = Emulator::new(bytes.clone(), config).unwrap();
        machine.load_save_ram(&cells()).unwrap();
        let mut futures = Vec::new();
        let mut held = None;
        for (index, pc) in cuts.iter().enumerate() {
            advance(&mut machine, Some(*pc));
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            let range = serial(&raw);
            if index == 0 {
                held = Some(raw[range.clone()].to_vec());
            }
            if index < 3 {
                assert_eq!(raw[range.clone()], held.as_ref().unwrap()[..]);
            }
            if index < 3 {
                assert_eq!(
                    machine.peek(guest::PORT + 1),
                    Some(if index == 1 { 0x6b } else { 0 })
                );
            }
            if *pc == dirty {
                assert_eq!(machine.dump_save_ram().unwrap(), cells());
            }
            let mut cloned = machine.clone();
            advance(&mut cloned, None);
            futures.push((
                cloned.encode_state().unwrap(),
                cloned.dump_save_ram().unwrap(),
            ));
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
                futures.push((copy.encode_state().unwrap(), copy.dump_save_ram().unwrap()));
            }
            let mut reset = machine.clone();
            reset.reset_cpu().unwrap();
            let cpu_reset = reset.encode_state().unwrap();
            assert_eq!(&cpu_reset[serial(&cpu_reset)], &raw[range.clone()]);
            let now = machine.timing_snapshot().now().get();
            let until = u64::from_le_bytes(raw[range.start + 19..range.end].try_into().unwrap());
            reset = machine.clone();
            reset.reset();
            let after = reset.encode_state().unwrap();
            let reset_range = serial(&after);
            assert_eq!(reset.peek(guest::PORT + 1), Some(0x6b));
            assert_eq!(
                &after[reset_range.start..reset_range.start + 19],
                &raw[range.start..range.start + 19]
            );
            let remaining = u64::from_le_bytes(
                after[reset_range.start + 19..reset_range.end]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(remaining, until.saturating_sub(now));
            assert_eq!(
                reset.dump_save_ram().unwrap(),
                machine.dump_save_ram().unwrap()
            );
            if *pc == busy {
                for (offset, byte) in [(0, 2), (3, 1), (3, 255), (4, 255), (17, 3), (26, 255)] {
                    let mut corrupt = raw.clone();
                    corrupt[range.start + offset] = byte;
                    assert!(machine.decode_state(&corrupt).is_err());
                    assert_eq!(machine.encode_state().unwrap(), raw);
                }
                assert!(machine.decode_state(&raw[..raw.len() - 1]).is_err());
                assert_eq!(machine.encode_state().unwrap(), raw);
                assert!(
                    machine
                        .decode_frontend_state(&front[..front.len() - 1])
                        .is_err()
                );
                assert_eq!(machine.encode_frontend_state().unwrap(), front);
                let mut other = bytes.clone();
                other[0x2fffff] ^= 1;
                let mut wrong_rom = Emulator::new(other, config).unwrap();
                let before = wrong_rom.encode_state().unwrap();
                assert!(wrong_rom.decode_state(&raw).is_err());
                assert_eq!(wrong_rom.encode_state().unwrap(), before);
            }
        }
        advance(&mut machine, None);
        assert_eq!(sample(&machine, ack), 0);
        assert_eq!(received(&machine, value), 0x96);
        let mut expected = cells();
        expected[3] = 0x96;
        assert_eq!(machine.dump_save_ram().unwrap(), expected);
        let final_raw = machine.encode_state().unwrap();
        assert!(
            futures
                .iter()
                .all(|(raw, save)| raw == &final_raw && save == &expected)
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn split_serial_profile_persistence_and_current_state() {
    let (bytes, _) = Guest::new().rom.finish();
    let mut machine = Emulator::new(bytes.clone(), LoadConfig::default()).unwrap();
    assert_eq!(machine.dump_save_ram().unwrap(), [0xff; 256]);
    let before = machine.encode_state().unwrap();
    assert_eq!(state_layout::sections(&before).version, 1);
    for size in [0, 128, 255, 257, 512] {
        assert!(machine.load_save_ram(&vec![0; size]).is_err());
        assert_eq!(machine.encode_state().unwrap(), before);
    }
    machine.load_save_ram(&cells()).unwrap();
    machine.reset();
    assert_eq!(machine.dump_save_ram().unwrap(), cells());
    for size in [0x200000, 0x2fffff, 0x300001, 0x400000] {
        let mut wrong = bytes.clone();
        wrong.resize(size, 0xff);
        assert!(matches!(
            Emulator::new(wrong, LoadConfig::default()),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
    for offset in (0x180..0x18e).chain(0x1b2..0x1bc) {
        let mut wrong = bytes.clone();
        wrong[offset] ^= 1;
        assert!(matches!(
            Emulator::new(wrong, LoadConfig::default()),
            Err(LoadError::UnsupportedSaveHardware)
        ));
    }
    for cells in [128, 256] {
        let mut rom = fixture::Rom::new();
        if cells == 256 {
            rom.bytes.resize(0x200000, 0);
            rom.bytes[0x180..0x18e].copy_from_slice(b"GM T-081326 01");
        }
        let (old_rom, _) = rom.finish();
        let mut old = Emulator::new(old_rom, LoadConfig::default()).unwrap();
        let raw = old.encode_state().unwrap();
        assert_eq!(state_layout::sections(&raw).version, 1);
        let range = state_layout::serial_range(&raw, cells);
        assert_eq!(
            &raw[range],
            &[
                1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
        old.decode_state(&raw).unwrap();
        assert_eq!(old.encode_state().unwrap(), raw);
        assert_eq!(old.dump_save_ram().unwrap(), vec![0xff; cells]);
    }
}
