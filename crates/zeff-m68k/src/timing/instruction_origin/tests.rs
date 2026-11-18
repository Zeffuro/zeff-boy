use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, cpu};
use crate::timing::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod physical;

#[derive(Clone)]
struct Tagged {
    bus: Bus,
    base: u64,
    nonstorage: [bool; 2],
    consumed: Vec<(u32, InstructionInput)>,
    legacy: Vec<(u32, u16)>,
    apertures: Vec<InterruptAperture>,
}

impl Default for Tagged {
    fn default() -> Self {
        Self {
            bus: Bus::default(),
            base: 1 << 36,
            nonstorage: [false; 2],
            consumed: Vec::new(),
            legacy: Vec::new(),
            apertures: Vec::new(),
        }
    }
}

impl Tagged {
    fn read_input(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, usize> {
        let high = if self.nonstorage[0] {
            InstructionByteOrigin::NON_STORAGE
        } else {
            InstructionByteOrigin::storage(self.base + u64::from(access.bus_address)).unwrap()
        };
        let low = if self.nonstorage[1] {
            InstructionByteOrigin::NON_STORAGE
        } else {
            InstructionByteOrigin::storage(self.base + u64::from(access.bus_address) + 1).unwrap()
        };
        let cycle = self.bus.read_cycle(access, start);
        CycleResult {
            result: cycle.result.map(|value| InstructionInput {
                value,
                origin: [high, low],
            }),
            wait_clocks: cycle.wait_clocks,
        }
    }

    fn origin(&self, address: u32) -> InstructionByteOrigin {
        InstructionByteOrigin::storage(self.base + u64::from(address & BUS_ADDRESS_MASK)).unwrap()
    }
}

impl ClockedBus for Tagged {
    type Error = usize;

    fn instruction_word(&mut self, address: u32, value: u16) {
        self.legacy.push((address, value));
    }

    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        self.consumed.push((address, input));
        ClockedBus::instruction_word(self, address, input.value);
    }

    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, usize> {
        self.read_input(access, start)
    }

    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<InstructionInput, usize> {
        self.apertures.push(aperture);
        ObservedCycle {
            cycle: self.read_input(access, start),
            samples: InterruptSamples::new(0, 0),
        }
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.bus.read_cycle(access, start)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.bus.write_cycle(access, value, start)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.bus.idle(start, clocks);
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        self.bus.acknowledge_cycle(access, start)
    }
}

struct Forward<'a>(&'a mut Tagged);

impl ClockedBus for Forward<'_> {
    type Error = usize;

    fn instruction_input(&mut self, address: u32, input: InstructionInput) {
        ClockedBus::instruction_input(self.0, address, input);
    }

    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, usize> {
        ClockedBus::read_instruction_cycle(self.0, access, start)
    }

    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        aperture: InterruptAperture,
    ) -> ObservedCycle<InstructionInput, usize> {
        ClockedBus::read_instruction_cycle_observed(self.0, access, start, aperture)
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        ClockedBus::read_cycle(self.0, access, start)
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        ClockedBus::write_cycle(self.0, access, value, start)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        ClockedBus::idle(self.0, start, clocks);
    }

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, usize> {
        ClockedBus::acknowledge_cycle(self.0, access, start)
    }
}

fn seeded(words: &[u16]) -> (PrefetchCpu, Tagged) {
    let mut cpu = cpu(SUPERVISOR);
    let mut bus = Tagged::default();
    for (index, &word) in words.iter().enumerate() {
        bus.bus.words.insert(
            (cpu.registers.pc + index as u32 * 2) & BUS_ADDRESS_MASK,
            word,
        );
    }
    cpu.prime(&mut bus).unwrap();
    (cpu, bus)
}

