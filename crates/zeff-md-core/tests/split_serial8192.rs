#[path = "eeprom/fixture.rs"]
mod fixture;
#[path = "split_serial/guest.rs"]
mod legacy_guest;
#[path = "support/state_layout.rs"]
mod state_layout;

use fixture::{advance, sample};
use legacy_guest::{Guest, received};
use zeff_emu_common::time::{MachineTiming, Reset};
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

fn guest(college: bool) -> Guest {
    let mut g = Guest::new();
    g.rom.bytes.resize(0x400000, 0);
    g.rom.bytes[0x180..0x18e].copy_from_slice(if college {
        b"GM T-81576 -00"
    } else {
        b"GM T-81476 -00"
    });
    if college {
        g.rom.bytes[0x1b7] = 1;
    }
    g
}

fn cells() -> Vec<u8> {
    (0..8192_u16)
        .map(|n| (n as u8).wrapping_mul(73) ^ ((n >> 8) as u8).wrapping_mul(29) ^ 0x59)
        .collect()
}

fn address(g: &mut Guest, target: u16) -> [usize; 3] {
    g.start();
    [
        g.send(0xa0),
        g.send((target >> 8) as u8),
        g.send(target as u8),
    ]
}

fn read(g: &mut Guest, target: u16) -> usize {
    address(g, target);
    g.start();
    g.send(0xa1)
}

fn serial(raw: &[u8]) -> std::ops::Range<usize> {
    state_layout::serial_payload_range(raw, 8192, 92)
}

fn word(payload: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(payload[offset..offset + 2].try_into().unwrap())
}

