use super::*;
use crate::bus::BusAccess;
use crate::timing::{ClockedBus, CpuClocks, CycleResult};

struct Tagged(crate::timing::test_bus::Bus);

impl ClockedBus for Tagged {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        self.0.read_cycle(access, start)
    }
    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, usize> {
        let cycle = self.0.read_cycle(access, start);
        CycleResult {
            result: cycle.result.map(|value| InstructionInput {
                value,
                origin: [
                    InstructionByteOrigin::storage(17).unwrap(),
                    InstructionByteOrigin::NON_STORAGE,
                ],
            }),
            wait_clocks: cycle.wait_clocks,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        self.0.write_cycle(access, value, start)
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        self.0.idle(start, clocks);
    }
}

fn tagged() -> PrefetchCpu {
    let mut cpu = crate::timing::test_bus::cpu(crate::SUPERVISOR);
    let mut bus = Tagged(Default::default());
    bus.0.words.insert(cpu.registers.pc & 0xffffff, 0x4e71);
    bus.0
        .words
        .insert((cpu.registers.pc + 2) & 0xffffff, 0x7007);
    cpu.prime(&mut bus).unwrap();
    cpu
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cpu_owned_capsule_roundtrip_binds_real_queue_and_full_boundary_without_codec_changes() {
    let cpu = tagged();
    let checkpoint = cpu.capture_instruction_origins(64).unwrap();
    let mut writer = StateWriter::new();
    cpu.write_boundary_state(&mut writer).unwrap();
    let raw = writer.into_bytes();
    let mut imported = PrefetchCpu::default();
    imported
        .read_boundary_state(&mut StateReader::new(&raw))
        .unwrap();
    assert_ne!(
        imported.queued_instruction_inputs(),
        cpu.queued_instruction_inputs()
    );
    let mut writer = StateWriter::new();
    checkpoint.write_owned(&mut writer);
    let bytes = writer.into_bytes();
    let checkpoint =
        InstructionOriginCheckpoint::read_owned(&mut StateReader::new(&bytes), 64).unwrap();
    imported
        .attach_instruction_origins(&checkpoint, 64)
        .unwrap();
    assert_eq!(
        imported.queued_instruction_inputs(),
        cpu.queued_instruction_inputs()
    );
    assert_eq!(boundary(&imported).unwrap(), raw);
    let empty = PrefetchCpu::default();
    let capsule = empty.capture_instruction_origins(0).unwrap();
    let mut receiver = empty.clone();
    receiver.attach_instruction_origins(&capsule, 0).unwrap();
    assert_eq!(receiver.queued_instruction_inputs(), None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cpu_owned_capsule_rejects_queue_clock_register_presence_and_source_bounds_atomically() {
    let cpu = tagged();
    let checkpoint = cpu.capture_instruction_origins(64).unwrap();
    for variant in 0..5 {
        let mut receiver = cpu.clone();
        match variant {
            0 => receiver.registers.d[0] ^= 1,
            1 => receiver.clock.0 += 4,
            2 => {
                let mut pipeline = receiver.pipeline.unwrap();
                pipeline.words[0] ^= 1;
                receiver.pipeline = Some(pipeline);
            }
            3 => receiver.pipeline = None,
            _ => receiver.registers.pc += 2,
        }
        let before = receiver.clone();
        let tags = receiver.queued_instruction_inputs();
        assert!(
            receiver
                .attach_instruction_origins(&checkpoint, 64)
                .is_err()
        );
        assert_eq!(receiver, before);
        assert_eq!(receiver.queued_instruction_inputs(), tags);
    }
    let mut receiver = cpu.clone();
    assert!(
        receiver
            .attach_instruction_origins(&checkpoint, 17)
            .is_err()
    );
    assert_eq!(
        receiver.queued_instruction_inputs(),
        cpu.queued_instruction_inputs()
    );
    assert!(cpu.capture_instruction_origins(17).is_err());
    let mut writer = StateWriter::new();
    checkpoint.write_owned(&mut writer);
    let mut bytes = writer.into_bytes();
    let end = bytes.len();
    bytes[end - 8..].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(InstructionOriginCheckpoint::read_owned(&mut StateReader::new(&bytes), 64).is_err());
}
