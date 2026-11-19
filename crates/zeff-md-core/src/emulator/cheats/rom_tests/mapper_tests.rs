use super::*;
use crate::cartridge::{Cartridge, ssf2::tests as ssf};
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_m68k::bus::Direction;

mod large;
mod owners;
mod sound;
mod state;

const PAGE: usize = 0x80000;
const WITHIN: u32 = 0x1000;
const WINDOW: u32 = PAGE as u32 + WITHIN;

#[derive(Clone, Copy, Debug)]
enum Family {
    Ssf2,
    Extended,
}

const FAMILIES: [Family; 2] = [Family::Ssf2, Family::Extended];
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn extended_image(size: usize, program: &[u16]) -> Vec<u8> {
    let mut rom = vec![0; size];
    for (page, bytes) in rom.chunks_mut(PAGE).enumerate() {
        bytes.fill(page as u8);
    }
    rom[..0x200].fill(b' ');
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x108].copy_from_slice(b"SEGA SSF");
    write_words(&mut rom, 0x200, program);
    rom
}

fn config(region: Region, concurrent_sound: bool) -> LoadConfig {
    LoadConfig {
        region,
        concurrent_sound,
    }
}

fn mapper(family: Family, program: &[u16], region: Region, concurrent: bool) -> Emulator {
    let cart = match family {
        Family::Ssf2 => ssf::cartridge(program),
        Family::Extended => Cartridge::new(extended_image(12 * PAGE, program)).unwrap(),
    };
    Emulator::new_with_cartridge(cart, config(region, concurrent)).unwrap()
}

fn source(page: u8, within: u32) -> u32 {
    u32::from(page) * PAGE as u32 + within
}

fn control(family: Family, slot: u32) -> u32 {
    0xa130f0 + slot * 2 + u32::from(matches!(family, Family::Ssf2))
}

fn select(machine: &mut Emulator, family: Family, slot: u32, page: u8) {
    let address = control(family, slot);
    match family {
        Family::Ssf2 => assert!(
            machine
                .bus
                .cartridge
                .ssf2
                .as_mut()
                .unwrap()
                .control(ssf::access(address), u16::from(page))
        ),
        Family::Extended => machine
            .bus
            .cartridge
            .extended_ssf
            .as_mut()
            .unwrap()
            .control(address, page),
    }
}

fn write_byte(program: &mut Vec<u16>, address: u32, value: u8) {
    program.extend([
        0x13fc,
        u16::from(value),
        (address >> 16) as u16,
        address as u16,
    ]);
}

fn advance(machine: &mut Emulator, ticks: u64) {
    ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    )
    .advance_machine(ticks)
    .unwrap();
}

