use super::*;
use crate::hardware::timing::{
    DataAccessOrigin, TimerIoAccessKind, TimerIoAccessWidth, TimerIoCompletionEvent,
    TimerIoRegister,
};

#[test]
fn arm_block_store_records_timer_word_lanes_at_transaction_completion() {
    let mut bus = bus_with_rom(&0xE8A0_0006u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[0] = 0x0400_00FC;
    cpu.regs[1] = 0x1122_3344;
    cpu.regs[2] = 0x0080_ABCD;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    assert_eq!(timeline.fetch_cycles, 8);
    assert_eq!(timeline.total_cycles, 10);
    assert_eq!(timeline.data_access_count, 2);
    assert_eq!(timeline.replaced_legacy_data_cycles, 2);
    assert_eq!(timeline.incremental_non_data_cycles, 0);
    assert_eq!(cpu.data_access_cycles(), 2);
    assert_eq!(timeline.required_cycles, 10);
    assert_eq!(
        cpu.timer_io_completion_events(),
        [
            TimerIoCompletionEvent {
                origin: DataAccessOrigin::Cpu,
                completion_cycle: 10,
                address: 0x0400_0100,
                timer: 0,
                register: TimerIoRegister::CounterReload,
                kind: TimerIoAccessKind::Write,
                width: TimerIoAccessWidth::Halfword,
                value: 0xABCD,
            },
            TimerIoCompletionEvent {
                origin: DataAccessOrigin::Cpu,
                completion_cycle: 10,
                address: 0x0400_0102,
                timer: 0,
                register: TimerIoRegister::Control,
                kind: TimerIoAccessKind::Write,
                width: TimerIoAccessWidth::Halfword,
                value: 0x0080,
            },
        ]
    );
    assert!(timeline.completion_events_are_bounded(cpu.timer_io_completion_events()));
    assert!(timeline.completion_events_fit_required_timeline(cpu.timer_io_completion_events()));
}

#[test]
fn thumb_word_store_emits_ordered_timer_halfword_completions() {
    let mut bus = bus_with_rom(&0x6008u16.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[0] = 0x00C0_FFFE;
    cpu.regs[1] = 0x0400_0100;

    cpu.step(&mut bus);

    assert_eq!(cpu.data_access_cycles(), 1);
    assert_eq!(cpu.timer_io_completion_events().len(), 2);
    let timeline = cpu.instruction_timeline();
    assert_eq!(timeline.fetch_cycles, 5);
    assert_eq!(timeline.total_cycles, 6);
    assert_eq!(timeline.data_access_count, 1);
    assert_eq!(timeline.replaced_legacy_data_cycles, 1);
    assert_eq!(timeline.incremental_non_data_cycles, 0);
    assert_eq!(timeline.required_cycles, 6);
    assert_eq!(cpu.timer_io_completion_events()[0].completion_cycle, 6);
    assert_eq!(cpu.timer_io_completion_events()[0].value, 0xFFFE);
    assert_eq!(cpu.timer_io_completion_events()[1].completion_cycle, 6);
    assert_eq!(cpu.timer_io_completion_events()[1].value, 0x00C0);
    assert!(timeline.completion_events_are_bounded(cpu.timer_io_completion_events()));
    assert!(timeline.completion_events_fit_required_timeline(cpu.timer_io_completion_events()));
}

#[test]
fn hle_cpu_set_cursor_includes_source_waitstates_before_timer_write() {
    let mut bus = bus_with_rom(&[]);
    let mut cpu = Cpu::new();
    cpu.reset();
    bus.write32(0x0200_0000, 0x0080_FFFF);
    cpu.regs[0] = 0x0200_0000;
    cpu.regs[1] = 0x0400_0100;
    cpu.regs[2] = (1 << 26) | (1 << 24) | 1;

    cpu.execute_software_interrupt(&mut bus, 0x0B);

    assert_eq!(cpu.data_access_cycles(), 7);
    let timeline = cpu.instruction_timeline();
    assert_eq!(timeline.fetch_cycles, 0);
    assert_eq!(timeline.total_cycles, 4);
    assert_eq!(timeline.data_access_count, 2);
    assert_eq!(timeline.replaced_legacy_data_cycles, 0);
    assert_eq!(timeline.incremental_non_data_cycles, 4);
    assert_eq!(timeline.required_cycles, 11);
    assert_eq!(cpu.timer_io_completion_events().len(), 2);
    assert_eq!(cpu.timer_io_completion_events()[0].completion_cycle, 7);
    assert_eq!(cpu.timer_io_completion_events()[1].completion_cycle, 7);
    assert!(!timeline.completion_events_are_bounded(cpu.timer_io_completion_events()));
    assert!(timeline.completion_events_fit_required_timeline(cpu.timer_io_completion_events()));
}

#[test]
fn arm_halfword_timer_write_has_a_bounded_fetch_relative_completion() {
    let mut bus = bus_with_rom(&0xE1C1_20B0u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[1] = 0x0400_0100;
    cpu.regs[2] = 0xABCD;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    let events = cpu.timer_io_completion_events();
    assert_eq!(timeline.fetch_cycles, 8);
    assert_eq!(timeline.total_cycles, 9);
    assert_eq!(timeline.data_access_cycles, 1);
    assert_eq!(timeline.data_access_count, 1);
    assert_eq!(timeline.replaced_legacy_data_cycles, 1);
    assert_eq!(timeline.incremental_non_data_cycles, 0);
    assert_eq!(timeline.required_cycles, 9);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].completion_cycle, 9);
    assert!(timeline.completion_events_are_bounded(events));
}

#[test]
fn irq_handler_timer_disable_store_completes_after_two_iwram_cycles() {
    let mut bus = bus_with_rom(&[]);
    bus.write32(0x0300_0000, 0xE1C1_20B0);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.set_pc(0x0300_0000);
    cpu.regs[1] = 0x0400_0102;
    cpu.regs[2] = 0;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    let events = cpu.timer_io_completion_events();
    assert_eq!(timeline.fetch_cycles, 1);
    assert_eq!(timeline.total_cycles, 2);
    assert_eq!(timeline.required_cycles, 2);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0],
        TimerIoCompletionEvent {
            origin: DataAccessOrigin::Cpu,
            completion_cycle: 2,
            address: 0x0400_0102,
            timer: 0,
            register: TimerIoRegister::Control,
            kind: TimerIoAccessKind::Write,
            width: TimerIoAccessWidth::Halfword,
            value: 0,
        }
    );
    assert!(timeline.completion_events_are_bounded(events));
    assert!(timeline.completion_events_fit_required_timeline(events));
}

