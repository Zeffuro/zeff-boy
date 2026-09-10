use super::*;

const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const CPSR_THUMB: u32 = 1 << 5;
const IWRAM_START: u32 = 0x0300_0000;
const IRQ_FLAG: u16 = 1 << 3;

fn fixture(thumb: bool) -> Emulator {
    let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
    if thumb {
        emu.bus.write16(IWRAM_START, 0x46C0);
        emu.bus.write16(IWRAM_START + 2, 0x46C0);
        emu.cpu.cpsr |= CPSR_THUMB;
    } else {
        emu.bus.write32(IWRAM_START, 0xE1A0_0000);
        emu.bus.write32(IWRAM_START + 4, 0xE1A0_0000);
    }
    emu.cpu.set_pc(IWRAM_START);
    emu.bus.write16(0x0400_0100, 0xFF00);
    emu.bus.write16(0x0400_0102, 0x0080);
    emu.step_instruction();
    emu
}

fn arm_pending_masked_irq(thumb: bool, delay: u32) -> Emulator {
    let mut emu = fixture(thumb);
    emu.bus.write32(0x03FF_FFFC, IWRAM_START + 0x100);
    emu.bus.write32(IWRAM_START + 0x100, 0xE1A0_0000);
    emu.bus.write16(0x0400_0200, IRQ_FLAG);
    emu.bus.write16(0x0400_0208, 1);
    emu.bus.request_interrupt(IRQ_FLAG);
    assert!(emu.bus.set_irq_delay_state(Some(delay)));
    emu.cpu.cpsr |= CPSR_IRQ_DISABLE;
    assert!(emu.bus.interrupt_ready());
    emu
}

fn timer_snapshot(emu: &Emulator) -> [(u16, u16, u16); 4] {
    emu.bus
        .timer_registers_snapshot()
        .map(|timer| (timer.reload, timer.counter, timer.control))
}

fn assert_masked_instruction_matches_no_irq(thumb: bool, delay: u32) {
    let mut baseline = fixture(thumb);
    baseline.cpu.cpsr |= CPSR_IRQ_DISABLE;
    let mut masked = arm_pending_masked_irq(thumb, delay);

    baseline.step_instruction();
    masked.step_instruction();

    assert_eq!(masked.cpu_pc(), baseline.cpu_pc());
    assert_eq!(masked.cpu_cycles(), baseline.cpu_cycles());
    assert_eq!(timer_snapshot(&masked), timer_snapshot(&baseline));
    assert_eq!(
        masked.bus.timer_timing_state(),
        baseline.bus.timer_timing_state()
    );
    assert_eq!(masked.cpu_mode(), crate::hardware::cpu::CpuMode::System);
    assert_ne!(masked.bus.read16(0x0400_0202) & IRQ_FLAG, 0);
    assert_eq!(masked.bus.irq_delay_state(), Some(delay - 1));

    masked.cpu.cpsr &= !CPSR_IRQ_DISABLE;
    masked.step_instruction();
    assert_eq!(masked.cpu_mode(), crate::hardware::cpu::CpuMode::Irq);
}

#[test]
fn masked_pending_irq_keeps_ready_delay_and_device_time_for_arm_and_thumb() {
    for thumb in [false, true] {
        for delay in 1..=3 {
            assert_masked_instruction_matches_no_irq(thumb, delay);
        }
    }
}

#[test]
fn native_restore_preserves_masked_pending_irq_without_consuming_its_delay() {
    let mut source = arm_pending_masked_irq(false, 2);
    let state = source.encode_state().unwrap();
    let mut restored = fixture(false);
    restored.load_state(&state).unwrap();

    assert_ne!(restored.cpu_cpsr() & CPSR_IRQ_DISABLE, 0);
    assert_ne!(restored.bus.read16(0x0400_0202) & IRQ_FLAG, 0);
    assert_eq!(restored.bus.irq_delay_state(), Some(2));

    source.step_instruction();
    restored.step_instruction();
    assert_eq!(
        restored.encode_state().unwrap(),
        source.encode_state().unwrap()
    );
    assert_eq!(restored.bus.irq_delay_state(), Some(1));
}
