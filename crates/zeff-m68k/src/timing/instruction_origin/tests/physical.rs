use super::*;
use crate::timing::physical::{
    EntryBus, EntryOutcome, EntryReport, ExternalResetBoundary, ResetQualification,
};
use crate::timing::physical_instruction::{
    InstructionBus, InstructionOutcome, InstructionReport, PhysicalInstructionStep,
};

impl EntryBus for Tagged {
    type Error = usize;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        let cycle = self.bus.read_cycle(access, start);
        EntryReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(value) => EntryOutcome::Complete(value),
                Err(error) => EntryOutcome::HostFailure(error),
            },
        }
    }

    fn read_entry_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> EntryReport<InstructionInput, usize> {
        let cycle = self.read_input(access, start);
        EntryReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(value) => EntryOutcome::Complete(value),
                Err(error) => EntryOutcome::HostFailure(error),
            },
        }
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        let cycle = self.bus.write_cycle(access, value, start);
        EntryReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(()) => EntryOutcome::Complete(()),
                Err(error) => EntryOutcome::HostFailure(error),
            },
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }
}

impl InstructionBus for Tagged {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        ClockedBus::instruction_input(self, address, input);
    }

    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, usize> {
        let cycle = self.bus.read_cycle(access, start);
        InstructionReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(value) => InstructionOutcome::Complete(value),
                Err(error) => InstructionOutcome::HostFailure(error),
            },
        }
    }

    fn read_instruction_fetch(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<InstructionInput, usize> {
        let cycle = self.read_input(access, start);
        InstructionReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(value) => InstructionOutcome::Complete(value),
                Err(error) => InstructionOutcome::HostFailure(error),
            },
        }
    }

    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), usize> {
        let cycle = self.bus.write_cycle(access, value, start);
        InstructionReport {
            elapsed: 4 + cycle.wait_clocks,
            outcome: match cycle.result {
                Ok(()) => InstructionOutcome::Complete(()),
                Err(error) => InstructionOutcome::HostFailure(error),
            },
        }
    }
}

impl EntryBus for Forward<'_> {
    type Error = usize;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, usize> {
        EntryBus::read_entry(self.0, access, start)
    }

    fn read_entry_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> EntryReport<InstructionInput, usize> {
        EntryBus::read_entry_instruction(self.0, access, start)
    }

    fn write_entry(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> EntryReport<(), usize> {
        EntryBus::write_entry(self.0, access, value, start)
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        EntryBus::idle_entry(self.0, start, clocks);
    }
}

impl InstructionBus for Forward<'_> {
    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        InstructionBus::instruction_input(self.0, address, input);
    }

    fn read_instruction(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<u16, usize> {
        InstructionBus::read_instruction(self.0, access, start)
    }

    fn read_instruction_fetch(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> InstructionReport<InstructionInput, usize> {
        InstructionBus::read_instruction_fetch(self.0, access, start)
    }

    fn write_instruction(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> InstructionReport<(), usize> {
        InstructionBus::write_instruction(self.0, access, value, start)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_register_paths_retain_the_old_input_while_refilling_from_new_source() {
    for opcode in [0x4e71, 0x7007, 0x3200, 0x4a40, 0xb240, 0x40c0, 0x4640] {
        let (mut cpu, mut bus) = seeded(&[opcode, 0x4e71, 0x4e71]);
        let old = cpu.queued_instruction_inputs().unwrap();
        let pc = cpu.registers.pc;
        bus.base += 0x100000;
        assert!(matches!(
            cpu.step_physical_instruction(&mut Forward(&mut bus))
                .unwrap(),
            PhysicalInstructionStep::Instruction(_)
        ));
        assert_eq!(bus.consumed, [(pc, old[0])]);
        let next = cpu.queued_instruction_inputs().unwrap();
        assert_eq!(next[0], old[1]);
        assert_eq!(next[1].origin[0], bus.origin(pc + 4));
        assert_eq!(cpu.clock().0, bus.bus.clock);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_extension_and_operand_paths_publish_the_actual_refilled_inputs() {
    for program in [
        vec![0x3039, 0, 0x1000, 0x4e71, 0x4e71],
        vec![0x33fc, 0x005a, 0, 0x1000, 0x4e71, 0x4e71],
        vec![0x4a79, 0, 0x1000, 0x4e71, 0x4e71],
        vec![0x0c40, 0x005a, 0x4e71, 0x4e71],
        vec![0x46fc, 0x2700, 0x4e71, 0x4e71],
    ] {
        let (mut cpu, mut bus) = seeded(&program);
        bus.bus.words.insert(0x1000, 0x1234);
        let old = cpu.queued_instruction_inputs().unwrap();
        let pc = cpu.registers.pc;
        bus.base += 0x100000;
        assert!(matches!(
            cpu.step_physical_instruction(&mut Forward(&mut bus))
                .unwrap(),
            PhysicalInstructionStep::Instruction(_)
        ));
        assert_eq!(bus.consumed[0], (pc, old[0]));
        let next = cpu.queued_instruction_inputs().unwrap();
        assert_eq!(next[0].origin[0], bus.origin(cpu.registers.pc));
        assert_eq!(next[1].origin[0], bus.origin(cpu.registers.pc + 2));
        assert_eq!(next.map(|word| word.value), [0x4e71, 0x4e71]);
        assert_eq!(cpu.clock().0, bus.bus.clock);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn physical_entry_refills_have_sources_and_failed_instruction_refills_have_no_queue() {
    let (mut cpu, mut bus) = seeded(&[0x4e71, 0x4e71]);
    for (address, word) in [
        (0, 0x00ff),
        (2, 0x1000),
        (4, 0x00ab),
        (6, 0x0400),
        (0xab0400, 0x4e71),
        (0xab0402, 0x7007),
    ] {
        bus.bus.words.insert(address, word);
    }
    bus.base += 0x100000;
    let start = cpu.clock();
    cpu.external_warm_reset(
        &mut Forward(&mut bus),
        ExternalResetBoundary {
            sampled_release: start,
            qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
        },
    )
    .unwrap();
    assert!(bus.consumed.is_empty());
    assert_eq!(cpu.clock().0 - start.0, 40);
    let queued = cpu.queued_instruction_inputs().unwrap();
    assert_eq!(queued[0].origin[0], bus.origin(0xab0400));
    assert_eq!(queued[1].origin[0], bus.origin(0xab0402));
    bus.bus.fail = Some(bus.bus.transfers);
    assert!(
        cpu.step_physical_instruction(&mut Forward(&mut bus))
            .is_err()
    );
    assert_eq!(bus.consumed, [(0xab0400, queued[0])]);
    assert_eq!(cpu.queued_instruction_inputs(), None);
    assert_eq!(cpu.clock().0, bus.bus.clock);
}
