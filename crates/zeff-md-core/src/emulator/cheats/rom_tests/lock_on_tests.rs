use super::*;
use crate::cartridge::{Cartridge, registers::LockOn};
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::sound::{CandidateBus, SoundFault};
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Z80Bus;

mod consumer;
mod save;
mod sound;
mod state;

const MODES: [LockOn; 4] = [LockOn::Empty, LockOn::HalfMiB, LockOn::MiB, LockOn::Patch];
const REGIONS: [Region; 3] = [Region::NtscUs, Region::NtscJapan, Region::Pal];

fn cartridge(mapping: LockOn) -> Cartridge {
    let mut bytes = crate::cartridge::registers::tests::normal_rom_fixture(mapping);
    write_words(&mut bytes, 0x200, &[0x4ef9, 0x00ff, 0x0200, 0x4e71]);
    assert!(LockOn::detect(&bytes).is_none());
    if mapping == LockOn::TwoMiB {
        return crate::cartridge::registers::tests::inserted_save_fixture_with_rom(bytes);
    }
    let mut cartridge = Cartridge::new(bytes).unwrap();
    assert!(cartridge.lock_on.is_none());
    cartridge.lock_on = Some(mapping);
    cartridge
}

fn machine(mapping: LockOn, region: Region, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut machine = Emulator::new_with_cartridge(
        cartridge(mapping),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    write_words(&mut machine.bus.ram, 0x200, program);
    if mapping == LockOn::TwoMiB {
        consumer::setup(&mut machine, 0x300000);
    }
    machine.step_instruction().unwrap();
    assert_eq!(machine.registers().pc, 0xff0200);
    machine
}

fn word(set: &mut Vec<CheatPatch>, address: u32, value: u16) {
    let [high, low] = value.to_be_bytes();
    set.extend([patch(address, high), patch(address + 1, low)]);
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn read(machine: &mut Emulator, address: u32, lanes: ByteLanes) -> Result<u16, crate::BusError> {
    machine
        .bus
        .read_cycle(access(address, lanes), machine.bus.cpu_clock)
        .result
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_overlay_no_save_sources_reach_main_sound_dma_aliases_without_populating_empty() {
    for region in REGIONS {
        for concurrent in [false, true] {
            for mode in MODES {
                let mut machine = machine(mode, region, concurrent, &[0x60fe, 0x4e71]);
                assert!(machine.supports_rom_cheats());
                assert!(machine.dump_save_ram().is_none());
                assert!(
                    crate::hardware::bus::Bus::new_owned(machine.bus.cartridge.clone(), region)
                        .is_err()
                );
                let bytes = machine.bus.cartridge.bytes.clone();
                let digest = machine.bus.cartridge.rom_digest();
                let mut set = Vec::new();
                for (source, value) in [
                    (0x1000, 0xa55a),
                    (0x200000, 0x1234),
                    (0x280000, 0x5678),
                    (0x300000, 0xabcd),
                ] {
                    if (source as usize) < bytes.len() {
                        word(&mut set, source, value);
                    }
                }
                let target = if mode == LockOn::Empty {
                    0x1100
                } else {
                    0x200100
                };
                let data = if mode == LockOn::Empty {
                    0x1000
                } else {
                    0x200000
                };
                for (index, opcode) in [
                    0x303c,
                    0x77,
                    0x3439,
                    (data >> 16) as u16,
                    data as u16,
                    0x4e75,
                    0x4e71,
                ]
                .into_iter()
                .enumerate()
                {
                    word(&mut set, target + 2 * index as u32, opcode);
                }
                machine.install_rom_patches(&set).unwrap();
                let return_pc = machine.registers().pc;
                let return_sp = machine.registers().stack_pointer();
                let context = machine.begin_debug_call(target).unwrap();
                for _ in 0..3 {
                    machine.step_instruction().unwrap();
                }
                assert!(machine.debug_call_returned(&context));
                machine.finish_debug_call(context).unwrap();
                assert_eq!(machine.registers().pc, return_pc);
                assert_eq!(machine.registers().stack_pointer(), return_sp);
                assert_eq!(machine.registers().d[0], 0x77);
                assert_eq!(
                    machine.registers().d[2] as u16,
                    if mode == LockOn::Empty {
                        0xa55a
                    } else {
                        0x1234
                    }
                );
                if mode == LockOn::Empty {
                    let before = machine.encode_frontend_state().unwrap();
                    assert!(machine.check_debug_call(0x200000).is_err());
                    assert_eq!(machine.encode_frontend_state().unwrap(), before);
                    assert_eq!(machine.rom_offset_for_cpu_address(0x300000), None);
                    assert!(machine.install_rom_patches(&[patch(0x200000, 1)]).is_err());
                    assert_eq!(machine.encode_frontend_state().unwrap(), before);
                }
                let cases: &[(u32, u16)] = match mode {
                    LockOn::Empty => &[(0x1000, 0xa55a), (0x200000, 0xffff), (0x300000, 0xffff)],
                    LockOn::HalfMiB => &[
                        (0x1000, 0xa55a),
                        (0x200000, 0x1234),
                        (0x280000, 0x1234),
                        (0x300000, 0x1234),
                        (0x380000, 0x1234),
                    ],
                    LockOn::MiB => &[
                        (0x1000, 0xa55a),
                        (0x200000, 0x1234),
                        (0x280000, 0x5678),
                        (0x300000, 0x1234),
                        (0x380000, 0x5678),
                    ],
                    LockOn::Patch => &[
                        (0x1000, 0xa55a),
                        (0x200000, 0x1234),
                        (0x280000, 0x5678),
                        (0x300000, 0xabcd),
                        (0x340000, 0xabcd),
                        (0x380000, 0xabcd),
                        (0x3c0000, 0xabcd),
                    ],
                    LockOn::TwoMiB => unreachable!(),
                };
                for &(address, expected) in cases {
                    write_words(
                        &mut machine.bus.ram,
                        0x300,
                        &[
                            0x1639,
                            (address >> 16) as u16,
                            address as u16,
                            0x1839,
                            ((address + 1) >> 16) as u16,
                            (address + 1) as u16,
                            0x4e75,
                            0x4e71,
                        ],
                    );
                    let context = machine.begin_debug_call(0xff0300).unwrap();
                    for _ in 0..3 {
                        machine.step_instruction().unwrap();
                    }
                    assert!(machine.debug_call_returned(&context));
                    machine.finish_debug_call(context).unwrap();
                    assert_eq!(machine.registers().d[3] as u8, (expected >> 8) as u8);
                    assert_eq!(machine.registers().d[4] as u8, expected as u8);
                }
                for &(address, expected) in cases {
                    assert_eq!(read(&mut machine, address, ByteLanes::Both), Ok(expected));
                    assert_eq!(read(&mut machine, address, ByteLanes::Upper), Ok(expected));
                    assert_eq!(
                        read(&mut machine, address + 1, ByteLanes::Lower),
                        Ok(expected)
                    );
                    VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram)
                        .assert_word(address, expected);
                    let mut memory = machine.bus.sound.clone();
                    memory.bank = (address >> 15) as u16;
                    let candidate = CandidateBus::new(&mut memory, &machine.bus.cartridge);
                    let local = 0x8000 | (address & 0x7fff) as u16;
                    assert_eq!(candidate.cpu_read(local), (expected >> 8) as u8);
                    assert_eq!(candidate.cpu_read(local + 1), expected as u8);
                    assert_eq!(candidate.fault.get(), None);
                }
                assert!(Arc::ptr_eq(&bytes, &machine.bus.cartridge.bytes));
                assert_eq!(machine.bus.cartridge.rom_digest(), digest);
                assert_eq!(machine.bus.cartridge.lock_on, Some(mode));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_overlay_cold_prime_uses_reset_mapping_and_boundary_restart_keeps_live_patch() {
    for mode in MODES {
        for region in REGIONS {
            for concurrent in [false, true] {
                let targets: &[u32] = if mode == LockOn::Empty {
                    &[0x1ffffc, 0xab1ffffc]
                } else {
                    &[0x1ffffe, 0x2ffffe, 0x3ffffc, 0xab3ffffc]
                };
                for &pc in targets {
                    let mut machine = machine(mode, region, concurrent, &[0x60fe, 0x4e71]);
                    let bytes = machine.bus.cartridge.bytes.clone();
                    let initial = machine.bus.cartridge.initial;
                    let address = pc & 0xffffff;
                    let sources = match (mode, address) {
                        (LockOn::Empty, _) => [0x1ffffc, 0x1ffffe],
                        (_, 0x1ffffe) => [0x1ffffe, 0x200000],
                        (LockOn::HalfMiB, 0x2ffffe) => [0x27fffe, 0x200000],
                        (_, 0x2ffffe) => [0x2ffffe, 0x200000],
                        (LockOn::HalfMiB, _) => [0x27fffc, 0x27fffe],
                        _ => [0x2ffffc, 0x2ffffe],
                    };
                    let mut set = patches(&format!(
                        "000000:0000+000002:0000+000004:{:04X}+000006:{:04X}",
                        pc >> 16,
                        pc & 0xffff
                    ));
                    word(&mut set, sources[0], 0x4444);
                    word(&mut set, sources[1], 0x5555);
                    if mode == LockOn::Patch && address == 0x3ffffc {
                        word(&mut set, 0x33fffc, 0xaaaa);
                        word(&mut set, 0x33fffe, 0xbbbb);
                    }
                    machine.install_rom_patches(&set).unwrap();
                    assert_eq!(machine.bus.cartridge.lock_on, Some(mode));
                    if mode == LockOn::Patch && address == 0x3ffffc {
                        machine.reset_cpu().unwrap();
                        assert_eq!(machine.queued_words(), Some([0xaaaa, 0xbbbb]));
                        assert_eq!(machine.bus.cartridge.lock_on, Some(LockOn::Patch));
                    }
                    machine.reset();
                    assert_eq!((machine.registers().pc, machine.registers().ssp), (pc, 0));
                    assert_eq!(machine.queued_words(), Some([0x4444, 0x5555]));
                    assert_eq!(
                        machine.bus.cartridge.lock_on,
                        Some(if mode == LockOn::Patch {
                            LockOn::MiB
                        } else {
                            mode
                        })
                    );
                    let before = machine.clone();
                    for bad in [0x201_u32, 0x3ffffe, 0x400000, 0xab3ffffe] {
                        let set = patches(&format!(
                            "000004:{:04X}+000006:{:04X}",
                            bad >> 16,
                            bad & 0xffff
                        ));
                        assert!(machine.install_rom_patches(&set).is_err());
                        assert_eq!(machine, before);
                    }
                    if mode == LockOn::Empty {
                        assert!(
                            machine
                                .install_rom_patches(&patches("000004:001F+000006:FFFE"))
                                .is_err()
                        );
                        assert!(
                            machine
                                .install_rom_patches(&patches("000004:0020+000006:0000"))
                                .is_err()
                        );
                        assert_eq!(machine, before);
                    }
                    assert!(machine.check_warm_reset().is_err());
                    assert!(machine.warm_reset().is_err());
                    assert_eq!(machine, before);
                    machine.clear_rom_patches();
                    machine.reset();
                    assert_eq!(machine.registers().pc, initial.pc);
                    assert_eq!(machine.registers().ssp, initial.ssp);
                    assert!(Arc::ptr_eq(&bytes, &machine.bus.cartridge.bytes));
                    assert_eq!(machine.bus.cartridge.initial, initial);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_unsealed_inserted_save_consumer_preserves_cells_and_declared_admission() {
    let mut machine = machine(LockOn::TwoMiB, Region::NtscUs, true, &[0x60fe, 0x4e71]);
    machine.load_save_ram(&[0x5a; 512]).unwrap();
    for enabled in [false, true] {
        machine.bus.cartridge.lock_on_save.as_mut().unwrap().enabled = enabled;
        let before = machine.encode_frontend_state().unwrap();
        assert!(!machine.supports_rom_cheats());
        consumer::overlay(&mut machine, &[patch(0x1000, 0xa5)]);
        assert!(machine.check_debug_call(0x1000).is_err());
        assert_eq!(machine.rom_offset_for_cpu_address(0x1000), None);
        assert_eq!(machine.encode_frontend_state().unwrap(), before);
        machine.clear_rom_patches();
        assert_eq!(machine.encode_frontend_state().unwrap(), before);
        assert_eq!(machine.dump_save_ram().unwrap(), [0x5a; 512]);
    }
    for stripped in 0..3 {
        let mut malformed = machine.clone();
        if stripped != 1 {
            malformed.bus.cartridge.lock_on_save = None;
        }
        if stripped != 0 {
            malformed.bus.cartridge.lock_on = None;
        }
        let ordinary_control = stripped == 2;
        assert_eq!(malformed.supports_rom_cheats(), ordinary_control);
        assert_eq!(malformed.check_debug_call(0x1000).is_ok(), ordinary_control);
        let before = malformed.clone();
        let installed = malformed.install_rom_patches(&[patch(0x1000, 0xa5)]);
        assert_eq!(installed.is_ok(), ordinary_control);
        if !ordinary_control {
            assert_eq!(malformed, before);
        }
    }
    let mut unsealed = Cartridge::new(crate::cartridge::registers::tests::normal_rom_fixture(
        LockOn::TwoMiB,
    ))
    .unwrap();
    unsealed.lock_on = Some(LockOn::TwoMiB);
    unsealed.lock_on_save = Some(crate::cartridge::registers::LockOnSave::new());
    assert!(!unsealed.supports_rom_overlay());
    assert!(
        unsealed
            .prepare_rom_overlay(&[patch(0x1000, 0xa5)])
            .is_err()
    );
    save::main_contract();
}