fn wait(g: &mut Guest) {
    for _ in 0..3 {
        g.wait();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_read_carry_and_rejected_addresses_preserve_pointer() {
    for college in [false, true] {
        let mut g = guest(college);
        g.mapping(false);
        let mut values = Vec::new();
        for target in (0..32)
            .map(|n| n * 256 + 0x35)
            .chain([0xff, 0x7ff, 0xfff, 0x1fff])
        {
            read(&mut g, target);
            let first = g.receive(true, false);
            let next = g.receive(false, false);
            values.push((target, first, next));
            g.stop();
        }
        let mut rejected = Vec::new();
        for control in [0xa2, 0xa3, 0xae, 0xaf, 0xb0] {
            address(&mut g, 0x1357);
            g.start();
            let ack = g.send(control);
            g.send(0);
            let cut = g.mark();
            g.stop();
            g.start();
            g.send(0xa1);
            rejected.push((ack, cut, g.receive(false, false)));
            g.stop();
        }
        for high in [0x20, 0x80, 0xe0, 0xff] {
            address(&mut g, 0x1357);
            g.start();
            g.send(0xa0);
            let ack = g.send(high);
            g.send(0x66);
            g.send(0x96);
            let cut = g.mark();
            g.stop();
            g.start();
            g.send(0xa1);
            rejected.push((ack, cut, g.receive(false, false)));
            g.stop();
        }
        let (bytes, _) = g.rom.finish();
        for config in configs() {
            let initial = cells();
            let mut machine = Emulator::new(bytes.clone(), config).unwrap();
            machine.load_save_ram(&initial).unwrap();
            for (_, cut, _) in &rejected {
                advance(&mut machine, Some(*cut));
                let raw = machine.encode_state().unwrap();
                assert_eq!(word(&raw[serial(&raw)], 6), 0x1357);
                continuations::assert_futures(&machine, &bytes, config, false);
            }
            advance(&mut machine, None);
            for (target, first, next) in &values {
                assert_eq!(received(&machine, *first), initial[usize::from(*target)]);
                assert_eq!(
                    received(&machine, *next),
                    initial[usize::from((target + 1) & 0x1fff)]
                );
            }
            for (ack, _, first) in &rejected {
                assert_eq!(sample(&machine, *ack), 1);
                assert_eq!(received(&machine, *first), initial[0x1357]);
            }
            assert_eq!(machine.dump_save_ram().unwrap(), initial);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_nonboundary_cache_wrap_dirty63_and_software_pages() {
    for college in [false, true] {
        let mut g = guest(college);
        g.mapping(false);
        let mut expected = cells();
        let mut checkpoints = Vec::new();
        let mut accepted = Vec::new();
        let mut busy = Vec::new();
        for (target, length) in [
            (0x1ffb_u16, 70),
            (0x03fd, 11),
            (0x0007, 64),
            (0x1020, 8),
            (0x1028, 8),
        ] {
            accepted.extend(address(&mut g, target));
            let mut dirty = 0_u64;
            for n in 0..length {
                let value = (n as u8).wrapping_mul(31) ^ (target as u8);
                accepted.push(g.send(value));
                let offset = ((target & 7) + n) & 63;
                dirty |= 1_u64 << offset;
                expected[usize::from(((target & !7) + offset) & 0x1fff)] = value;
            }
            let before = g.mark();
            g.stop();
            let committed = g.mark();
            g.start();
            busy.push(g.send(0xa0));
            g.stop();
            wait(&mut g);
            g.start();
            accepted.push(g.send(0xa0));
            g.stop();
            let ready = g.mark();
            checkpoints.push((before, committed, ready, dirty, expected.clone()));
        }
        let (bytes, _) = g.rom.finish();
        for config in configs() {
            let mut previous = cells();
            let mut machine = Emulator::new(bytes.clone(), config).unwrap();
            machine.load_save_ram(&previous).unwrap();
            for (before, committed, ready, dirty, next) in &checkpoints {
                advance(&mut machine, Some(*before));
                assert_eq!(machine.dump_save_ram().unwrap(), previous);
                let raw = machine.encode_state().unwrap();
                let payload = &raw[serial(&raw)];
                assert_eq!(
                    u64::from_le_bytes(payload[76..84].try_into().unwrap()),
                    *dirty
                );
                advance(&mut machine, Some(*committed));
                assert_eq!(machine.dump_save_ram().unwrap(), *next);
                let raw = machine.encode_state().unwrap();
                let until = u64::from_le_bytes(raw[serial(&raw)][84..92].try_into().unwrap());
                let pages = (0..8)
                    .filter(|p| dirty & (0xff_u64 << (p * 8)) != 0)
                    .count() as u64;
                let page_ticks = config.region.master_hz().div_ceil(200);
                let remaining = until.saturating_sub(machine.timing_snapshot().now().get());
                assert!(remaining <= pages * page_ticks && remaining > (pages - 1) * page_ticks);
                advance(&mut machine, Some(*ready));
                previous = next.clone();
            }
            advance(&mut machine, None);
            for ack in &accepted {
                assert_eq!(sample(&machine, *ack), 0);
            }
            for ack in &busy {
                assert_eq!(sample(&machine, *ack), 1);
            }
            assert_eq!(machine.dump_save_ram().unwrap(), expected);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_exact_header_product_size_and_save_capacity() {
    for college in [false, true] {
        let (bytes, _) = guest(college).rom.finish();
        let machine = Emulator::new(bytes.clone(), LoadConfig::default()).unwrap();
        assert_eq!(machine.dump_save_ram().unwrap(), vec![0xff; 8192]);
        for size in [0x200000, 0x300000, 0x3fffff, 0x400001] {
            let mut wrong = bytes.clone();
            wrong.resize(size, 0);
            assert!(Emulator::new(wrong, LoadConfig::default()).is_err());
        }
        for offset in (0x180..0x18e).chain(0x1b2..0x1bc) {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1;
            assert!(Emulator::new(wrong, LoadConfig::default()).is_err());
        }
        for size in [0, 128, 256, 2048, 8191, 8193] {
            let mut copy = machine.clone();
            let before = copy.clone();
            assert!(copy.load_save_ram(&vec![0; size]).is_err());
            assert_eq!(copy, before);
        }
    }
}

#[path = "split_serial8192/continuations.rs"]
mod continuations;
