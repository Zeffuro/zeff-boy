use super::*;

fn reference_step(machine: &mut Emulator) -> Result<MachineStep, MachineFault> {
    machine.begin_main_trace();
    let result = machine.step_main_boundary();
    machine.bus.audio.trace_origin = None;
    if let Err(fault) = &result {
        machine.invalidate_trace_fault(fault);
    }
    result
}

fn compare_step(actual: &mut Emulator, reference: &mut Emulator) {
    assert_eq!(actual.step_instruction(), reference_step(reference));
    assert!(
        actual == reference,
        "full machine differs after public return"
    );
    assert_eq!(
        actual.encode_state().unwrap(),
        reference.encode_state().unwrap()
    );
}

#[test]
fn real_guest_continuations_match_each_return_with_status_stop_and_host_changes() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for stopped in [false, true] {
            let mut actual = cpu_continuation_tests::machine(region, stopped);
            let mut reference = actual.clone();
            for index in 0..1200 {
                if index == 800 || index == 900 {
                    for machine in [&mut actual, &mut reference] {
                        machine.set_apu_sample_generation_enabled(index != 800);
                    }
                }
                compare_step(&mut actual, &mut reference);
            }
            assert_eq!(actual.reset_cpu(), reference.reset_cpu());
            assert!(actual == reference);
            for _ in 0..40 {
                compare_step(&mut actual, &mut reference);
            }
        }
    }
}

#[test]
fn sound_fault_and_failed_main_transfer_preserve_the_successful_prefix() {
    for fault in [false, true] {
        let mut rom = vec![0; 0x200];
        rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
        rom[0x100..0x10c].copy_from_slice(&[
            0x23, 0xc0, 0, 0x80, 0, 0, 0x4e, 0x71, 0x4e, 0x71, 0x4e, 0x71,
        ]);
        let mut actual = Emulator::new(
            rom,
            LoadConfig {
                concurrent_sound: true,
                ..Default::default()
            },
        )
        .unwrap();
        if fault {
            actual.sound_schedule.fault = Some(SoundFault::ClockOverflow);
        }
        let mut reference = actual.clone();
        for _ in 0..4 {
            let result = actual.step_instruction();
            assert!(result.is_err());
            assert_eq!(result, reference_step(&mut reference));
            assert!(actual == reference);
        }
    }
}
