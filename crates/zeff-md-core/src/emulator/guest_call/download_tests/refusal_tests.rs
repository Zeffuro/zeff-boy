use super::*;
use crate::ControllerPort;
use crate::cartridge::parallel_adapter::{Model, ParallelAdapter};

fn snapshots(machine: &Emulator) -> [Option<Vec<u8>>; 3] {
    if machine.bus.audio.batch_active() || machine.bus.owned.is_some() {
        [None, None, None]
    } else {
        [
            machine.encode_state().ok(),
            machine.encode_frontend_state().ok(),
            machine.encode_debugger_checkpoint().ok(),
        ]
    }
}

fn rejected(machine: &mut Emulator, target: u32, continue_future: bool) {
    let before = machine.clone();
    let bytes = snapshots(machine);
    let queue = machine.cpu.queued_instruction_inputs();
    let frames = machine.call_stack().copied().collect::<Vec<_>>();
    let status = machine.call_stack_status();
    assert!(machine.check_debug_call(target).is_err());
    assert_eq!(*machine, before);
    assert_eq!(snapshots(machine), bytes);
    assert!(machine.begin_debug_call(target).is_err());
    assert_eq!(*machine, before);
    assert_eq!(snapshots(machine), bytes);
    assert_eq!(machine.cpu.queued_instruction_inputs(), queue);
    assert_eq!(machine.call_stack().copied().collect::<Vec<_>>(), frames);
    assert_eq!(machine.call_stack_status(), status);
    if continue_future {
        assert!(bytes.iter().all(Option::is_some));
        future_field(machine, &mut before.clone());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_fetch_bounds_owners_and_admission_faults_preserve_entire_machine() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let source = super::undo_tests::donor(region, concurrent);
            assert!(source.debug_call_fetch_span(0x3ffffc));
            assert!(source.debug_call_fetch_span(0x1ff1000));
            assert!(source.bus.cartridge.debug_download_fetch_span(0x2ffffe));
            for address in [0x3ffffe, 0x400000, 0x1000000, 0x1050000, 0xfffffe, u32::MAX] {
                assert!(!source.bus.cartridge.debug_download_fetch_span(address));
                assert!(!source.debug_call_fetch_span(address));
            }
            for target in [
                RAM_TARGET + 1,
                0x60001,
                0x1000000,
                0x1ff1000,
                0x400000,
                0x3ffffe,
                0xa00000,
                0xc00010,
                0xa130f1,
            ] {
                rejected(&mut source.clone(), target, true);
            }
            for sp in [0xff0000, 0xff0002, 0xe01004, 0xff1001] {
                let mut actual = source.clone();
                actual.cpu.registers.ssp = sp;
                rejected(&mut actual, RAM_TARGET, false);
            }
            for pc in [0x60009, 0x6000a] {
                let mut actual = source.clone();
                actual.cpu.registers.pc = pc;
                rejected(&mut actual, RAM_TARGET, false);
            }
            let mut alias = machine(image(START, &[0x4ef9, 0xe0, 0x1000]), region, concurrent);
            ram(&mut alias, RAM_TARGET, &[0x60fe, 0x4e71]);
            alias.step_instruction().unwrap();
            assert_eq!(alias.registers().pc, 0xe01000);
            assert!(matches!(
                alias.check_debug_call(RAM_TARGET),
                Err(GuestCallError::Rejected(
                    "return prefetch is outside qualified executable memory"
                ))
            ));
            rejected(&mut alias, RAM_TARGET, false);
            let mut stopped = machine(
                image(START, &[0x4e72, 0x2700, 0x60fe, 0x4e71]),
                region,
                concurrent,
            );
            stopped.step_instruction().unwrap();
            assert!(stopped.main_cpu_stopped());
            rejected(&mut stopped, RAM_TARGET, false);
            for fault in 0..6 {
                let mut actual = source.clone();
                match fault {
                    0 => actual.bus.clock_fault = true,
                    1 => actual.sound_schedule.fault = Some(SoundFault::ClockOverflow),
                    2 => actual.bus.reset_output = true,
                    3 => actual.bus.irq_level = 7,
                    4 => actual.bus.audio.begin_batch(),
                    5 => {
                        let now = actual.bus.now.get();
                        assert!(actual.bus.input.write_serial(2, 0x20, now, region));
                        actual
                            .input_mut()
                            .set_controller_raw(ControllerPort::One, 0xdf);
                        assert!(actual.input_fault().is_some());
                    }
                    _ => unreachable!(),
                }
                rejected(&mut actual, RAM_TARGET, false);
            }
            let mut stripped = source.clone();
            stripped.bus.cartridge.download = None;
            let mut plain = source.rom_bytes().to_vec();
            plain[0x80..0x90].fill(0);
            let mut injected = machine(plain, region, concurrent);
            assert!(injected.bus.cartridge.download.is_none());
            injected.bus.cartridge.download = source.bus.cartridge.download.clone();
            let mut conflicting = source.clone();
            conflicting.bus.cartridge.parallel_adapter = Some(ParallelAdapter::new(Model::First));
            let mut detached = source.clone();
            detached.bus.cartridge.bytes = source.rom_bytes().to_vec().into();
            assert_eq!(detached.rom_bytes(), source.rom_bytes());
            assert!(!Arc::ptr_eq(
                &detached.bus.cartridge.bytes,
                &source.bus.cartridge.bytes
            ));
            let mut geometry = source.clone();
            let mut bytes = source.rom_bytes().to_vec();
            bytes.resize(0x300002, 0);
            geometry.bus.cartridge = Cartridge::new(bytes).unwrap();
            geometry.bus.cartridge.download = source.bus.cartridge.download.clone();
            let mut svp = source.clone();
            svp.bus.cartridge.svp = Some(Box::new(
                crate::svp::InstructionModel::new_synthetic(vec![0; 16], vec![0; 1024]).unwrap(),
            ));
            let mut boot = source.clone();
            boot.bus.tmss = Some(
                crate::hardware::tmss::Tmss::new(source.rom_bytes()[..2048].to_vec()).unwrap(),
            );
            let mut selected = boot.clone();
            selected.bus.tmss.as_mut().unwrap().select_cartridge(true);
            let mut owned = source.clone();
            let precise = super::super::tests::fixture(
                &[0x4e71, 0x4e71],
                0x200,
                0xfffffc,
                region,
                TimingModel::Precise,
                true,
            );
            owned.bus.owned = precise.bus.owned;
            for mut actual in [
                stripped,
                injected,
                conflicting,
                detached,
                geometry,
                svp,
                boot,
                selected,
                owned,
            ] {
                assert!(!actual.debug_call_fetch_span(0x60000));
                rejected(&mut actual, RAM_TARGET, false);
            }
            assert!(
                Emulator::new_with_timing(
                    source.rom_bytes().to_vec(),
                    LoadConfig {
                        region,
                        concurrent_sound: concurrent
                    },
                    TimingModel::Precise,
                )
                .is_err()
            );
        }
    }
}

#[cfg(feature = "first-failure-diagnostic")]
#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_active_instruction_and_field_diagnostics_refuse_purely() {
    for region in REGIONS {
        for concurrent in [false, true] {
            let source = super::undo_tests::donor(region, concurrent);
            let instruction = crate::diagnostic::Capture::begin().unwrap();
            rejected(&mut source.clone(), RAM_TARGET, false);
            drop(instruction);
            let field = crate::diagnostic::field::Capture::begin().unwrap();
            rejected(&mut source.clone(), RAM_TARGET, false);
            drop(field);
            assert_eq!(source.check_debug_call(RAM_TARGET), Ok(()));
        }
    }
}
