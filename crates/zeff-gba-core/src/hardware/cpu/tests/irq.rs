use super::*;

#[test]
fn irq_exception_enters_bios_stub_and_restores_user_sp_lr() {
    let mut rom = vec![0; 0x104];
    rom[0x100..0x104].copy_from_slice(&0xE12F_FF1E_u32.to_le_bytes()); // bx lr
    let mut bus = bus_with_rom(&rom);
    bus.write32(0x03FF_FFFC, 0x0800_0100);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr &= !CPSR_IRQ_DISABLE;
    cpu.regs[13] = 0x0300_7000;
    cpu.regs[14] = 0x0800_2222;

    cpu.try_service_irq(true);
    for _ in 0..8 {
        cpu.step(&mut bus);
    }

    assert_eq!(cpu.pc(), RESET_VECTOR);
    assert_eq!(cpu.regs[13], 0x0300_7000);
    assert_eq!(cpu.regs[14], 0x0800_2222);
    assert_eq!(cpu.cpsr & CPSR_IRQ_DISABLE, 0);
}

#[test]
fn irq_return_sets_protected_bios_latch_for_following_game_reads() {
    let mut rom = vec![0; 0x104];
    rom[0x100..0x104].copy_from_slice(&0xE12F_FF1E_u32.to_le_bytes()); // bx lr
    let mut bus = bus_with_rom(&rom);
    bus.write32(0x03FF_FFFC, 0x0800_0100);
    let mut cpu = Cpu::new();
    cpu.reset();
    cpu.cpsr &= !CPSR_IRQ_DISABLE;

    cpu.try_service_irq(true);
    for _ in 0..8 {
        cpu.step(&mut bus);
    }
    cpu.fetch_decode_stub(&bus);

    assert_eq!(cpu.cpu_read32(&bus, 0x0000_0000), 0xE55E_C002);
    assert_eq!(cpu.cpu_read16(&bus, 0x0000_0000), 0xC002);
}