#[test]
fn thumb_mov_immediate_uses_the_sequential_fetch_cycle() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0300_0000, 0x2200);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.set_pc(0x0300_0000);
    cpu.regs[2] = u32::MAX;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    assert_eq!(cpu.regs[2], 0);
    assert_eq!(cpu.cycles, 1);
    assert_eq!(timeline.fetch_cycles, 1);
    assert_eq!(timeline.total_cycles, 1);
    assert_eq!(timeline.incremental_non_data_cycles, 0);
    assert_eq!(timeline.required_cycles, 1);
}

#[test]
fn thumb_register_shift_retains_its_internal_cycle() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0300_0000, 0x4088);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.set_pc(0x0300_0000);
    cpu.regs[0] = 1;
    cpu.regs[1] = 3;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    assert_eq!(cpu.regs[0], 8);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(timeline.fetch_cycles, 1);
    assert_eq!(timeline.total_cycles, 2);
    assert_eq!(timeline.incremental_non_data_cycles, 1);
    assert_eq!(timeline.required_cycles, 2);
}

#[test]
fn thumb_load_final_internal_cycle_overlaps_following_fetch() {
    let mut bus = bus_with_rom(&[]);
    bus.write16(0x0300_0000, 0x880A);
    bus.write16(0x0300_0002, 0x2307);
    bus.write16(0x0300_0100, 0xABCD);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.set_pc(0x0300_0000);
    cpu.regs[1] = 0x0300_0100;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[2], 0xABCD);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.instruction_timeline().incremental_non_data_cycles, 0);

    cpu.step(&mut bus);

    assert_eq!(cpu.regs[3], 7);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn thumb_halfword_timer_read_has_a_bounded_fetch_relative_completion() {
    let mut bus = bus_with_rom(&0x8808u16.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr |= CPSR_THUMB;
    cpu.regs[1] = 0x0400_0100;

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    let events = cpu.timer_io_completion_events();
    assert_eq!(timeline.fetch_cycles, 5);
    assert_eq!(timeline.total_cycles, 6);
    assert_eq!(timeline.data_access_cycles, 1);
    assert_eq!(timeline.data_access_count, 1);
    assert_eq!(timeline.replaced_legacy_data_cycles, 1);
    assert_eq!(timeline.incremental_non_data_cycles, 0);
    assert_eq!(timeline.required_cycles, 6);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].completion_cycle, 6);
    assert!(timeline.completion_events_are_bounded(events));
}

#[test]
fn arm_word_load_retains_internal_cycle_while_expanding_ewram_bus_phases() {
    let mut bus = bus_with_rom(&0xE591_2000u32.to_le_bytes());
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.regs[1] = 0x0200_0000;
    bus.write32(0x0200_0000, 0xA5A5_5A5A);

    cpu.step(&mut bus);

    let timeline = cpu.instruction_timeline();
    assert_eq!(cpu.regs[2], 0xA5A5_5A5A);
    assert_eq!(timeline.fetch_cycles, 8);
    assert_eq!(timeline.total_cycles, 10);
    assert_eq!(timeline.data_access_cycles, 6);
    assert_eq!(timeline.data_access_count, 1);
    assert_eq!(timeline.replaced_legacy_data_cycles, 1);
    assert_eq!(timeline.incremental_non_data_cycles, 1);
    assert_eq!(timeline.required_cycles, 15);
}
