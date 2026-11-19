use super::*;
use crate::cartridge::{Cartridge, realtec::Realtec};
use crate::hardware::bus::scheduled::ScheduledBus;
use crate::hardware::sound::{CandidateBus, SoundFault};
use crate::hardware::vdp::VdpDmaSource;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;
use zeff_z80::Z80Bus;

mod sound;

fn cartridge(boot: &[u16]) -> Cartridge {
    let mut bytes = vec![0; 0x80000];
    for base in [0, 0x7e000] {
        bytes[base..base + 8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    }
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    write_words(&mut bytes, 0x7e200, boot);
    for offset in [0x1000, 0x7f000] {
        write_words(&mut bytes, offset, &[0x7001, 0x0640, 3, 0x4ef9, 0, 0x1000]);
    }
    write_words(&mut bytes, 0x7fffc, &[0x4e71, 0x4e71]);
    assert!(!Realtec::detect(&bytes));
    let mut cart = Cartridge::new(bytes).unwrap();
    assert!(cart.realtec.is_none());
    cart.realtec = Some(Realtec::new());
    cart
}

fn machine(program: &[u16], region: Region, concurrent_sound: bool) -> Emulator {
    let mut machine = Emulator::new_with_cartridge(
        cartridge(&[0x4ef9, 0x00ff, 0x0200, 0x4e71]),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    write_words(&mut machine.bus.ram, 0x200, program);
    machine.step_instruction().unwrap();
    machine
}

fn select(machine: &mut Emulator, mask: u8, value: u8) {
    let board = machine.bus.cartridge.realtec.as_mut().unwrap();
    board.reset();
    board.write_control(0x402000, mask);
    board.write_control(0x404000, value);
    board.write_control(0x400000, 1);
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
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
fn realtec_overlay_physical_cells_reach_boot_force_bit_aliases_and_all_main_lanes() {
    let addresses = [
        0, 0x1ffe, 0x2000, 0x1fffe, 0x20000, 0x7dffe, 0x7e000, 0x7fffe, 0x80000, 0x3ffffe,
    ];
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = machine(&[0x60fe, 0x4e71], region, concurrent);
            assert!(machine.supports_rom_cheats());
            assert!(machine.dump_save_ram().is_none());
            assert!(
                crate::hardware::bus::Bus::new_owned(machine.bus.cartridge.clone(), region)
                    .is_err()
            );
            for mode in 0..17 {
                machine.clear_rom_patches();
                if mode == 0 {
                    machine.bus.cartridge.realtec.as_mut().unwrap().reset();
                } else {
                    select(&mut machine, (mode - 1) / 4, (mode - 1) % 4);
                }
                let set: Vec<_> = addresses
                    .iter()
                    .flat_map(|&address| {
                        let offset =
                            machine.bus.cartridge.rom_source_offset(address).unwrap() as u32;
                        [patch(offset, 0xa5), patch(offset + 1, 0x5a)]
                    })
                    .collect();
                machine.install_rom_patches(&set).unwrap();
                for address in addresses {
                    assert_eq!(machine.peek(address), Some(0xa5));
                    assert_eq!(machine.bus.cartridge.read(address), Some(0xa5));
                    assert_eq!(read(&mut machine, address, ByteLanes::Both), Ok(0xa55a));
                    assert_eq!(read(&mut machine, address, ByteLanes::Upper), Ok(0xa500));
                    assert_eq!(read(&mut machine, address + 1, ByteLanes::Lower), Ok(0x5a));
                }
                let original = machine.clone();
                for address in [0x100, 0xab000100] {
                    assert!(matches!(
                        read(&mut machine, address, ByteLanes::Both),
                        Err(crate::BusError::UnqualifiedCartridgeReset { .. })
                    ));
                    assert_eq!(machine, original);
                }
                for address in [0x100, 0x101, 0xab000100, 0xab000101, 0x400000] {
                    assert_eq!(machine.rom_offset_for_cpu_address(address), None);
                }
                assert_eq!(
                    machine.rom_offset_for_cpu_address(0x1ffe),
                    machine.bus.cartridge.rom_source_offset(0x1ffe)
                );
                assert_eq!(
                    machine.rom_offset_for_cpu_address(0x2000),
                    machine.bus.cartridge.rom_source_offset(0x2000)
                );
                let mut memory = machine.bus.sound.clone();
                let candidate = CandidateBus::new(&mut memory, &machine.bus.cartridge);
                assert_eq!(candidate.cpu_read(0x8100), 0xff);
                assert!(matches!(
                    candidate.fault.get(),
                    Some(SoundFault::UnsupportedAccess {
                        direction: Direction::Read,
                        ..
                    })
                ));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_cold_vectors_use_boot_source_and_guard_every_prime_byte_atomically() {
    let mut machine = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    for target in [0xfe, 0x100] {
        assert!(matches!(machine.check_debug_call(target),
            Err(crate::GuestCallError::Rejected(reason)) if reason.starts_with("target prefetch")));
    }
    let bytes = machine.bus.cartridge.bytes.clone();
    let digest = machine.bus.cartridge.rom_digest();
    let initial = machine.bus.cartridge.initial;
    let header = machine.bus.cartridge.header.clone();
    select(&mut machine, 3, 2);
    machine
        .install_rom_patches(&patches("000004:0000+000006:0300"))
        .unwrap();
    assert_eq!(machine.bus.cartridge.effective_initial(), initial);
    for pc in [0x1ffe_u32, 0x3ffffc, 0xab001ffe, 0xab3ffffc] {
        let text = format!(
            "07E000:0000+07E002:0000+07E004:{:04X}+07E006:{:04X}",
            pc >> 16,
            pc & 0xffff
        );
        machine.install_rom_patches(&patches(&text)).unwrap();
        assert_eq!(machine.bus.cartridge.effective_initial().pc, pc);
        machine.reset();
        assert_eq!((machine.registers().pc, machine.registers().ssp), (pc, 0));
        assert_eq!(machine.bus.cartridge.rom_source_offset(0), Some(0x7e000));
        assert_eq!(machine.queued_words().unwrap()[0], 0x4e71);
        select(&mut machine, 3, 2);
    }
    let before = machine.clone();
    for pc in [
        0xfe_u32, 0x100, 0xab0000fe, 0xab000100, 0x201, 0x3ffffe, 0x400000,
    ] {
        let text = format!("07E004:{:04X}+07E006:{:04X}", pc >> 16, pc & 0xffff);
        assert!(machine.install_rom_patches(&patches(&text)).is_err());
        assert_eq!(machine, before);
    }
    assert!(machine.install_rom_patches(&[patch(0x80000, 1)]).is_err());
    assert_eq!(machine, before);
    machine.clear_rom_patches();
    machine.reset();
    assert_eq!(machine.registers(), initial);
    assert!(Arc::ptr_eq(&bytes, &machine.bus.cartridge.bytes));
    assert_eq!(machine.bus.cartridge.rom_digest(), digest);
    assert_eq!(machine.bus.cartridge.initial, initial);
    assert_eq!(machine.bus.cartridge.header, header);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_invalid_final_vectors_preserve_trace_and_effective_noops() {
    let mut machine = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    machine.reset_and_begin_audio_trace(8).unwrap();
    let before = machine.clone();
    for set in [
        patches("07F000:A5+07E007:01"),
        patches("07F000:A5+080000:5A"),
    ] {
        assert!(machine.install_rom_patches(&set).is_err());
        assert_eq!(machine, before);
    }
    machine.install_rom_patches(&patches("07F000:70")).unwrap();
    assert_eq!(machine, before);
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        None
    );
    machine.reset_and_begin_audio_trace(8).unwrap();
    machine.install_rom_patches(&patches("07F000:A5")).unwrap();
    let before = machine.clone();
    machine
        .install_rom_patches(&patches("07F000:70+07F000:A5"))
        .unwrap();
    assert_eq!(machine, before);
    assert_eq!(
        machine.finish_audio_trace().unwrap().trace.invalidated,
        Some(AudioTraceInvalidation::ExternalMutation)
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_main_queue_stays_sampled_while_future_fetches_follow_current_set() {
    for concurrent in [false, true] {
        let mut machine = machine(&[0x4ef9, 0, 0x1000, 0x4e71], Region::NtscUs, concurrent);
        machine
            .install_rom_patches(&patches("07F001:07+07F004:4567"))
            .unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(machine.queued_words(), Some([0x7007, 0x0640]));
        let sample = machine.bus.prefetch;
        machine
            .install_rom_patches(&patches("07F001:09+07F004:0005"))
            .unwrap();
        assert_eq!(machine.queued_words(), Some([0x7007, 0x0640]));
        assert_eq!(machine.bus.prefetch, sample);
        machine.step_instruction().unwrap();
        assert_eq!(machine.registers().d[0], 7);
        machine.step_instruction().unwrap();
        assert_eq!(machine.registers().d[0], 12);
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(machine.registers().d[0], 9);
        machine.clear_rom_patches();
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        machine.step_instruction().unwrap();
        assert_eq!(machine.registers().d[0], 1);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_rejected_identity_restore_retains_live_map_queue_and_external_set() {
    let mut machine = machine(&[0x60fe, 0x4e71], Region::NtscUs, true);
    let plain = Emulator::new(
        machine.bus.cartridge.bytes.to_vec(),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    let raw = plain.encode_state().unwrap();
    let front = plain.encode_frontend_state().unwrap();
    machine
        .install_rom_patches(&patches("07F000:A55A"))
        .unwrap();
    select(&mut machine, 2, 2);
    assert!(
        machine
            .encode_state()
            .unwrap_err()
            .to_string()
            .contains("MD Realtec identity mismatch")
    );
    assert!(machine.encode_frontend_state().is_err());
    let before = machine.clone();
    assert!(
        machine
            .decode_state(&raw)
            .unwrap_err()
            .to_string()
            .contains("MD Realtec owner mismatch")
    );
    assert_eq!(machine, before);
    for bad in [
        raw.clone(),
        raw[..raw.len() - 1].to_vec(),
        [raw, vec![0]].concat(),
    ] {
        assert!(machine.decode_state(&bad).is_err());
        assert_eq!(machine, before);
    }
    for bad in [front.as_slice(), &front[..front.len() - 1]] {
        assert!(machine.decode_frontend_state(bad).is_err());
        assert_eq!(machine, before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_overlay_dma_fifo_keeps_sample_and_later_word_uses_current_source() {
    let mut program = Vec::new();
    for value in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9608, 0x9700, 0x4400,
    ] {
        program.extend([0x33fc, value, 0x00c0, 4]);
    }
    program.extend([0x60fe, 0x4e71]);
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut machine = machine(&program, region, true);
        machine
            .install_rom_patches(&patches("07F000:A55A+07F002:B66B"))
            .unwrap();
        for _ in 0..8 {
            machine.step_instruction().unwrap();
        }
        machine
            .bus
            .vdp
            .write_control_from(
                0x0080,
                region,
                VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram),
            )
            .unwrap();
        for _ in 0..2000 {
            if machine
                .vdp()
                .fifo()
                .front()
                .is_some_and(|entry| entry.data == 0xa55a)
            {
                break;
            }
            advance(&mut machine, 1);
        }
        assert_eq!(machine.vdp().fifo().front().unwrap().data, 0xa55a);
        let queue = machine.queued_words();
        machine
            .install_rom_patches(&patches("07F000:C77C+07F002:D88D"))
            .unwrap();
        assert_eq!(machine.vdp().fifo().front().unwrap().data, 0xa55a);
        assert_eq!(machine.queued_words(), queue);
        for _ in 0..4000 {
            if machine.vdp().memory().vram()[0x400..0x404] == [0xa5, 0x5a, 0xd8, 0x8d] {
                break;
            }
            advance(&mut machine, 1);
        }
        assert_eq!(
            &machine.vdp().memory().vram()[0x400..0x404],
            &[0xa5, 0x5a, 0xd8, 0x8d]
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn realtec_queued_psg_origin_stays_unknown_after_mapping_commit() {
    use zeff_emu_common::audio_trace::{MdTraceSource, MdTraceWrite};
    for concurrent_sound in [false, true] {
        let cart = cartridge(&[0x1281, 0x1080, 0x60fe, 0x4e71]);
        let mut machine = Emulator::new_with_cartridge(
            cart,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound,
            },
        )
        .unwrap();
        machine.reset_and_begin_audio_trace(8).unwrap();
        machine.cpu.registers.a[0] = 0xc00011;
        machine.cpu.registers.a[1] = 0x400000;
        machine.cpu.registers.d[0] = 0x9f;
        machine.cpu.registers.d[1] = 1;
        machine.step_instruction().unwrap();
        assert_eq!(machine.queued_words().unwrap()[0], 0x1080);
        assert_eq!(machine.peek(0x202), Some(0));
        machine.step_instruction().unwrap();
        let capture = machine.finish_audio_trace().unwrap();
        capture.trace.validate_complete().unwrap();
        assert_eq!(capture.trace.events.len(), 1);
        match capture.trace.events[0].write {
            MdTraceWrite::Psg { value, origin } => {
                assert_eq!(
                    (value, origin.boundary_pc, origin.source),
                    (0x9f, 0x202, MdTraceSource::Unknown)
                );
            }
            other => panic!("unexpected trace write: {other:?}"),
        }
        let cart = &machine.bus.cartridge;
        assert_eq!(
            crate::hardware::audio::trace::sound_origin(0x8202, 0, cart).source,
            MdTraceSource::Unknown
        );
        assert!(matches!(
            crate::hardware::audio::trace::main_origin(0xff0200, cart).source,
            MdTraceSource::MainRam { .. }
        ));
        assert!(matches!(
            crate::hardware::audio::trace::sound_origin(0x200, 0, cart).source,
            MdTraceSource::SoundRam { .. }
        ));
    }
}