fn replace_page(machine: &mut Emulator, family: Family, page: u8, bytes: &[u8]) {
    match family {
        Family::Ssf2 => ssf::page_program(&mut machine.bus.cartridge, page as usize, bytes),
        Family::Extended => {
            let start = usize::from(page) * PAGE;
            Arc::make_mut(&mut machine.bus.cartridge.bytes)[start..start + bytes.len()]
                .copy_from_slice(bytes);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_admission_is_bounded_and_precise_stays_rejected() {
    for size in [12 * PAGE, 12 * PAGE + 4, 16 * PAGE, 16 * PAGE + 1] {
        let rom = extended_image(size, &[0x60fe, 0x4e71]);
        let mut machine = Emulator::new(rom.clone(), config(Region::NtscUs, true)).unwrap();
        assert!(machine.supports_rom_cheats());
        let before = machine.clone();
        let result = machine.install_rom_patches(&[patch((size - 1) as u32, 0xa5)]);
        assert!(result.is_ok());
        if result.is_err() {
            assert_eq!(machine, before);
        } else {
            select(&mut machine, Family::Extended, 1, ((size - 1) / PAGE) as u8);
            assert_eq!(
                machine.peek(PAGE as u32 + ((size - 1) % PAGE) as u32),
                Some(0xa5)
            );
        }
        assert!(
            Emulator::new_with_timing(rom, config(Region::NtscUs, true), TimingModel::Precise)
                .is_err()
        );
    }
    let cart = ssf::cartridge(&[0x60fe, 0x4e71]);
    assert!(crate::hardware::bus::Bus::new_owned(cart, Region::NtscUs).is_err());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_source_aliases_boundaries_and_original_compares_preserve_identity() {
    for family in FAMILIES {
        let mut machine = mapper(family, &[0x60fe, 0x4e71], Region::NtscUs, true);
        let source_bytes = machine.bus.cartridge.bytes.clone();
        let digest = machine.bus.cartridge.rom_digest();
        let header = machine.bus.cartridge.header.clone();
        let initial = machine.bus.cartridge.initial;
        let mut set = patches("401000:A55A + 47FFFF:C7 + 480000:D8");
        set.extend([
            CheatPatch::WideRomWriteIfEquals {
                address: source(8, WITHIN),
                value: CheatValue::Constant(0x5a),
                compare: CheatValue::Constant(8),
            },
            CheatPatch::WideRomWriteIfEquals {
                address: source(8, WITHIN),
                value: CheatValue::Constant(0xee),
                compare: CheatValue::Constant(0xa5),
            },
            CheatPatch::WideRomWrite {
                address: source(8, WITHIN + 1),
                value: CheatValue::PreserveWithCurrent {
                    mask: 0xf0,
                    base: 0x0b,
                },
            },
        ]);
        machine.install_rom_patches(&set).unwrap();
        for slot in 1..8 {
            select(&mut machine, family, slot, 8);
            let address = slot * PAGE as u32 + WITHIN;
            assert_eq!(machine.peek(address), Some(0x5a));
            assert_eq!(machine.peek(address + 1), Some(0x0b));
            assert_eq!(machine.peek((slot + 1) * PAGE as u32 - 1), Some(0xc7));
            select(&mut machine, family, slot, 9);
            assert_eq!(machine.peek(address), Some(9));
            assert_eq!(machine.peek(slot * PAGE as u32), Some(0xd8));
        }
        select(&mut machine, family, 1, 8);
        select(&mut machine, family, 2, 8);
        assert_eq!(machine.peek(WINDOW), machine.peek(0x101000));
        let before = machine.clone();
        let len = machine.bus.cartridge.bytes.len() as u32;
        for bad in [len, 0x800000, 0xff0000, u32::MAX] {
            assert!(
                machine
                    .install_rom_patches(&[patch(source(8, WITHIN), 0), patch(bad, 1)])
                    .is_err()
            );
            assert_eq!(machine, before);
        }
        assert!(Arc::ptr_eq(&source_bytes, &machine.bus.cartridge.bytes));
        assert_eq!(machine.bus.cartridge.rom_digest(), digest);
        assert_eq!(machine.bus.cartridge.header, header);
        assert_eq!(machine.bus.cartridge.initial, initial);
        machine.clear_rom_patches();
        assert_eq!(machine.peek(WINDOW), Some(8));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extended_slot_zero_partial_sources_and_game_genie_use_source_offsets() {
    let mut machine = mapper(Family::Extended, &[0x60fe, 0x4e71], Region::NtscUs, true);
    select(&mut machine, Family::Extended, 0, 8);
    machine
        .install_rom_patches(&patches("MJBT-AMNA + 401000:A55A"))
        .unwrap();
    assert_eq!(machine.peek(0x380), Some(8));
    assert_eq!(machine.peek(WITHIN), Some(0xa5));
    select(&mut machine, Family::Extended, 1, 0);
    assert_eq!(machine.peek(0x80380), Some(0xa5));
    assert_eq!(machine.peek(0x80381), Some(0x5a));
    let bytes = extended_image(12 * PAGE + 4, &[0x60fe, 0x4e71]);
    let mut partial = Emulator::new(bytes, config(Region::NtscUs, true)).unwrap();
    partial.install_rom_patches(&patches("600003:A5")).unwrap();
    select(&mut partial, Family::Extended, 1, 12);
    assert_eq!(partial.peek(0x80003), Some(0xa5));
    assert_eq!(partial.peek(0x80004), None);
    select(&mut partial, Family::Extended, 1, 15);
    assert_eq!(partial.peek(0x80000), None);
    let before = partial.clone();
    assert!(partial.install_rom_patches(&patches("600004:A5")).is_err());
    assert_eq!(partial, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_cold_reset_uses_source_vectors_before_bank_prime() {
    for family in FAMILIES {
        for region in REGIONS {
            let mut machine = mapper(family, &[0x60fe, 0x4e71], region, true);
            select(&mut machine, family, 1, 9);
            if matches!(family, Family::Extended) {
                select(&mut machine, family, 0, 8);
            }
            machine
                .install_rom_patches(&patches(
                    "000000:1234 + 000002:5679 + 000004:8000 + 000006:0200 + 000200:7007",
                ))
                .unwrap();
            let before = machine.clone();
            for code in [
                "000007:01",
                "000004:0040 + 000006:0000",
                "000004:003F + 000006:FFFE",
            ] {
                assert!(machine.install_rom_patches(&patches(code)).is_err());
                assert_eq!(machine, before);
            }
            let mut end = machine.clone();
            end.install_rom_patches(&patches("000004:003F + 000006:FFFC"))
                .unwrap();
            end.reset();
            assert_eq!(end.registers().pc, 0x3ffffc);
            assert_eq!(end.queued_words(), Some([0x0707, 0x0707]));
            machine.reset();
            assert_eq!(machine.registers().pc, 0x80000200);
            assert_eq!(machine.registers().ssp, 0x12345679);
            assert_eq!(machine.queued_words(), Some([0x7007, 0x4e71]));
            assert_eq!(machine.peek(0x81000), Some(1));
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0], 7);
            assert!(machine.check_warm_reset().is_err());
            if matches!(family, Family::Extended) {
                let before = machine.clone();
                assert!(machine.reset_cpu().is_err());
                assert_eq!(machine, before);
            } else {
                select(&mut machine, family, 1, 9);
                machine.reset_cpu().unwrap();
                assert_eq!(machine.peek(WINDOW), Some(9));
            }
        }
    }
}
