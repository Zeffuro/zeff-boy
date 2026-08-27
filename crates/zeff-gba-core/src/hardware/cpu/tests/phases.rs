use super::*;

const CODE: u32 = 0x0300_0000;

fn cpu_at_iwram(bus: &mut Bus, words: &[u32]) -> Cpu {
    for (index, word) in words.iter().copied().enumerate() {
        bus.write32(CODE + (index as u32) * 4, word);
    }
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.set_pc(CODE);
    cpu
}

fn advance_through_sequential_fetch(cpu: &mut Cpu, bus: &mut Bus) {
    let start_cycles = cpu.cycles;
    let instruction_pc = cpu.pc();
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
    assert_eq!(cpu.step_cpu_phase_for_test(bus), None);
    assert_eq!(
        cpu.execution_state().phase,
        CpuExecutionPhase::SequentialFetch
    );
    assert_eq!(cpu.cycles, start_cycles);
    assert_eq!(cpu.pc(), instruction_pc);

    assert_eq!(cpu.step_cpu_phase_for_test(bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::Execute);
    assert_eq!(cpu.cycles, start_cycles + 1);
    assert_eq!(cpu.pc(), instruction_pc + 4);
    let pipeline = cpu.pipeline_state();
    assert_eq!(pipeline.len, 2);
    assert_eq!(pipeline.entries[0].pc, instruction_pc + 4);
    assert_eq!(pipeline.entries[1].pc, instruction_pc + 8);
}

#[test]
fn alu_and_condition_failed_instructions_finish_at_a_boundary() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE3A0_0001, 0x03A0_1002, 0xE1A0_0000]);

    advance_through_sequential_fetch(&mut cpu, &mut bus);
    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("ALU instruction completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.regs[0], 1);
    assert_eq!(cpu.cycles, 1);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());

    advance_through_sequential_fetch(&mut cpu, &mut bus);
    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("condition-failed instruction completion");
    assert_eq!(fetched.pc, CODE + 4);
    assert_eq!(cpu.regs[1], 0);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
}

#[test]
fn branch_refill_owns_one_nonsequential_and_one_sequential_fetch() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(
        &mut bus,
        &[
            0xEA00_0002,
            0xE1A0_0000,
            0xE1A0_0000,
            0xE1A0_0000,
            0xE3A0_0007,
        ],
    );
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(
        cpu.execution_state().phase,
        CpuExecutionPhase::RefillNonSequential
    );
    assert_eq!(cpu.pc(), CODE + 0x10);
    assert_eq!(cpu.cycles, 1);
    assert_eq!(cpu.pipeline_state().len, 0);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(
        cpu.execution_state().phase,
        CpuExecutionPhase::RefillSequential
    );
    assert_eq!(cpu.cycles, 2);
    let pipeline = cpu.pipeline_state();
    assert_eq!(pipeline.len, 1);
    assert_eq!(pipeline.entries[0].pc, CODE + 0x10);

    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("branch completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
    let pipeline = cpu.pipeline_state();
    assert_eq!(pipeline.len, 2);
    assert_eq!(pipeline.entries[0].pc, CODE + 0x10);
    assert_eq!(pipeline.entries[1].pc, CODE + 0x14);
}

#[test]
fn load_pc_refill_preserves_the_retained_instruction_total() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE590_F000, 0xE1A0_0000, 0xE1A0_0000]);
    let target = CODE + 0x40;
    bus.write32(CODE + 0x100, target);
    bus.write32(target, 0xE3A0_2009);
    bus.write32(target + 4, 0xE1A0_0000);
    cpu.regs[0] = CODE + 0x100;
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::DataBus);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::LoadInternal);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::Writeback);
    assert_eq!(cpu.pc(), target);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(
        cpu.execution_state().phase,
        CpuExecutionPhase::RefillNonSequential
    );
    assert_eq!(cpu.pc(), target);
    assert_eq!(cpu.cycles, 3);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("load-PC completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.cycles, 5);
    let pipeline = cpu.pipeline_state();
    assert_eq!(pipeline.entries[0].pc, target);
    assert_eq!(pipeline.entries[1].pc, target + 4);
}

