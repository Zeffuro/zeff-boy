use super::*;

mod overlay;
use crate::cartridge::sf002::{Mapping, Sf002};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::BusResponse;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

pub(crate) fn access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes: if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

pub(crate) fn cartridge() -> Cartridge {
    let mut rom = vec![0x17; 0x220000];
    rom[0x200000..].fill(0x83);
    rom[..4].copy_from_slice(&0xffff_f000_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x180..0x18e].copy_from_slice(b"SF-002        ");
    rom[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x3c\x00\x01\x00\x3c\x3f\xff");
    let program = [0x13fc_u16, 0, 0, 0xe01, 0x4ef9, 0x20, 0x7000];
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let old = [0x13fc_u16, 0x81, 0, 0xe01, 0x7008, 0x4ef9, 0x20, 0x7008];
    let mut new = old;
    new[4] = 0x7009;
    for (base, words) in [(0x207000, old), (0x7000, new)] {
        for (index, word) in words.into_iter().enumerate() {
            rom[base + index * 2..base + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    let mut cart = Cartridge::new(rom).unwrap();
    assert!(!Sf002::detect(&cart.bytes));
    cart.sf002 = Some(Sf002::new());
    cart
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_guarded_map_main_prefetch_reset_and_codec_boundaries() {
    let cart = cartridge();
    let mut machine = Emulator::new(cart.bytes.to_vec(), LoadConfig::default()).unwrap();
    let legacy = machine.encode_state().unwrap();
    machine.bus.cartridge = cart;
    let before = machine.clone();
    assert!(machine.decode_state(&legacy).is_err());
    assert_eq!(machine, before);
    assert_eq!(machine.bus.cartridge.read(0x131638), Some(0x17));
    for address in [0x200000, 0x21ffff, 0x220000, 0x3bffff, 0x3c4000, 0x3fffff] {
        assert_eq!(machine.bus.peek(address), None);
        assert_eq!(machine.bus.cartridge.rom_source_offset(address), None);
        let mut bus = machine.bus.clone();
        let clock = bus.cpu_clock;
        assert!(bus.read_cycle(access(address), clock).result.is_err());
    }
    for (mut transfer, value) in [
        (access(0xe01), 0x81),
        (access(0x101), 0),
        (access(0xe00), 0),
        (access(0xa130f1), 3),
        (access(0x3c4001), 0),
    ] {
        for lanes in [transfer.lanes, ByteLanes::Both] {
            transfer.lanes = lanes;
            let before = machine.clone();
            assert!(
                machine
                    .bus
                    .write_cycle(transfer, value, before.bus.cpu_clock)
                    .result
                    .is_err()
            );
            assert_eq!(machine, before);
        }
    }
    machine.step_instruction().unwrap();
    assert_eq!(
        machine.bus.cartridge.sf002.as_ref().unwrap().mapping(),
        Mapping::Linear
    );
    assert_eq!(machine.bus.cartridge.read(0x21ffff), Some(0x83));
    assert_eq!(machine.bus.cartridge.read(0x220000), None);
    machine.step_instruction().unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(
        machine.bus.cartridge.rom_source_offset(0x207008),
        Some(0x7008)
    );
    assert_eq!(machine.cpu.queued_words().unwrap()[0], 0x7008);
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().d[0], 8);
    machine.step_instruction().unwrap();
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().d[0], 9);
    let before = machine.clone();
    assert_eq!(
        machine.reset_cpu(),
        Err(TimingFault::ResetOutputUnsupported)
    );
    assert_eq!(machine, before);
    assert!(machine.check_warm_reset().is_err());
    assert_eq!(
        machine.bus.begin_reset_output(machine.bus.cpu_clock),
        Ok(BusResponse::Unsupported)
    );
    assert_eq!(machine, before);
    let board = machine.bus.cartridge.sf002.as_mut().unwrap();
    let mut writer = StateWriter::new();
    board.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = Sf002::new();
    restored.read_state(&mut StateReader::new(&bytes)).unwrap();
    assert_eq!(*board, restored);
    for bad in [b"SF02\x03".as_slice(), b"BAD!\x01", b"SF02"] {
        let before = restored;
        assert!(restored.read_state(&mut StateReader::new(bad)).is_err());
        assert_eq!(restored, before);
    }
    assert!(machine.encode_state().is_err());
    machine.bus.cartridge.save.as_mut().unwrap().bytes[0] = 0x42;
    machine.reset();
    assert_eq!(
        machine.bus.cartridge.sf002.as_ref().unwrap().mapping(),
        Mapping::Unknown
    );
    assert_eq!(machine.bus.cartridge.read(0x3c0001), Some(0x42));
    assert_eq!(machine.bus.cartridge.read(0x200000), None);
}
