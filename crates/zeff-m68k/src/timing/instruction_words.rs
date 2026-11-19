use super::test_bus::{Bus, cpu};
use super::*;
use crate::{SUPERVISOR, ZERO};

#[derive(Clone, Default)]
struct Observer {
    bus: Bus,
    enabled: bool,
    words: Vec<(u32, u16, usize)>,
    apertures: Vec<InterruptAperture>,
    samples: Option<InterruptSamples>,
}

impl ClockedBus for Observer {
    type Error = usize;

    fn instruction_word(&mut self, address: u32, value: u16) {
        if self.enabled {
            self.words.push((address, value, self.bus.transfers));
        }
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.bus.read_cycle(access, start)
    }

    fn read_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<u16, usize> {
        self.apertures.push(aperture);
        ObservedCycle {
            cycle: self.bus.read_cycle(access, start),
            samples: self.samples,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.bus.write_cycle(access, value, start)
    }

    fn write_cycle_observed(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> ObservedCycle<(), usize> {
        ObservedCycle {
            cycle: self.bus.write_cycle(access, value, start),
            samples: self.samples,
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }

    fn begin_rmw(&mut self, _: BusAccess, _: CpuClocks) -> Result<BusResponse<()>, usize> {
        Ok(BusResponse::Complete(()))
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        self.bus.acknowledge_cycle(access, start)
    }
}

fn seed(opcode: u16, extensions: &[u16]) -> (PrefetchCpu, Bus) {
    let mut timed = cpu(SUPERVISOR);
    let pc = timed.registers.pc;
    timed.pipeline.as_mut().unwrap().words =
        [opcode, extensions.first().copied().unwrap_or(0x7201)];
    for register in 0..7 {
        timed.registers.set_address_register(register, 0x3000);
        timed.registers.d[register] = 0x10;
    }
    let mut bus = Bus::default();
    bus.words.insert(pc & BUS_ADDRESS_MASK, 0x4e71);
    bus.words
        .insert(pc.wrapping_add(2) & BUS_ADDRESS_MASK, 0xffff);
    for (index, &word) in extensions.iter().enumerate().skip(1) {
        bus.words.insert(
            pc.wrapping_add(2 + index as u32 * 2) & BUS_ADDRESS_MASK,
            word,
        );
    }
    (timed, bus)
}

fn compare(timed: PrefetchCpu, bus: Bus, expected: &[u16], success: bool) -> Observer {
    let pc = timed.registers.pc;
    let mut plain = timed.clone();
    let mut plain_bus = Observer {
        bus: bus.clone(),
        ..Observer::default()
    };
    let mut observed = timed;
    let mut observer = Observer {
        bus,
        enabled: true,
        ..Observer::default()
    };
    let result = observed.step_instruction(&mut observer);
    assert_eq!(
        result.is_ok(),
        success,
        "opcode {:04x}: {result:?}",
        expected[0]
    );
    assert_eq!(result, plain.step_instruction(&mut plain_bus));
    assert_eq!(observed, plain);
    assert_eq!(observer.bus, plain_bus.bus);
    assert_eq!(observer.apertures, plain_bus.apertures);
    assert_eq!(
        observer
            .words
            .iter()
            .map(|&(address, value, _)| (address, value))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .enumerate()
            .map(|(index, &word)| (pc.wrapping_add(index as u32 * 2), word))
            .collect::<Vec<_>>()
    );
    observer
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn queued_opcode_is_consumed_once_without_reporting_retained_words() {
    for opcode in [0x4e71, 0x7080, 0x4e58, 0x4e60, 0xd180] {
        let (timed, bus) = seed(opcode, &[]);
        compare(timed, bus, &[opcode], true);
    }
    let (mut timed, bus) = seed(0x4e71, &[]);
    let mut observer = Observer {
        bus,
        enabled: true,
        ..Observer::default()
    };
    timed.step_instruction(&mut observer).unwrap();
    timed.step_instruction(&mut observer).unwrap();
    assert_eq!(
        observer
            .words
            .iter()
            .map(|&(_, word, _)| word)
            .collect::<Vec<_>>(),
        [0x4e71, 0x7201]
    );
    assert_eq!(timed.registers.d[1], 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn branch_displacements_include_word_forms_in_all_condition_paths() {
    for opcode in [0x6002, 0x6000, 0x6602, 0x6600, 0x6102, 0x6100] {
        for status in [SUPERVISOR, SUPERVISOR | ZERO] {
            let (mut timed, bus) = seed(opcode, &[0x10]);
            timed.registers.set_status_register(status);
            let expected = if opcode as u8 == 0 {
                vec![opcode, 0x10]
            } else {
                vec![opcode]
            };
            compare(timed, bus, &expected, true);
        }
    }
    for opcode in [0x50c8, 0x51c8] {
        for counter in [0, 1] {
            let (mut timed, bus) = seed(opcode, &[0x10]);
            timed.registers.d[0] = counter;
            compare(timed, bus, &[opcode, 0x10], true);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn accepted_status_immediates_and_link_report_historical_extensions() {
    for opcode in [
        0x4e72, 0x007c, 0x027c, 0x0a7c, 0x46fc, 0x003c, 0x023c, 0x0a3c, 0x4e50,
    ] {
        let (timed, bus) = seed(opcode, &[0x2000]);
        compare(timed, bus, &[opcode, 0x2000], true);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn denied_status_instructions_do_not_consume_the_immediate() {
    for opcode in [0x4e72, 0x007c, 0x027c, 0x0a7c, 0x46fc] {
        let (mut timed, mut bus) = seed(opcode, &[0x2000]);
        timed.registers.set_status_register(0);
        bus.handler(crate::constants::VECTOR_PRIVILEGE_VIOLATION);
        compare(timed, bus, &[opcode], true);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn jumps_calls_lea_and_pea_report_each_extension_once_for_every_ea() {
    for family in [0x4ec0, 0x4e80, 0x41c0, 0x4840] {
        for mode in [2, 5, 6, 7] {
            for register in 0..if mode == 7 { 4 } else { 8 } {
                let opcode = family | mode << 3 | register;
                let extensions: &[u16] = match (mode, register) {
                    (2, _) => &[],
                    (7, 0) => &[0x3000],
                    (7, 1) => &[0, 0x3000],
                    _ => &[0x10],
                };
                let (timed, bus) = seed(opcode, extensions);
                let expected: Vec<_> = [opcode]
                    .into_iter()
                    .chain(extensions.iter().copied())
                    .collect();
                compare(timed, bus, &expected, true);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn stream_families_report_all_words_including_both_move_eas() {
    for (opcode, extensions) in [
        (0x23fc, vec![0x1234, 0x5678, 0, 0x3000]),
        (0x23f9, vec![0, 0x3100, 0, 0x3000]),
        (0x0080, vec![0x1234, 0x5678]),
        (0x3030, vec![0x10]),
        (0x303a, vec![0x10]),
        (0x0830, vec![3, 0x10]),
        (0x48f9, vec![0xffff, 0, 0x3000]),
        (0x0108, vec![0x10]),
        (0x4af0, vec![0x10]),
        (0x4830, vec![0x10]),
        (0xd188, vec![]),
    ] {
        let (timed, bus) = seed(opcode, &extensions);
        let expected: Vec<_> = [opcode].into_iter().chain(extensions).collect();
        compare(timed, bus, &expected, true);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn special_move_reports_successful_samples_before_the_next_transfer() {
    let (timed, bus) = seed(0x33fc, &[0x64, 0, 0x3000]);
    let observer = compare(timed.clone(), bus.clone(), &[0x33fc, 0x64, 0, 0x3000], true);
    assert_eq!(
        observer
            .words
            .iter()
            .map(|&(_, _, transfer)| transfer)
            .collect::<Vec<_>>(),
        [0, 0, 1, 2]
    );
    let samples = InterruptSamples::new(4, 4);
    let mut plain = timed.clone();
    let mut observed = timed;
    let mut plain_bus = Observer {
        bus: bus.clone(),
        samples,
        ..Observer::default()
    };
    let mut traced_bus = Observer {
        bus,
        samples,
        enabled: true,
        ..Observer::default()
    };
    assert_eq!(
        plain.step_instruction(&mut plain_bus),
        observed.step_instruction(&mut traced_bus)
    );
    assert_eq!(plain, observed);
    assert_eq!(plain_bus.bus, traced_bus.bus);
    assert_eq!(observed.interrupts.observed_parts(), Some((true, 4)));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn failed_extension_fetches_preserve_only_the_selected_prefix() {
    for (opcode, extensions) in [
        (0x203c, vec![0x1234, 0x5678]),
        (0x33fc, vec![0x1234, 0, 0x3000]),
        (0x4ef9, vec![0, 0x3000]),
    ] {
        let (timed, bus) = seed(opcode, &extensions);
        for failed in 0..extensions.len() {
            let mut bus = bus.clone();
            bus.fail = Some(failed);
            let prefix = (failed + 1).min(extensions.len());
            let expected: Vec<_> = [opcode]
                .into_iter()
                .chain(extensions[..prefix].iter().copied())
                .collect();
            compare(timed.clone(), bus, &expected, false);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_queued_extensions_are_reported_before_a_failed_first_transfer() {
    for opcode in [
        0x6000, 0x6600, 0x6100, 0x50c8, 0x51c8, 0x007c, 0x027c, 0x0a7c, 0x46fc, 0x003c, 0x023c,
        0x0a3c, 0x4e50, 0x4ee8,
    ] {
        let (timed, mut bus) = seed(opcode, &[0x10]);
        bus.fail = Some(0);
        let observer = compare(timed, bus, &[opcode, 0x10], false);
        assert_eq!(observer.words[1].2, 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sampled_future_extension_uses_bus_value_and_logical_addresses_wrap() {
    let (mut timed, mut bus) = seed(0x0080, &[0x1234, 0xffff]);
    timed.registers.pc = u32::MAX - 1;
    timed.pipeline.as_mut().unwrap().address = timed.registers.pc;
    bus.read_values.push((0, 0x5678));
    let observer = compare(timed, bus, &[0x0080, 0x1234, 0x5678], true);
    assert_eq!(
        observer
            .words
            .iter()
            .map(|&(address, _, _)| address)
            .collect::<Vec<_>>(),
        [0xffff_fffe, 0, 2]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn callbacks_are_observational_with_default_bus_on_success_and_all_transfer_failures() {
    for (opcode, extensions) in [
        (0x23fc, vec![0x1234, 0x5678, 0, 0x3000]),
        (0xd188, vec![]),
        (0x33fc, vec![0x64, 0, 0x3000]),
    ] {
        let (timed, bus) = seed(opcode, &extensions);
        let mut success_cpu = timed.clone();
        let mut success_bus = bus.clone();
        success_cpu.step_instruction(&mut success_bus).unwrap();
        for failure in (0..success_bus.transfers).map(Some).chain([None]) {
            let mut plain = timed.clone();
            let mut observed = timed.clone();
            let mut plain_bus = bus.clone();
            plain_bus.fail = failure;
            let mut observer = Observer {
                bus: plain_bus.clone(),
                enabled: true,
                ..Observer::default()
            };
            assert_eq!(
                plain.step_instruction(&mut plain_bus),
                observed.step_instruction(&mut observer)
            );
            assert_eq!(plain, observed);
            assert_eq!(plain_bus, observer.bus);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn prime_guards_stop_polls_and_interrupt_service_report_no_instruction_words() {
    let (mut timed, bus) = seed(0x4e72, &[SUPERVISOR]);
    let mut observer = Observer {
        bus,
        enabled: true,
        ..Observer::default()
    };
    timed.prime(&mut observer).unwrap();
    assert!(observer.words.is_empty());
    timed.pipeline.as_mut().unwrap().words = [0x4e72, SUPERVISOR];
    timed.step_instruction(&mut observer).unwrap();
    observer.words.clear();
    assert!(matches!(
        timed.step_boundary(&mut observer),
        Ok(TimedOutcome::Stopped)
    ));
    assert!(observer.words.is_empty());
    let (mut timed, bus) = seed(0x4e71, &[]);
    timed.pipeline = None;
    let mut observer = Observer {
        bus,
        enabled: true,
        ..Observer::default()
    };
    assert!(matches!(
        timed.step_instruction(&mut observer),
        Err(TimingFault::Unprimed)
    ));
    assert!(observer.words.is_empty());
    let (mut timed, mut bus) = seed(0x4e71, &[]);
    bus.handler(27);
    timed.interrupt_input().set_level(3);
    let mut observer = Observer {
        bus,
        enabled: true,
        ..Observer::default()
    };
    assert!(matches!(
        timed.step_boundary(&mut observer),
        Ok(TimedOutcome::Interrupt { .. })
    ));
    assert!(observer.words.is_empty());
}