#[test]
fn external_bios_swi_refill_is_phase_owned() {
    let mut rom = vec![0; 0xC0];
    rom[0xA0..0xA4].copy_from_slice(b"TEST");
    rom[0xB2] = 0x96;
    let bios = vec![0; crate::hardware::constants::BIOS_SIZE];
    let cartridge = Cartridge::load(&rom).unwrap();
    let mut bus = Bus::new_with_bios(cartridge, 48_000, &bios).unwrap();
    let mut cpu = cpu_at_iwram(&mut bus, &[0xEF00_0006, 0xE1A0_0000, 0xE1A0_0000]);
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(
        cpu.execution_state().phase,
        CpuExecutionPhase::RefillNonSequential
    );
    assert_eq!(cpu.pc(), 8);
    assert_eq!(cpu.cycles, 2);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("SWI completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.cycles, 4);
    let pipeline = cpu.pipeline_state();
    assert_eq!(pipeline.entries[0].pc, 8);
    assert_eq!(pipeline.entries[1].pc, 12);
}

#[test]
fn arm_single_load_owns_data_and_internal_phases() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE590_1000, 0xE1A0_0000, 0xE1A0_0000]);
    bus.write32(CODE + 0x100, 0x4433_2211);
    cpu.regs[0] = CODE + 0x101;
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let pending = cpu.execution_state();
    assert_eq!(pending.phase, CpuExecutionPhase::DataBus);
    assert_eq!(pending.bus_operation, CpuBusOperation::Read);
    assert_eq!(pending.bus_address, CODE + 0x101);
    assert_eq!(pending.bus_width, 4);
    assert_eq!(pending.data_access_count, 0);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let loaded = cpu.execution_state();
    assert_eq!(loaded.phase, CpuExecutionPhase::LoadInternal);
    assert_eq!(loaded.data_access_count, 1);
    assert_eq!(cpu.regs[1], 0);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::Writeback);
    assert_eq!(cpu.regs[1], 0x1144_3322);
    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("single-load completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
}

#[test]
fn condition_failed_transfer_never_enters_a_data_phase() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0x0590_1000, 0xE1A0_0000, 0xE1A0_0000]);
    cpu.cpsr &= !CPSR_ZERO;
    cpu.regs[0] = CODE + 0x100;
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    let fetched = cpu
        .step_cpu_phase_for_test(&mut bus)
        .flatten()
        .expect("condition-failed transfer completion");
    assert_eq!(fetched.pc, CODE);
    assert_eq!(cpu.regs[1], 0);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
    assert_eq!(cpu.data_access_cycles(), 0);
}

#[test]
fn arm_block_store_advances_before_the_next_save_boundary() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE8A2_0005, 0xE1A0_0000, 0xE1A0_0000]);
    cpu.regs[0] = 0x1111_2222;
    cpu.regs[2] = 0x0200_0000;
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let first = cpu.execution_state();
    assert_eq!(first.phase, CpuExecutionPhase::DataBus);
    assert!(!first.bus_sequential);
    assert_eq!(first.transfer_next_register, 0);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    let second = cpu.execution_state();
    assert_eq!(second.phase, CpuExecutionPhase::DataBus);
    assert!(second.bus_sequential);
    assert_eq!(second.bus_address, 0x0200_0004);
    assert_eq!(second.transfer_next_register, 2);
    assert_eq!(second.transfer_register_mask, 1 << 2);
    assert_eq!(bus.read32(0x0200_0000), 0x1111_2222);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::Writeback);
    assert_eq!(bus.read32(0x0200_0004), 0x0200_0008);
    let _ = cpu.step_cpu_phase_for_test(&mut bus);
    assert_eq!(cpu.regs[2], 0x0200_0008);
}

#[test]
fn arm_swap_uses_read_internal_write_and_writeback_phases() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE100_1092, 0xE1A0_0000, 0xE1A0_0000]);
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[2] = 0xAABB_CCDD;
    bus.write32(0x0200_0000, 0x1122_3344);
    advance_through_sequential_fetch(&mut cpu, &mut bus);

    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::DataBus);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::LoadInternal);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::DataBus);
    assert_eq!(cpu.execution_state().bus_operation, CpuBusOperation::Write);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::Writeback);
    assert_eq!(bus.read32(0x0200_0000), 0xAABB_CCDD);
    let _ = cpu.step_cpu_phase_for_test(&mut bus);
    assert_eq!(cpu.regs[1], 0x1122_3344);
}

#[test]
fn pending_irq_cannot_preempt_a_data_transfer_phase() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = cpu_at_iwram(&mut bus, &[0xE590_1000, 0xE1A0_0000, 0xE1A0_0000]);
    cpu.regs[0] = 0x0200_0000;
    cpu.cpsr &= !CPSR_IRQ_DISABLE;
    advance_through_sequential_fetch(&mut cpu, &mut bus);
    assert_eq!(cpu.step_cpu_phase_for_test(&mut bus), None);
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::DataBus);

    assert!(!cpu.try_service_irq(&mut bus, true));
    assert_eq!(cpu.execution_state().phase, CpuExecutionPhase::DataBus);
    assert_eq!(cpu.pc(), CODE + 4);
}

#[test]
fn hle_data_helpers_do_not_create_a_persistent_transfer() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    bus.write32(0x0200_0000, 0x1122_3344);

    assert_eq!(cpu.cpu_read32(&mut bus, 0x0200_0000), 0x1122_3344);
    cpu.cpu_write16(&mut bus, 0x0200_0004, 0xBEEF);
    assert_eq!(cpu.execution_state(), CpuExecutionState::default());
}
