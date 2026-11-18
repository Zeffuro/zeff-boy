use super::*;
use crate::hardware::vdp::{Vdp, VdpDmaSource, VdpInterrupts, VdpRegisterPipeline};
use std::sync::Arc;
use zeff_emu_common::audio_trace::MdTraceSource;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};

fn patch(address: u32, value: u8) -> CheatPatch {
    CheatPatch::WideRomWrite {
        address,
        value: CheatValue::Constant(value),
    }
}

fn machine(region: Region, concurrent_sound: bool) -> Emulator {
    let cart = cartridge();
    let mut machine = Emulator::new(
        cart.bytes.to_vec(),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap();
    machine.bus.cartridge = cart;
    machine
}

fn select(machine: &mut Emulator, mapping: Mapping) {
    let board = machine.bus.cartridge.sf002.as_mut().unwrap();
    *board = Sf002::new();
    if mapping != Mapping::Unknown {
        assert!(board.control(access(0xe01), 0));
    }
    if mapping == Mapping::Mirrored {
        assert!(board.control(access(0xe01), 0x81));
    }
}

fn dma_sample(machine: &Emulator, region: Region, address: u32) -> Option<u16> {
    let mut registers = VdpRegisterPipeline::at_h32_edge(MasterTicks::ZERO);
    for index in 0..registers.registers().len() {
        let value = match index {
            1 => 0x54,
            15 => 2,
            19 => 1,
            21 => (address >> 1) as u8,
            22 => (address >> 9) as u8,
            23 => (address >> 17) as u8,
            _ => 0,
        };
        registers
            .advance_to(MasterTicks::new(registers.now().get() + 1))
            .unwrap();
        let write = registers.queue_register(index as u8, value).unwrap();
        registers.advance_to(write.publish_at).unwrap();
    }
    let mut interrupts = VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false);
    while interrupts
        .advance_to(registers.now(), region, &registers)
        .unwrap()
        .is_some()
    {}
    let mut vdp = Vdp::from_stable_display_state(interrupts, registers, region).unwrap();
    let source = VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram);
    vdp.write_control_from(0x4000, region, source).unwrap();
    vdp.write_control_from(0x0080, region, source).ok()?;
    let release = vdp.external_dma_release(region, source).ok()??;
    vdp.advance_with_source(release, region, source).unwrap();
    Some(vdp.fifo().front().expect("sampled DMA word").data)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_main_peek_dma_and_save_keep_physical_source_and_lane_owners() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let mut m = machine(region, concurrent);
            assert!(m.supports_rom_cheats());
            m.install_rom_patches(&[
                patch(0x796c, 0xa5),
                patch(0x796d, 0x5a),
                patch(0x20796c, 0xc7),
                patch(0x20796d, 0x6b),
                patch(0x1bfffe, 0xe9),
                patch(0x1bffff, 0x8d),
                patch(0x1ffffe, 0x12),
                patch(0x1fffff, 0x34),
                patch(0x21fffe, 0x56),
                patch(0x21ffff, 0x78),
            ])
            .unwrap();
            for (address, value) in [(0x3c0001, 0x42), (0x3c3fff, 0x6d)] {
                m.bus
                    .write_cycle(access(address), value, m.bus.cpu_clock)
                    .result
                    .unwrap();
            }
            for mapping in [Mapping::Unknown, Mapping::Linear, Mapping::Mirrored] {
                select(&mut m, mapping);
                for (address, expected) in [
                    (0x796c, Some(0xa55a)),
                    (0x1ffffe, Some(0x1234)),
                    (
                        0x200000,
                        match mapping {
                            Mapping::Unknown => None,
                            Mapping::Linear => Some(0x8383),
                            Mapping::Mirrored => Some(0xffff),
                        },
                    ),
                    (
                        0x20796c,
                        match mapping {
                            Mapping::Unknown => None,
                            Mapping::Linear => Some(0xc76b),
                            Mapping::Mirrored => Some(0xa55a),
                        },
                    ),
                    (
                        0x21fffe,
                        match mapping {
                            Mapping::Unknown => None,
                            Mapping::Linear => Some(0x5678),
                            Mapping::Mirrored => Some(0x1717),
                        },
                    ),
                    (0x220000, (mapping == Mapping::Mirrored).then_some(0x1717)),
                    (0x3bfffe, (mapping == Mapping::Mirrored).then_some(0xe98d)),
                    (0x3c0000, Some(0xff42)),
                    (0x3c3ffe, Some(0xff6d)),
                    (0x3c4000, None),
                ] {
                    for (target, lanes, value) in [
                        (address, ByteLanes::Both, expected),
                        (address, ByteLanes::Upper, expected.map(|v| v & 0xff00)),
                        (address + 1, ByteLanes::Lower, expected.map(|v| v & 0xff)),
                    ] {
                        let mut transfer = access(target);
                        transfer.lanes = lanes;
                        let result = m.bus.read_cycle(transfer, m.bus.cpu_clock).result;
                        match value {
                            Some(value) => assert_eq!(result, Ok(value)),
                            None => assert!(result.is_err(), "{mapping:?} {target:06x}"),
                        }
                    }
                    assert_eq!(m.peek(address), expected.map(|v| (v >> 8) as u8));
                    assert_eq!(dma_sample(&m, region, address), expected);
                }
                for (address, expected) in [(0x3c0001, 0x42), (0x3c3fff, 0x6d)] {
                    assert_eq!(m.peek(address), Some(expected));
                    assert_eq!(
                        m.bus.read_cycle(access(address), m.bus.cpu_clock).result,
                        Ok(u16::from(expected))
                    );
                }
            }
            let before = m.clone();
            for (mut transfer, value) in [
                (access(0xe00), 0),
                (access(0xa130f1), 3),
                (access(0x3c4001), 0),
            ] {
                for lanes in [transfer.lanes, ByteLanes::Both] {
                    transfer.lanes = lanes;
                    assert!(
                        m.bus
                            .write_cycle(transfer, value, m.bus.cpu_clock)
                            .result
                            .is_err()
                    );
                    assert_eq!(m, before);
                }
            }
            m.clear_rom_patches();
            assert_eq!(m.peek(0x20796c), Some(0x17));
            assert_eq!(m.peek(0x3c0001), Some(0x42));
            assert_eq!(
                m.bus.cartridge.sf002.as_ref().unwrap().mapping(),
                Mapping::Mirrored
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_atomic_install_uses_cold_aperture_and_preserves_identity_and_samples() {
    let mut m = machine(Region::NtscUs, true);
    let source = m.bus.cartridge.bytes.clone();
    let digest = m.bus.cartridge.rom_digest();
    let header = m.bus.cartridge.header.clone();
    let initial = m.bus.cartridge.initial;
    assert!(!Sf002::detect(&source));
    assert!(m.encode_state().is_err());
    assert!(m.encode_frontend_state().is_err());
    let cells = vec![0x42; 0x2000];
    m.load_save_ram(&cells).unwrap();
    select(&mut m, Mapping::Linear);
    let before = m.clone();
    m.install_rom_patches(&[patch(0x796c, 0xa5)]).unwrap();
    assert_eq!(m.cpu, before.cpu);
    assert_eq!(m.bus.cpu_clock, before.bus.cpu_clock);
    assert_eq!(m.bus.now, before.bus.now);
    assert_eq!(m.bus.prefetch, before.bus.prefetch);
    assert_eq!(m.bus.sound, before.bus.sound);
    assert_eq!(m.sound_cpu, before.sound_cpu);
    assert_eq!(m.sound_schedule, before.sound_schedule);
    assert_eq!(m.dump_save_ram(), Some(cells.clone()));
    let before = m.clone();
    for pc in [0x1ffffe_u32, 0x200000, 0x21fffc, 0x200001] {
        let mut codes = vec![patch(0x796c, 0x5a)];
        codes.extend(
            pc.to_be_bytes()
                .into_iter()
                .enumerate()
                .map(|(i, v)| patch(4 + i as u32, v)),
        );
        assert!(m.install_rom_patches(&codes).is_err(), "{pc:08x}");
        assert_eq!(m, before);
    }
    assert!(
        m.install_rom_patches(&[patch(0x796c, 0x5a), patch(0x220000, 1)])
            .is_err()
    );
    assert_eq!(m, before);
    for pc in [0x1ffffc_u32, 0x801ffffc] {
        let codes: Vec<_> = pc
            .to_be_bytes()
            .into_iter()
            .enumerate()
            .map(|(i, v)| patch(4 + i as u32, v))
            .collect();
        m.install_rom_patches(&codes).unwrap();
        m.reset();
        assert_eq!(m.registers().pc, pc);
        assert_eq!(m.queued_words(), Some([0x1717, 0x1717]));
        assert_eq!(
            m.bus.cartridge.sf002.as_ref().unwrap().mapping(),
            Mapping::Unknown
        );
        assert_eq!(m.peek(0x200000), None);
    }
    assert_eq!(m.dump_save_ram(), Some(cells));
    assert!(Arc::ptr_eq(&source, &m.bus.cartridge.bytes));
    assert_eq!(m.bus.cartridge.rom_digest(), digest);
    assert_eq!(m.bus.cartridge.header, header);
    assert_eq!(m.bus.cartridge.initial, initial);
    assert!(!Sf002::detect(&m.bus.cartridge.bytes));
    assert!(m.encode_state().is_err());
    assert!(m.encode_frontend_state().is_err());
    let before = m.clone();
    assert_eq!(m.reset_cpu(), Err(TimingFault::ResetOutputUnsupported));
    assert_eq!(m, before);
    assert!(m.check_warm_reset().is_err());
    m.clear_rom_patches();
    assert_eq!(m.bus.cartridge.effective_initial(), initial);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_trace_keeps_fixed_lower_origins_and_unknown_upper_origins() {
    let mut m = machine(Region::NtscUs, true);
    for mapping in [Mapping::Unknown, Mapping::Linear, Mapping::Mirrored] {
        select(&mut m, mapping);
        assert_eq!(
            crate::hardware::audio::trace::main_origin(0x796c, &m.bus.cartridge).source,
            MdTraceSource::EffectiveRom { offset: 0x796c }
        );
        assert_eq!(
            crate::hardware::audio::trace::sound_origin(0xf96c, 0, &m.bus.cartridge).source,
            MdTraceSource::EffectiveRom { offset: 0x796c }
        );
        for address in [0x200000, 0x20796c, 0x21ffff, 0x220000, 0x3bffff] {
            assert_eq!(
                crate::hardware::audio::trace::main_origin(address, &m.bus.cartridge).source,
                MdTraceSource::Unknown
            );
            assert_eq!(
                crate::hardware::audio::trace::sound_origin(
                    0x8000 | (address & 0x7fff) as u16,
                    (address >> 15) as u16,
                    &m.bus.cartridge
                )
                .source,
                MdTraceSource::Unknown
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_overlay_queued_main_opcode_keeps_sampled_source_across_mapping_change() {
    let mut m = machine(Region::NtscUs, true);
    let words = m.queued_words();
    m.install_rom_patches(&[patch(0x207009, 0x0a), patch(0x7009, 0x0b)])
        .unwrap();
    assert_eq!(m.queued_words(), words);
    m.step_instruction().unwrap();
    m.step_instruction().unwrap();
    m.step_instruction().unwrap();
    assert_eq!(
        m.bus.cartridge.sf002.as_ref().unwrap().mapping(),
        Mapping::Mirrored
    );
    assert_eq!(m.queued_words().unwrap()[0], 0x700a);
    assert_eq!(m.peek(0x207009), Some(0x0b));
    m.step_instruction().unwrap();
    assert_eq!(m.registers().d[0], 10);
    m.step_instruction().unwrap();
    m.step_instruction().unwrap();
    assert_eq!(m.registers().d[0], 11);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf002_queued_upper_main_audio_write_does_not_guess_current_mirror_source() {
    use zeff_emu_common::audio_trace::MdTraceWrite;
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut bytes = cartridge().bytes.to_vec();
            bytes[0x207008..0x20700a].copy_from_slice(&0x1080_u16.to_be_bytes());
            bytes[0x7008..0x700a].copy_from_slice(&0x4e71_u16.to_be_bytes());
            let mut m = Emulator::new(
                bytes,
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            )
            .unwrap();
            assert!(!Sf002::detect(&m.bus.cartridge.bytes));
            m.bus.cartridge.sf002 = Some(Sf002::new());
            m.reset_and_begin_audio_trace(8).unwrap();
            m.cpu.registers.a[0] = 0xc00011;
            m.cpu.registers.d[0] = 0x9f;
            for _ in 0..3 {
                m.step_instruction().unwrap();
            }
            assert_eq!(m.queued_words().unwrap()[0], 0x1080);
            assert_eq!(m.peek(0x207008), Some(0x4e));
            assert_eq!(m.bus.cartridge.rom_source_offset(0x207008), Some(0x7008));
            m.step_instruction().unwrap();
            let capture = m.finish_audio_trace().unwrap();
            capture.trace.validate_complete().unwrap();
            assert_eq!(capture.trace.events.len(), 1);
            match capture.trace.events[0].write {
                MdTraceWrite::Psg { value, origin } => {
                    assert_eq!(value, 0x9f);
                    assert_eq!(origin.boundary_pc, 0x207008);
                    assert_eq!(origin.source, MdTraceSource::Unknown);
                }
                other => panic!("unexpected trace write: {other:?}"),
            }
        }
    }
}