fn encoded(cpu: &PrefetchCpu) -> Vec<u8> {
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    writer.into_bytes()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn compact_origin_distinguishes_unknown_nonstorage_and_checked_storage_offsets() {
    assert_eq!(std::mem::size_of::<InstructionByteOrigin>(), 8);
    assert_ne!(
        InstructionByteOrigin::UNKNOWN,
        InstructionByteOrigin::NON_STORAGE
    );
    assert_eq!(InstructionByteOrigin::UNKNOWN.storage_offset(), None);
    assert_eq!(InstructionByteOrigin::NON_STORAGE.storage_offset(), None);
    for offset in [0, 1, 1 << 36, InstructionByteOrigin::MAX_STORAGE_OFFSET] {
        let origin = InstructionByteOrigin::storage(offset).unwrap();
        assert_eq!(origin.storage_offset(), Some(offset));
        assert_ne!(origin, InstructionByteOrigin::UNKNOWN);
        assert_ne!(origin, InstructionByteOrigin::NON_STORAGE);
    }
    assert_eq!(InstructionByteOrigin::storage(u64::MAX - 1), None);
    assert_eq!(InstructionByteOrigin::storage(u64::MAX), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn queued_equal_words_keep_old_per_byte_sources_and_new_refills_use_new_source() {
    let (mut cpu, mut bus) = seeded(&[0x4e71; 6]);
    let pc = cpu.registers.pc;
    let old = cpu.queued_instruction_inputs().unwrap();
    assert_eq!(old[0].origin[0], bus.origin(pc));
    bus.base += 0x100000;
    bus.nonstorage = [false, true];
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    assert_eq!(bus.consumed, [(pc, old[0])]);
    let next = cpu.queued_instruction_inputs().unwrap();
    assert_eq!(next[0], old[1]);
    assert_eq!(
        next[1].origin,
        [bus.origin(pc + 4), InstructionByteOrigin::NON_STORAGE]
    );
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    assert_eq!(bus.consumed[1], (pc + 2, old[1]));
    assert_eq!(bus.consumed[2], (pc + 4, next[1]));
    assert!(bus.consumed.iter().all(|(_, input)| input.value == 0x4e71));
    assert_eq!(
        bus.legacy,
        bus.consumed
            .iter()
            .map(|(pc, word)| (*pc, word.value))
            .collect::<Vec<_>>()
    );
    assert!(bus.consumed.iter().all(|(pc, _)| *pc > BUS_ADDRESS_MASK));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn extension_stream_and_observed_jump_keep_consumed_inputs_and_aperture_timing() {
    let (mut cpu, mut bus) = seeded(&[0x4ef9, 0x00ab, 0x0400]);
    bus.bus.words.insert(0xab0400, 0x7007);
    bus.bus.words.insert(0xab0402, 0x4e71);
    let old = cpu.queued_instruction_inputs().unwrap();
    let pc = cpu.registers.pc;
    bus.base += 0x100000;
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    assert_eq!(cpu.registers.pc, 0xab0400);
    assert_eq!(bus.consumed[0], (pc, old[0]));
    assert_eq!(bus.consumed[1], (pc + 2, old[1]));
    assert_eq!(bus.consumed[2].1.origin[0], bus.origin(pc + 4));
    assert_eq!(bus.apertures, [InterruptAperture::FollowingPrefetch]);
    assert_eq!(
        cpu.queued_instruction_inputs().unwrap()[0].origin[0],
        bus.origin(0xab0400)
    );
    assert_eq!(cpu.clock().0, bus.bus.clock);
    for opcode in [0x41f9, 0x4879] {
        for wait in [0, 3] {
            let (mut cpu, mut bus) = seeded(&[opcode, 0x00ab, 0x0400, 0x7007, 0x4e71]);
            let pc = cpu.registers.pc;
            let old = cpu.queued_instruction_inputs().unwrap();
            let stack = cpu.registers.ssp;
            let a0 = cpu.registers.address_register(0).unwrap();
            bus.base += 0x100000;
            bus.bus.wait = wait;
            let mut plain = cpu.clone();
            let mut control = bus.bus.clone();
            let step = cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
            assert_eq!(step, plain.step_instruction(&mut control).unwrap());
            assert_eq!(cpu, plain);
            assert_eq!(bus.bus, control);
            assert_eq!(cpu.registers.pc, pc + 6);
            assert_eq!(
                bus.consumed,
                [
                    (pc, old[0]),
                    (pc + 2, old[1]),
                    (
                        pc + 4,
                        InstructionInput {
                            value: 0x0400,
                            origin: [bus.origin(pc + 4), bus.origin(pc + 5)],
                        }
                    ),
                ]
            );
            assert_eq!(
                bus.legacy,
                [(pc, opcode), (pc + 2, 0x00ab), (pc + 4, 0x0400)]
            );
            assert!(bus.apertures.is_empty());
            assert_eq!(
                cpu.queued_instruction_inputs().unwrap(),
                [
                    InstructionInput {
                        value: 0x7007,
                        origin: [bus.origin(pc + 6), bus.origin(pc + 7)],
                    },
                    InstructionInput {
                        value: 0x4e71,
                        origin: [bus.origin(pc + 8), bus.origin(pc + 9)],
                    },
                ]
            );
            if opcode == 0x41f9 {
                assert_eq!(cpu.registers.address_register(0), Some(0xab0400));
                assert_eq!(cpu.registers.ssp, stack);
                assert_eq!(step.clocks, 12 + u64::from(wait) * 3);
            } else {
                assert_eq!(cpu.registers.address_register(0), Some(a0));
                assert_eq!(cpu.registers.ssp, stack - 4);
                assert_eq!(bus.bus.words[&(stack - 4)], 0x00ab);
                assert_eq!(bus.bus.words[&(stack - 2)], 0x0400);
                assert_eq!(step.clocks, 20 + u64::from(wait) * 5);
            }
            assert_eq!(cpu.clock().0, bus.bus.clock);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn failed_second_refill_keeps_old_queue_and_reports_only_real_consumed_input() {
    for wait in [0, 3] {
        let (mut cpu, mut bus) = seeded(&[0x6002, 0x4e71, 0x4e71, 0x4e71]);
        let before = cpu.queued_instruction_inputs();
        let mut plain = cpu.clone();
        let mut control = bus.bus.clone();
        bus.base += 0x100000;
        bus.bus.wait = wait;
        bus.bus.fail = Some(bus.bus.transfers + 1);
        control.wait = wait;
        control.fail = bus.bus.fail;
        let actual = cpu.step_instruction(&mut Forward(&mut bus));
        assert!(actual.is_err());
        assert_eq!(actual, plain.step_instruction(&mut control));
        assert_eq!(cpu, plain);
        assert_eq!(bus.bus, control);
        assert_eq!(cpu.queued_instruction_inputs(), before);
        assert_eq!(bus.consumed.len(), 1);
        assert_eq!(bus.consumed[0].1, before.unwrap()[0]);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn provenance_changes_neither_hardware_equality_nor_codec_and_import_is_unknown() {
    let (cpu, mut bus) = seeded(&[0x4e71, 0x7007, 0x4e71]);
    let mut unknown =
        PrefetchCpu::from_prefetched(cpu.registers, cpu.queued_words().unwrap(), cpu.clock())
            .unwrap();
    assert_eq!(cpu, unknown);
    assert_ne!(
        cpu.queued_instruction_inputs(),
        unknown.queued_instruction_inputs()
    );
    let bytes = encoded(&cpu);
    assert_eq!(bytes, encoded(&unknown));
    let before = cpu.queued_instruction_inputs();
    let mut receiver = cpu.clone();
    assert!(
        receiver
            .read_boundary_state(&mut StateReader::new(&bytes[..bytes.len() - 1]))
            .is_err()
    );
    assert_eq!(receiver.queued_instruction_inputs(), before);
    receiver
        .read_boundary_state(&mut StateReader::new(&bytes))
        .unwrap();
    assert_eq!(cpu, receiver);
    assert!(
        receiver
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|word| word.origin == [InstructionByteOrigin::UNKNOWN; 2])
    );
    assert_eq!(encoded(&receiver), bytes);
    bus.bus.clock = unknown.clock().0;
    unknown.prime(&mut bus.bus).unwrap();
    assert!(
        unknown
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|word| word.origin == [InstructionByteOrigin::UNKNOWN; 2])
    );
    bus.nonstorage = [true; 2];
    unknown.prime(&mut Forward(&mut bus)).unwrap();
    assert!(
        unknown
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|word| word.origin == [InstructionByteOrigin::NON_STORAGE; 2])
    );
    receiver
        .read_boundary_state(&mut StateReader::new(&encoded(&unknown)))
        .unwrap();
    assert_eq!(receiver, unknown);
    assert!(
        receiver
            .queued_instruction_inputs()
            .unwrap()
            .iter()
            .all(|word| word.origin == [InstructionByteOrigin::UNKNOWN; 2])
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn guest_call_and_real_rts_keep_actual_return_refill_instead_of_old_metadata() {
    let (mut cpu, mut bus) = seeded(&[0x4e71, 0x4e71, 0x4e71]);
    let original = cpu.queued_instruction_inputs().unwrap();
    let pc = cpu.registers.pc;
    bus.bus.words.insert(0x400, 0x7007);
    bus.bus.words.insert(0x402, 0x4e75);
    bus.base += 0x100000;
    let context = cpu.begin_debug_call(&mut Forward(&mut bus), 0x400).unwrap();
    assert_eq!(
        cpu.queued_instruction_inputs().unwrap()[0].origin[0],
        bus.origin(0x400)
    );
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    let callee = cpu.queued_instruction_inputs().unwrap()[0];
    bus.base += 0x100000;
    cpu.step_instruction(&mut Forward(&mut bus)).unwrap();
    assert!(cpu.debug_call_returned(&context));
    cpu.finish_debug_call(context).unwrap();
    assert_eq!(cpu.registers.pc, pc);
    assert_eq!(cpu.registers.d[0], 7);
    let returned = cpu.queued_instruction_inputs().unwrap();
    assert_eq!(returned[0].origin[0], bus.origin(pc));
    assert_ne!(returned[0].origin, original[0].origin);
    assert!(bus.consumed.iter().any(|(_, input)| *input == callee));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn interrupt_refill_keeps_sampled_sources_without_reporting_an_unexecuted_opcode() {
    let (mut cpu, mut bus) = seeded(&[0x4e71, 0x4e71]);
    bus.bus.handler(27);
    cpu.interrupt_input().set_level(3);
    bus.base += 0x100000;
    let outcome = cpu.step_boundary(&mut Forward(&mut bus)).unwrap();
    assert!(matches!(
        outcome,
        TimedOutcome::Interrupt {
            level: 3,
            vector: 27,
            ..
        }
    ));
    assert!(bus.consumed.is_empty() && bus.legacy.is_empty());
    assert_eq!(bus.apertures, [InterruptAperture::HandlerRefill]);
    let queued = cpu.queued_instruction_inputs().unwrap();
    assert_eq!(queued[0].origin[0], bus.origin(0x12345000));
    assert_eq!(queued[1].origin[0], bus.origin(0x12345002));
    assert_eq!(cpu.registers.pc, 0x12345000);
    cpu.step_boundary(&mut Forward(&mut bus)).unwrap();
    assert_eq!(bus.consumed[0], (0x12345000, queued[0]));
    assert_eq!(cpu.clock().0, bus.bus.clock);
}
