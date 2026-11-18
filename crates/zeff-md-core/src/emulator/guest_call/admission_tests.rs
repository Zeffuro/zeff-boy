use super::tests::{PROFILES, REGIONS, fixture, ram};
use super::*;
use zeff_emu_common::audio_trace::AudioTraceTiming;

fn rejected(machine: &mut Emulator, target: u32) {
    let before = machine.clone();
    assert!(machine.check_debug_call(target).is_err());
    assert_eq!(*machine, before);
    assert!(machine.begin_debug_call(target).is_err());
    assert_eq!(*machine, before);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn domain_and_machine_state_refusals_preserve_every_existing_effect() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            let source = fixture(
                &[0x4e71, 0x4e71],
                0x200,
                0xff3000,
                region,
                timing,
                concurrent,
            );
            assert!(source.peek(0xe01000).is_some());
            for target in [0xff1001, 0x01001000, 0x1000, 0xe01000, 0xa00000, 0xc00010] {
                rejected(&mut source.clone(), target);
            }
            for sp in [0xff0000, 0xff0002, 0xe01004] {
                let mut actual = source.clone();
                actual.cpu.registers.ssp = sp;
                rejected(&mut actual, 0xff1000);
            }
            for fault in 0..5 {
                let mut actual = source.clone();
                match fault {
                    0 => actual.bus.clock_fault = true,
                    1 => actual.sound_schedule.fault = Some(SoundFault::ClockOverflow),
                    2 => actual.bus.reset_output = true,
                    3 => actual.bus.irq_level = 7,
                    4 => actual.bus.audio.begin_batch(),
                    _ => unreachable!(),
                }
                rejected(&mut actual, 0xff1000);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn accepted_entry_invalidates_audio_trace_and_clears_main_origin_after_pure_admission() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let mut actual = fixture(
                &[0x4e71, 0x4e71],
                0x200,
                0xff3000,
                region,
                TimingModel::Standard,
                concurrent,
            );
            ram(&mut actual, 0xff1000, &[0x4e75, 0x4e71]);
            actual.bus.audio.trace = actual
                .bus
                .audio
                .trace
                .prepare_with_clock(
                    64,
                    actual.bus.rate(),
                    crate::hardware::audio::trace::trace_chip(region),
                    AudioTraceTiming::BusServiceBoundary,
                )
                .unwrap();
            actual.begin_main_trace();
            assert!(actual.bus.audio.trace_origin.is_some());
            let before = actual.clone();
            assert_eq!(actual.check_debug_call(0xff1000), Ok(()));
            assert_eq!(actual, before);
            rejected(&mut actual, 0xc00010);
            assert!(actual.bus.audio.trace.is_enabled());
            let context = actual.begin_debug_call(0xff1000).unwrap();
            assert!(!actual.bus.audio.trace.is_enabled());
            assert!(actual.bus.audio.trace_origin.is_none());
            assert!(!actual.bus.audio.batch_active());
            actual.step_instruction().unwrap();
            actual.finish_debug_call(context).unwrap();
            let trace = actual.finish_audio_trace().unwrap().trace;
            assert_eq!(
                trace.invalidated,
                Some(AudioTraceInvalidation::ExternalMutation)
            );
            assert!(trace.events.is_empty());
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn return_guards_allow_temporary_odd_sp_and_reject_current_bus_level7() {
    for region in REGIONS {
        for (timing, concurrent) in PROFILES {
            for level in [0, 7] {
                let mut actual = fixture(
                    &[0x4e71, 0x4e71],
                    0x200,
                    0xff3000,
                    region,
                    timing,
                    concurrent,
                );
                ram(&mut actual, 0xff1000, &[0x528f, 0x538f, 0x4e75, 0x4e71]);
                let context = actual.begin_debug_call(0xff1000).unwrap();
                actual.step_instruction().unwrap();
                assert_eq!(actual.registers().stack_pointer() & 1, 1);
                assert!(!actual.debug_call_returned(&context));
                actual.step_instruction().unwrap();
                actual.step_instruction().unwrap();
                assert!(actual.debug_call_returned(&context));
                actual.bus.irq_level = level;
                let before = actual.clone();
                if level == 7 {
                    assert!(!actual.debug_call_returned(&context));
                    assert_eq!(
                        actual.finish_debug_call(context),
                        Err(GuestCallError::Cpu(DebugCallRejection::Level7Interrupt))
                    );
                    assert_eq!(actual, before);
                } else {
                    actual.finish_debug_call(context).unwrap();
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rom_source_owner_resolution_rejects_save_holes_boot_and_unqualified_owners() {
    let mut actual = fixture(
        &[0x4e71, 0x4e71],
        0x200,
        0xff3000,
        Region::NtscUs,
        TimingModel::Standard,
        true,
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x200));
    assert_eq!(actual.rom_offset_for_cpu_address(0x01000200), Some(0x200));
    for address in [0x1000, 0x400000, 0xa00000, 0xc00010, 0xff1000] {
        assert_eq!(actual.rom_offset_for_cpu_address(address), None);
    }
    let mut boot = actual.rom_bytes()[..2048].to_vec();
    boot[..4].copy_from_slice(&0xff3000_u32.to_be_bytes());
    actual.bus.tmss = Some(crate::hardware::tmss::Tmss::new(boot).unwrap());
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), None);
    actual.bus.tmss.as_mut().unwrap().select_cartridge(true);
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x200));
    actual.bus.tmss = None;
    let mut rom = actual.rom_bytes().to_vec();
    rom.resize(0x220000, 0);
    actual.bus.cartridge = Cartridge::new(rom).unwrap();
    actual.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x200));
    assert_eq!(
        actual.bus.cartridge.rom_source_offset(0x200001),
        Some(0x200001)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x200001), None);
    assert!(!actual.debug_call_fetch_span(0x200000));
    actual.bus.cartridge = crate::cartridge::ssf2::tests::cartridge(&[0x4e71, 0x4e71]);
    assert_eq!(actual.rom_offset_for_cpu_address(0x80000), Some(0x80000));
    assert!(
        actual
            .bus
            .cartridge
            .ssf2
            .as_mut()
            .unwrap()
            .control(crate::cartridge::ssf2::tests::access(0xa130f3), 9)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x80000), Some(0x480000));
    let rom = crate::cartridge::sf001::tests::rom(crate::cartridge::sf001::Revision::HiddenSave);
    actual.bus.cartridge = Cartridge::new(rom).unwrap();
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x200));
    assert!(
        actual
            .bus
            .cartridge
            .sf001
            .as_mut()
            .unwrap()
            .control(0xe00, 0x80)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x380200));
    assert_eq!(actual.rom_offset_for_cpu_address(0x3c0000), None);
    assert!(
        actual
            .bus
            .cartridge
            .sf001
            .as_mut()
            .unwrap()
            .control(0xe00, 0x40)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), None);
    actual.bus.cartridge = super::super::sf002_tests::cartridge();
    assert!(actual.encode_state().is_err());
    assert_eq!(actual.rom_offset_for_cpu_address(0x207000), None);
    let control = super::super::sf002_tests::access(0xe01);
    assert!(
        actual
            .bus
            .cartridge
            .sf002
            .as_mut()
            .unwrap()
            .control(control, 0)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x207000), Some(0x207000));
    assert_eq!(actual.rom_offset_for_cpu_address(0x220000), None);
    assert!(
        actual
            .bus
            .cartridge
            .sf002
            .as_mut()
            .unwrap()
            .control(control, 0x81)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x207000), Some(0x7000));
    assert_eq!(actual.rom_offset_for_cpu_address(0x3c0001), None);
    let mut rom = vec![0; 0x200000];
    rom[..8].copy_from_slice(&[0, 0xff, 0x30, 0, 0, 0, 2, 0]);
    rom[0x180..0x18e].copy_from_slice(b"SF-004        ");
    rom[0x1a0..0x1a8].copy_from_slice(&[0, 0, 0, 0, 0, 3, 0xff, 0xff]);
    rom[0x1b0..0x1bc].fill(b' ');
    actual.bus.cartridge = Cartridge::new(rom).unwrap();
    assert_eq!(actual.rom_offset_for_cpu_address(0x40000), Some(0));
    assert!(
        actual
            .bus
            .cartridge
            .sf004
            .as_mut()
            .unwrap()
            .write(0xf01, 0x30)
    );
    assert_eq!(actual.rom_offset_for_cpu_address(0x40000), Some(0xc0000));
    assert_eq!(actual.rom_offset_for_cpu_address(0x200001), None);
    actual.bus.cartridge = crate::cartridge::linear_rom::fixture(&[0x4e71, 0x4e71]);
    assert!(actual.peek(0x200).is_some());
    assert_eq!(actual.rom_offset_for_cpu_address(0x200), Some(0x200));
    assert_eq!(actual.rom_offset_for_cpu_address(0x100000), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn linear_rom_owner_can_enter_ram_before_and_after_a_real_ram_control_transfer() {
    let cartridge = crate::cartridge::linear_rom::fixture(&[0x4ef9, 0xff, 0x2000]);
    let mut actual = Emulator::new(cartridge.bytes.to_vec(), LoadConfig::default()).unwrap();
    actual.bus.cartridge = cartridge;
    ram(&mut actual, 0xff2000, &[0x60fe, 0x4e71]);
    ram(&mut actual, 0xff1000, &[0x4e75, 0x4e71]);
    assert!(actual.encode_state().is_ok());
    assert_eq!(actual.check_debug_call(0xff1000), Ok(()));
    actual.step_instruction().unwrap();
    assert_eq!(actual.registers().pc, 0xff2000);
    assert_eq!(actual.check_debug_call(0xff1000), Ok(()));
    let context = actual.begin_debug_call(0xff1000).unwrap();
    actual.step_instruction().unwrap();
    actual.finish_debug_call(context).unwrap();
}

#[cfg(feature = "first-failure-diagnostic")]
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn active_instruction_and_field_diagnostics_refuse_entry_without_machine_effects() {
    let mut actual = fixture(
        &[0x4e71, 0x4e71],
        0x200,
        0xff3000,
        Region::NtscUs,
        TimingModel::Standard,
        true,
    );
    let instruction = crate::diagnostic::Capture::begin().unwrap();
    rejected(&mut actual, 0xff1000);
    drop(instruction);
    let field = crate::diagnostic::field::Capture::begin().unwrap();
    rejected(&mut actual, 0xff1000);
    drop(field);
    assert_eq!(actual.check_debug_call(0xff1000), Ok(()));
}
