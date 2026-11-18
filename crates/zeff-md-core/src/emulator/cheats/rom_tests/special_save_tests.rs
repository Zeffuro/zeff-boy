use super::*;
use crate::cartridge::ram::profile_tests::overlay_fixture::{ALL, Profile, cartridge};
use crate::emulator::InstructionObserver;
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, InstructionByteOrigin, InstructionInput};

mod pending;
mod state;

fn enter_expansion_ram(machine: &mut Emulator, program: &[u16]) {
    write_words(&mut machine.bus.ram, 0x200, program);
    let entry = machine.begin_debug_call(0xff0200).unwrap();
    assert_eq!(machine.registers().pc, 0xff0200);
    assert!(!machine.debug_call_returned(&entry));
}

fn machine(profile: Profile, program: &[u16], concurrent_sound: bool) -> Emulator {
    let mut machine = Emulator::new_with_cartridge(
        cartridge(profile, program),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap();
    if profile == Profile::Expansion {
        enter_expansion_ram(&mut machine, program);
    }
    machine
}

fn overlay(machine: &mut Emulator, profile: Profile, patches: &[CheatPatch]) {
    assert_eq!(machine.supports_rom_cheats(), profile.factory_fixture());
    if profile.factory_fixture() {
        machine.install_rom_patches(patches).unwrap();
    } else {
        if !patches.is_empty() {
            let before = machine.clone();
            assert!(machine.install_rom_patches(patches).is_err());
            assert_eq!(*machine, before);
        }
        let ordinary =
            crate::cartridge::Cartridge::new(machine.bus.cartridge.bytes.to_vec()).unwrap();
        assert!(ordinary.save.is_none());
        machine.bus.cartridge.rom_overlay = ordinary.prepare_rom_overlay(patches).unwrap();
        assert!(machine.encode_state().is_err());
        assert!(machine.encode_frontend_state().is_err());
        assert!(machine.encode_debugger_checkpoint().is_err());
        assert_eq!(machine.rom_offset_for_cpu_address(CODE), None);
        assert!(machine.check_debug_call(CODE).is_err());
    }
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

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn offsets(input: InstructionInput) -> [Option<u64>; 2] {
    input.origin.map(InstructionByteOrigin::storage_offset)
}

#[derive(Default)]
struct Capture(Vec<(u32, InstructionInput)>);
impl InstructionObserver for Capture {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.0.push((address, input));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn six_special_save_owners_consume_main_bytes_words_tail_and_preserve_atomic_admission() {
    for profile in ALL {
        let tail = profile.size() as u32 - 2;
        let program = [
            0x707f,
            0x727f,
            0x747f,
            0x767f,
            0x787f,
            0x1039,
            1,
            0x8000,
            0x1239,
            1,
            0x8001,
            0x3439,
            1,
            0x8000,
            0x3639,
            (tail >> 16) as u16,
            tail as u16,
            0x1839,
            ((tail + 1) >> 16) as u16,
            (tail + 1) as u16,
            0x60fe,
            0x4e71,
        ];
        for concurrent in [false, true] {
            let mut m = machine(profile, &program, concurrent);
            if profile != Profile::Reserved {
                let bytes = m.dump_save_ram().unwrap();
                m.load_save_ram(&vec![0x77; bytes.len()]).unwrap();
            }
            let source = m.bus.cartridge.bytes.clone();
            let digest = m.bus.cartridge.rom_digest();
            let original = if profile == Profile::Expansion {
                [0; 5]
            } else {
                [0x12, 0x34, 0x1234, 0x9abc, 0xbc]
            };
            for (text, expected) in [
                (None, original),
                (
                    Some(format!("018000:A55A+{tail:06X}:C73D")),
                    [0xa5, 0x5a, 0xa55a, 0xc73d, 0x3d],
                ),
                (
                    Some(format!("018000:96E1+{tail:06X}:4826")),
                    [0x96, 0xe1, 0x96e1, 0x4826, 0x26],
                ),
                (None, original),
            ] {
                let before = m.clone();
                overlay(
                    &mut m,
                    profile,
                    &text.map(|text| patches(&text)).unwrap_or_default(),
                );
                let mut hardware = m.clone();
                hardware.bus.cartridge.rom_overlay = before.bus.cartridge.rom_overlay.clone();
                assert_eq!(hardware, before);
                let battery = m.dump_save_ram();
                m.reset();
                assert_eq!(m.dump_save_ram(), battery);
                if profile == Profile::Expansion {
                    enter_expansion_ram(&mut m, &program);
                }
                for _ in 0..5 {
                    m.step_instruction().unwrap();
                }
                assert_eq!(m.registers().d[..5], [0x7f; 5]);
                let mut capture = Capture::default();
                m.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0[0].1.value, 0x1039);
                assert_eq!(
                    offsets(capture.0[0].1),
                    if profile == Profile::Expansion {
                        [None; 2]
                    } else {
                        [Some(0x20a), Some(0x20b)]
                    }
                );
                for _ in 0..4 {
                    m.step_instruction().unwrap();
                }
                assert_eq!(m.registers().d[..5], expected);
                assert!(Arc::ptr_eq(&source, &m.bus.cartridge.bytes));
                assert_eq!(m.bus.cartridge.rom_digest(), digest);
                assert_eq!(m.supports_rom_cheats(), profile.factory_fixture());
                assert!(m.check_warm_reset().is_err());
                assert!(
                    crate::hardware::bus::Bus::new_owned(m.bus.cartridge.clone(), Region::NtscUs)
                        .is_err()
                );
                if !profile.factory_fixture() {
                    let before = m.clone();
                    assert!(m.encode_state().is_err());
                    assert!(m.encode_frontend_state().is_err());
                    assert!(m.encode_debugger_checkpoint().is_err());
                    assert!(m.check_debug_call(CODE).is_err());
                    assert_eq!(m, before);
                }
                let before = m.clone();
                for bad in [profile.size() as u32, 0xa130f1, u32::MAX] {
                    assert!(
                        m.install_rom_patches(&[patch(DATA, 0xee), patch(bad, 1)])
                            .is_err()
                    );
                    assert_eq!(m, before);
                }
                assert!(
                    m.install_rom_patches(&patches("018000:EE+000007:01"))
                        .is_err()
                );
                assert_eq!(m, before);
            }
            for mutation in 0..6 {
                let mut invalid = m.clone();
                match mutation {
                    0 => invalid.bus.cartridge.save = None,
                    1 => invalid.bus.cartridge.bytes = m.bus.cartridge.bytes.to_vec().into(),
                    2 => Arc::make_mut(&mut invalid.bus.cartridge.bytes)[DATA as usize] ^= 1,
                    3 => {
                        invalid.bus.cartridge.save.as_mut().unwrap().bytes =
                            vec![0; 1].into_boxed_slice()
                    }
                    4 => {
                        invalid.bus.cartridge.save =
                            Some(crate::cartridge::ram::headerless_odd8_fixture())
                    }
                    _ => invalid.bus.cartridge.sf002 = Some(crate::cartridge::sf002::Sf002::new()),
                }
                let ordinary_control = !profile.factory_fixture() && matches!(mutation, 0 | 4);
                assert_eq!(invalid.supports_rom_cheats(), ordinary_control);
                let before = invalid.clone();
                let installed = invalid.install_rom_patches(&[patch(DATA, 0xee)]);
                assert_eq!(installed.is_ok(), ordinary_control);
                if !ordinary_control {
                    assert_eq!(invalid, before);
                }
            }
            if profile == Profile::Reserved {
                assert_eq!(m.dump_save_ram(), None);
                assert!(!m.save_ram_kind().has_ram());
                for len in [0, 1, 8192] {
                    let before = m.clone();
                    assert!(m.load_save_ram(&vec![0; len]).is_err());
                    assert_eq!(m, before);
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn six_special_save_apertures_keep_ram_priority_lanes_holes_and_consumer_refusals() {
    use crate::hardware::sound::{CandidateBus, SoundFault};
    use zeff_z80::Z80Bus;
    for profile in ALL {
        let mut m = machine(profile, &[0x60fe, 0x4e71], true);
        let target = if profile == Profile::Expansion {
            0x400001
        } else {
            0x200001
        };
        if matches!(
            profile,
            Profile::Bytes | Profile::Switched | Profile::Reserved
        ) {
            overlay(&mut m, profile, &patches("200000:C73D"));
        } else {
            overlay(
                &mut m,
                profile,
                &[patch(
                    if profile == Profile::Odd {
                        0x0fffff
                    } else {
                        0x200001
                    },
                    0xee,
                )],
            );
        }
        if profile != Profile::Reserved {
            let len = m.dump_save_ram().unwrap().len();
            m.load_save_ram(&vec![0x77; len]).unwrap();
        }
        if matches!(
            profile,
            Profile::Bytes | Profile::Switched | Profile::Reserved
        ) {
            let modes: &[u16] = if profile == Profile::Reserved {
                &[0]
            } else {
                &[0, 1, 3, 2]
            };
            for &mode in modes {
                let expected = if mode & 1 == 0 {
                    0x3d
                } else if mode == 1 {
                    0x99
                } else {
                    0x77
                };
                let mut candidate = m.clone();
                let mut program = Vec::new();
                if profile != Profile::Reserved {
                    program.extend([0x13fc, mode, 0xa1, 0x30f1]);
                }
                if mode & 1 != 0 {
                    program.extend([0x13fc, 0x99, 0x20, 1]);
                }
                program.extend([0x1039, 0x20, 1]);
                if mode & 1 == 0 {
                    program.extend([0x1239, 0x20, 0, 0x3439, 0x20, 0]);
                } else if profile == Profile::Bytes {
                    program.extend([0x1239, 0x20, 0]);
                }
                program.extend([0x60fe, 0x4e71]);
                write_words(&mut candidate.bus.ram, 0x400, &program);
                candidate.cpu.registers.pc = 0xff0400;
                candidate
                    .cpu
                    .prime(&mut ScheduledBus::new(
                        &mut candidate.bus,
                        &mut candidate.sound_cpu,
                        &mut candidate.sound_schedule,
                    ))
                    .unwrap();
                if profile != Profile::Reserved {
                    candidate.step_instruction().unwrap();
                }
                if mode & 1 != 0 {
                    candidate.step_instruction().unwrap();
                }
                candidate.step_instruction().unwrap();
                assert_eq!(candidate.registers().d[0] as u8, expected);
                if mode & 1 == 0 {
                    candidate.step_instruction().unwrap();
                    candidate.step_instruction().unwrap();
                    assert_eq!(
                        (candidate.registers().d[1], candidate.registers().d[2]),
                        (0xc7, 0xc73d)
                    );
                } else if profile == Profile::Bytes {
                    candidate.step_instruction().unwrap();
                    assert_eq!(candidate.registers().d[1], 0x77);
                }
                let byte = candidate.bus.cartridge.read_sampled(target).unwrap();
                assert_eq!(
                    (byte.value, byte.origin.storage_offset()),
                    (expected, (mode & 1 == 0).then_some(0x200001))
                );
                let before = candidate.bus.clone();
                if mode & 1 != 0 {
                    let result = candidate
                        .bus
                        .read_cycle(access(0x200000, ByteLanes::Both), candidate.bus.cpu_clock);
                    assert!(result.result.is_err());
                    assert_eq!(candidate.bus, before);
                }
                assert_eq!(
                    candidate
                        .bus
                        .cartridge
                        .save
                        .as_ref()
                        .unwrap()
                        .write_protected,
                    mode == 3
                );
            }
        } else if profile != Profile::Reserved {
            let sampled = m.bus.cartridge.read_sampled(target).unwrap();
            assert_eq!(
                (sampled.value, sampled.origin.storage_offset()),
                (0x77, None)
            );
        }
        if profile == Profile::Odd {
            for address in [0x100000, 0x1fffff] {
                let byte = m.bus.cartridge.read_sampled(address).unwrap();
                assert_eq!((byte.value, byte.origin.storage_offset()), (0xff, None));
                assert_eq!(m.rom_offset_for_cpu_address(address), None);
            }
        }
        if profile == Profile::Split {
            let byte = m.bus.cartridge.read_sampled(0x200000).unwrap();
            assert_eq!((byte.value, byte.origin.storage_offset()), (0xff, None));
            assert_eq!(m.rom_offset_for_cpu_address(0x200000), None);
        }
        let forbidden = match profile {
            Profile::Expansion => 0x400001,
            Profile::Split => 0x210001,
            _ => 0x200001,
        };
        for direction in [Direction::Read, Direction::Write] {
            let mut memory = m.bus.sound.clone();
            memory.bank = (forbidden >> 15) as u16;
            let before = memory.clone();
            let local = (0x8000 | (forbidden & 0x7fff)) as u16;
            {
                let mut candidate = CandidateBus::new(&mut memory, &m.bus.cartridge);
                if direction == Direction::Read {
                    candidate.cpu_read(local);
                } else {
                    candidate.cpu_write(local, 0x5a);
                }
                assert_eq!(
                    candidate.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        address: local,
                        direction
                    })
                );
            }
            assert_eq!(memory, before);
        }
        for address in match profile {
            Profile::Odd => vec![0x200000, 0x2012c0, 0x3fffff],
            Profile::Split => vec![0x210000, 0x2fffff],
            Profile::Expansion => vec![0x410000],
            _ => Vec::new(),
        } {
            assert_eq!(m.peek(address), None);
            assert_eq!(m.rom_offset_for_cpu_address(address), None);
        }
    }
}
