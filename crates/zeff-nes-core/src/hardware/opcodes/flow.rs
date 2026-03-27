use crate::hardware::bus::Bus;
use crate::hardware::constants::{IRQ_VECTOR_LO, IRQ_VECTOR_HI, NMI_VECTOR_LO, NMI_VECTOR_HI};
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::*;

// 0x00: BRK
pub fn brk(cpu: &mut Cpu, bus: &mut Bus) {
    cpu.pc = cpu.pc.wrapping_add(1);
    cpu.push16(bus, cpu.pc);
    cpu.push8(bus, cpu.regs.status_for_push(true));
    cpu.regs.set_flag(INTERRUPT_FLAG, true);

    let (vec_lo, vec_hi) = if cpu.nmi_pending {
        cpu.nmi_pending = false;
        (NMI_VECTOR_LO, NMI_VECTOR_HI)
    } else {
        (IRQ_VECTOR_LO, IRQ_VECTOR_HI)
    };
    let lo = bus.cpu_read(vec_lo) as u16;
    let hi = bus.cpu_read(vec_hi) as u16;
    cpu.pc = (hi << 8) | lo;
}

// 0xEA: NOP
pub fn nop(_cpu: &mut Cpu, _bus: &mut Bus) {}

// Unofficial 2-byte NOPs (e.g. 0x80/0x82/0x89/0xC2/0xE2).
pub fn nop_imm(cpu: &mut Cpu, bus: &mut Bus) {
    let _ = cpu.addr_immediate(bus);
}

// Returns extra cycles: 0 (not taken), 1 (taken same page), 2 (taken + page cross)
fn branch(cpu: &mut Cpu, bus: &mut Bus, condition: bool) -> u8 {
    let target = cpu.addr_relative(bus);
    if condition {
        let page_cross = (cpu.pc & 0xFF00) != (target & 0xFF00);
        cpu.pc = target;
        if page_cross { 2 } else { 1 }
    } else {
        0
    }
}

// 0x90: BCC
pub fn bcc(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(CARRY_FLAG))
}

// 0xB0: BCS
pub fn bcs(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(CARRY_FLAG))
}

// 0xF0: BEQ
pub fn beq(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(ZERO_FLAG))
}

// 0xD0: BNE
pub fn bne(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(ZERO_FLAG))
}

// 0x30: BMI
pub fn bmi(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(NEGATIVE_FLAG))
}

// 0x10: BPL
pub fn bpl(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(NEGATIVE_FLAG))
}

// 0x70: BVS
pub fn bvs(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, cpu.regs.get_flag(OVERFLOW_FLAG))
}

// 0x50: BVC
pub fn bvc(cpu: &mut Cpu, bus: &mut Bus) -> u8 {
    branch(cpu, bus, !cpu.regs.get_flag(OVERFLOW_FLAG))
}

// 0x4C: JMP abs
pub fn jmp_abs(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.pc = a;
}

// 0x6C: JMP (ind)
pub fn jmp_ind(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_indirect(bus);
    cpu.pc = a;
}

// 0x20: JSR abs
pub fn jsr(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.addr_absolute(bus);
    cpu.push16(bus, cpu.pc.wrapping_sub(1));
    cpu.pc = a;
}

// 0x60: RTS
pub fn rts(cpu: &mut Cpu, bus: &mut Bus) {
    let a = cpu.pop16(bus);
    cpu.pc = a.wrapping_add(1);
}

// 0x40: RTI
pub fn rti(cpu: &mut Cpu, bus: &mut Bus) {
    let p = cpu.pop8(bus);
    cpu.regs.p = (p & 0xEF) | 0x20;
    cpu.pc = cpu.pop16(bus);
}

// 0x18: CLC
pub fn clc(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(CARRY_FLAG, false);
}

// 0x38: SEC
pub fn sec(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(CARRY_FLAG, true);
}

// 0x58: CLI
pub fn cli(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(INTERRUPT_FLAG, false);
}

// 0x78: SEI
pub fn sei(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(INTERRUPT_FLAG, true);
}

// 0xD8: CLD
pub fn cld(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(DECIMAL_FLAG, false);
}

// 0xF8: SED
pub fn sed(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(DECIMAL_FLAG, true);
}

// 0xB8: CLV
pub fn clv(cpu: &mut Cpu, _bus: &mut Bus) {
    cpu.regs.set_flag(OVERFLOW_FLAG, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::bus::Bus;
    use crate::hardware::cartridge::Cartridge;

    fn build_bus_with_program(program: &[u8]) -> Bus {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;

        let prg_start = 16;
        rom[prg_start..prg_start + program.len()].copy_from_slice(program);

        rom[prg_start + 0x3FFC] = 0x00;
        rom[prg_start + 0x3FFD] = 0x80;

        let cart = Cartridge::load(&rom).expect("test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    // Helper: build bus, reset CPU, return both ready to run
    fn setup(program: &[u8]) -> (Cpu, Bus) {
        let mut bus = build_bus_with_program(program);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn unofficial_nop_immediate_consumes_operand_byte() {
        let mut bus = build_bus_with_program(&[0x80, 0x02, 0xEA]);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 2);
        assert_eq!(cpu.last_opcode, 0x80);
        assert_eq!(cpu.pc, 0x8002);

        cpu.step(&mut bus);
        assert_eq!(cpu.last_opcode, 0xEA);
    }

    // ── ADC Tests ───────────────────────────────────────────────────────

    #[test]
    fn adc_imm_basic() {
        // LDA #$10; ADC #$20 → A = $30, no carry, no overflow
        let (mut cpu, mut bus) = setup(&[0xA9, 0x10, 0x69, 0x20]);
        cpu.step(&mut bus); // LDA #$10
        cpu.step(&mut bus); // ADC #$20
        assert_eq!(cpu.regs.a, 0x30);
        assert!(!cpu.regs.get_flag(CARRY_FLAG));
        assert!(!cpu.regs.get_flag(OVERFLOW_FLAG));
        assert!(!cpu.regs.get_flag(ZERO_FLAG));
        assert!(!cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    #[test]
    fn adc_carry_out() {
        // LDA #$FF; ADC #$01 → A = $00, C=1, Z=1
        let (mut cpu, mut bus) = setup(&[0xA9, 0xFF, 0x69, 0x01]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(ZERO_FLAG));
    }

    #[test]
    fn adc_carry_in() {
        // SEC; LDA #$10; ADC #$20 → A = $31 (carry adds 1)
        let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x10, 0x69, 0x20]);
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // LDA #$10
        cpu.step(&mut bus); // ADC #$20
        assert_eq!(cpu.regs.a, 0x31);
    }

    #[test]
    fn adc_overflow_positive() {
        // LDA #$50; ADC #$50 → $A0 (positive + positive = negative → overflow)
        let (mut cpu, mut bus) = setup(&[0xA9, 0x50, 0x69, 0x50]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0xA0);
        assert!(cpu.regs.get_flag(OVERFLOW_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    #[test]
    fn adc_overflow_negative() {
        // LDA #$D0; ADC #$90 → $60 (negative + negative = positive → overflow)
        // CLC first to ensure carry is clear
        let (mut cpu, mut bus) = setup(&[0x18, 0xA9, 0xD0, 0x69, 0x90]);
        cpu.step(&mut bus); // CLC
        cpu.step(&mut bus); // LDA #$D0
        cpu.step(&mut bus); // ADC #$90
        assert_eq!(cpu.regs.a, 0x60);
        assert!(cpu.regs.get_flag(OVERFLOW_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
    }

    // ── SBC Tests ───────────────────────────────────────────────────────

    #[test]
    fn sbc_basic() {
        // SEC; LDA #$30; SBC #$10 → A = $20, C=1 (no borrow)
        let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x30, 0xE9, 0x10]);
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // LDA #$30
        cpu.step(&mut bus); // SBC #$10
        assert_eq!(cpu.regs.a, 0x20);
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(!cpu.regs.get_flag(OVERFLOW_FLAG));
    }

    #[test]
    fn sbc_borrow() {
        // CLC; LDA #$10; SBC #$20 → $EF (borrow), C=0
        let (mut cpu, mut bus) = setup(&[0x18, 0xA9, 0x10, 0xE9, 0x20]);
        cpu.step(&mut bus); // CLC
        cpu.step(&mut bus); // LDA #$10
        cpu.step(&mut bus); // SBC #$20
        assert_eq!(cpu.regs.a, 0xEF);
        assert!(!cpu.regs.get_flag(CARRY_FLAG));
    }

    #[test]
    fn sbc_overflow() {
        // SEC; LDA #$50; SBC #$B0 → $A0 (positive - negative = negative → overflow)
        let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x50, 0xE9, 0xB0]);
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // LDA #$50
        cpu.step(&mut bus); // SBC #$B0
        assert_eq!(cpu.regs.a, 0xA0);
        assert!(cpu.regs.get_flag(OVERFLOW_FLAG));
    }

    // ── CMP / CPX / CPY Tests ───────────────────────────────────────────

    #[test]
    fn cmp_equal() {
        // LDA #$42; CMP #$42 → Z=1, C=1, N=0
        let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xC9, 0x42]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(!cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    #[test]
    fn cmp_greater() {
        // LDA #$42; CMP #$20 → Z=0, C=1, N=0
        let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xC9, 0x20]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(!cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
    }

    #[test]
    fn cmp_less() {
        // LDA #$20; CMP #$42 → Z=0, C=0, N=1
        let (mut cpu, mut bus) = setup(&[0xA9, 0x20, 0xC9, 0x42]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(!cpu.regs.get_flag(ZERO_FLAG));
        assert!(!cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    #[test]
    fn cpx_basic() {
        // LDX #$10; CPX #$10 → Z=1, C=1
        let (mut cpu, mut bus) = setup(&[0xA2, 0x10, 0xE0, 0x10]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
    }

    #[test]
    fn cpy_basic() {
        // LDY #$FF; CPY #$00 → Z=0, C=1, N=1
        let (mut cpu, mut bus) = setup(&[0xA0, 0xFF, 0xC0, 0x00]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert!(!cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    // ── Branch Cycle Counting ───────────────────────────────────────────

    #[test]
    fn branch_not_taken_2_cycles() {
        // SEC; BCC +2 → not taken, 2 cycles
        let (mut cpu, mut bus) = setup(&[0x38, 0x90, 0x02, 0xEA]);
        cpu.step(&mut bus); // SEC
        let cycles = cpu.step(&mut bus); // BCC (not taken)
        assert_eq!(cycles, 2); // base 2 + 0 extra
    }

    #[test]
    fn branch_taken_same_page_3_cycles() {
        // CLC; BCC +2 → taken, same page, 3 cycles
        let (mut cpu, mut bus) = setup(&[0x18, 0x90, 0x02, 0xEA, 0xEA]);
        cpu.step(&mut bus); // CLC
        let cycles = cpu.step(&mut bus); // BCC (taken, same page)
        assert_eq!(cycles, 3); // base 2 + 1 extra
    }

    // ── Page-Cross Cycle Penalty ────────────────────────────────────────

    #[test]
    fn lda_abs_x_no_page_cross() {
        // LDX #$01; LDA $8010,X → no page cross, 4 cycles
        let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xBD, 0x10, 0x80]);
        cpu.step(&mut bus); // LDX #$01
        let cycles = cpu.step(&mut bus); // LDA $8010,X
        assert_eq!(cycles, 4);
    }

    #[test]
    fn lda_abs_x_page_cross() {
        // LDX #$FF; LDA $80FF,X → page cross ($80FF+$FF=$81FE), 5 cycles
        let (mut cpu, mut bus) = setup(&[0xA2, 0xFF, 0xBD, 0xFF, 0x80]);
        cpu.step(&mut bus); // LDX #$FF
        let cycles = cpu.step(&mut bus); // LDA $80FF,X
        assert_eq!(cycles, 5); // 4 base + 1 penalty
    }

    // ── JMP Indirect Page Boundary Bug ──────────────────────────────────

    #[test]
    fn jmp_indirect_page_boundary_bug() {
        // JMP ($80FF) — pointer at $80FF, hi byte wraps to $8000 (not $8100)
        // Set up: $80FF = lo byte, $8000 = hi byte (6502 bug)
        let mut program = vec![0u8; 0x4000];
        // At $8000: JMP ($80FF)
        program[0] = 0x6C;
        program[1] = 0xFF;
        program[2] = 0x80;
        // At $80FF: lo byte of target
        program[0xFF] = 0x10; // lo = $10
        // At $8000: (also our JMP instruction's first byte) hi byte wraps here
        // program[0] is 0x6C which becomes the hi byte = 0x6C → target = $6C10
        // Actually let me put specific bytes. We need the JMP somewhere else.
        // Let's put program start after the jump target setup
        program[0] = 0xEA; // $8000 = NOP (will be hi byte of buggy read)
        program[3] = 0x6C; // $8003: JMP ($80FF)
        program[4] = 0xFF;
        program[5] = 0x80;
        program[0xFF] = 0x20; // $80FF = lo byte
        // On the 6502 bug: hi byte reads from $8000, not $8100
        // $8000 = 0xEA. So jump target = $EA20

        // Reset vector → $8003
        program[0x3FFC] = 0x03;
        program[0x3FFD] = 0x80;

        let mut rom = vec![0u8; 16 + 0x2000]; // header + CHR
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let mut full_rom = Vec::new();
        full_rom.extend_from_slice(&rom[..16]); // header
        full_rom.extend_from_slice(&program);    // PRG
        full_rom.extend_from_slice(&rom[16..16 + 0x2000]); // CHR

        let cart = Cartridge::load(&full_rom).expect("test ROM");
        let mut bus = Bus::new(cart, 44_100.0);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        cpu.step(&mut bus); // JMP ($80FF)
        // Target = hi:$8000=0xEA, lo:$80FF=0x20 → $EA20
        assert_eq!(cpu.pc, 0xEA20);
    }

    // ── Zero Page X Wrapping ────────────────────────────────────────────

    #[test]
    fn zero_page_x_wraps() {
        // Store $42 at ZP $03
        // LDX #$04; LDA $FF,X → should read from $03 (wrap), not $0103
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0x42, // LDA #$42
            0x85, 0x03, // STA $03
            0xA2, 0x04, // LDX #$04
            0xB5, 0xFF, // LDA $FF,X → wraps to $03
        ]);
        cpu.step(&mut bus); // LDA #$42
        cpu.step(&mut bus); // STA $03
        cpu.step(&mut bus); // LDX #$04
        cpu.step(&mut bus); // LDA $FF,X
        assert_eq!(cpu.regs.a, 0x42);
    }

    // ── BRK / NMI Hijack ────────────────────────────────────────────────

    #[test]
    fn brk_normal_uses_irq_vector() {
        // BRK with no NMI pending → jumps to IRQ vector
        let mut program = vec![0xEA; 0x4000]; // fill with NOP
        program[0] = 0x00; // BRK at $8000
        // IRQ vector → $8100
        program[0x3FFE] = 0x00;
        program[0x3FFF] = 0x81;
        // NMI vector → $8200
        program[0x3FFA] = 0x00;
        program[0x3FFB] = 0x82;
        // Reset vector → $8000
        program[0x3FFC] = 0x00;
        program[0x3FFD] = 0x80;

        let mut rom = vec![0u8; 16 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let mut full = Vec::new();
        full.extend_from_slice(&rom[..16]);
        full.extend_from_slice(&program);
        full.extend_from_slice(&rom[16..16 + 0x2000]);

        let cart = Cartridge::load(&full).expect("test ROM");
        let mut bus = Bus::new(cart, 44_100.0);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        cpu.step(&mut bus); // BRK
        assert_eq!(cpu.pc, 0x8100, "BRK without NMI should use IRQ vector");
    }

    #[test]
    fn brk_nmi_hijack_via_step_services_nmi_first() {
        // When nmi_pending is true at the start of step(), the NMI is serviced
        // before BRK executes (NMI has higher priority at instruction boundary)
        let mut program = vec![0xEA; 0x4000];
        program[0] = 0x00; // BRK
        program[0x3FFE] = 0x00;
        program[0x3FFF] = 0x81; // IRQ → $8100
        program[0x3FFA] = 0x00;
        program[0x3FFB] = 0x82; // NMI → $8200
        program[0x3FFC] = 0x00;
        program[0x3FFD] = 0x80;

        let mut rom = vec![0u8; 16 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let mut full = Vec::new();
        full.extend_from_slice(&rom[..16]);
        full.extend_from_slice(&program);
        full.extend_from_slice(&rom[16..16 + 0x2000]);

        let cart = Cartridge::load(&full).expect("test ROM");
        let mut bus = Bus::new(cart, 44_100.0);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        cpu.nmi_pending = true;
        cpu.step(&mut bus);
        // NMI is serviced first (checked before opcode fetch in step())
        assert_eq!(cpu.pc, 0x8200, "NMI should be serviced before BRK executes");
        assert!(!cpu.nmi_pending, "NMI should be consumed");
    }

    #[test]
    fn brk_hijack_via_direct_call() {
        // Test the brk() function directly with nmi_pending set
        let mut program = vec![0xEA; 0x4000];
        // IRQ vector → $8100
        program[0x3FFE] = 0x00;
        program[0x3FFF] = 0x81;
        // NMI vector → $8200
        program[0x3FFA] = 0x00;
        program[0x3FFB] = 0x82;
        program[0x3FFC] = 0x00;
        program[0x3FFD] = 0x80;

        let mut rom = vec![0u8; 16 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let mut full = Vec::new();
        full.extend_from_slice(&rom[..16]);
        full.extend_from_slice(&program);
        full.extend_from_slice(&rom[16..16 + 0x2000]);

        let cart = Cartridge::load(&full).expect("test ROM");
        let mut bus = Bus::new(cart, 44_100.0);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);

        // Simulate: BRK is executing, NMI fires during push sequence
        cpu.nmi_pending = true;
        brk(&mut cpu, &mut bus);

        assert_eq!(cpu.pc, 0x8200, "BRK with NMI pending should hijack to NMI vector");
        assert!(!cpu.nmi_pending, "NMI should be consumed");
    }

    // ── Unofficial Opcode Tests ─────────────────────────────────────────

    #[test]
    fn lax_zp_loads_a_and_x() {
        // Store $42 at ZP $10, then LAX $10 (opcode 0xA7)
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0x42, // LDA #$42
            0x85, 0x10, // STA $10
            0xA7, 0x10, // *LAX $10
        ]);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // STA
        cpu.step(&mut bus); // LAX
        assert_eq!(cpu.regs.a, 0x42);
        assert_eq!(cpu.regs.x, 0x42);
    }

    #[test]
    fn sax_zp_stores_a_and_x() {
        // LDA #$FF; LDX #$0F; SAX $10 → stores $0F (A & X) at $10
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0xFF, // LDA #$FF
            0xA2, 0x0F, // LDX #$0F
            0x87, 0x10, // *SAX $10
            0xA9, 0x00, // LDA #$00 (clear A)
            0xA5, 0x10, // LDA $10 (read back result)
        ]);
        cpu.step(&mut bus); // LDA
        cpu.step(&mut bus); // LDX
        cpu.step(&mut bus); // SAX
        cpu.step(&mut bus); // LDA #$00
        cpu.step(&mut bus); // LDA $10
        assert_eq!(cpu.regs.a, 0x0F);
    }

    #[test]
    fn dcp_zp_decrements_and_compares() {
        // Store $43 at ZP $10, LDA #$42, DCP $10 → mem=$42, CMP $42=$42 → Z=1
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0x43, // LDA #$43
            0x85, 0x10, // STA $10
            0xA9, 0x42, // LDA #$42
            0xC7, 0x10, // *DCP $10
        ]);
        cpu.step(&mut bus); // LDA #$43
        cpu.step(&mut bus); // STA $10
        cpu.step(&mut bus); // LDA #$42
        cpu.step(&mut bus); // DCP $10
        assert!(cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(CARRY_FLAG));
    }

    #[test]
    fn isb_zp_increments_and_subtracts() {
        // Store $09 at ZP $10, SEC; LDA #$20; ISB $10 → mem=$0A, A=$20-$0A=$16
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0x09, // LDA #$09
            0x85, 0x10, // STA $10
            0x38,       // SEC
            0xA9, 0x20, // LDA #$20
            0xE7, 0x10, // *ISB $10
        ]);
        cpu.step(&mut bus); // LDA #$09
        cpu.step(&mut bus); // STA $10
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // LDA #$20
        cpu.step(&mut bus); // ISB $10
        assert_eq!(cpu.regs.a, 0x16);
    }

    // ── Stack / Push / Pop Tests ────────────────────────────────────────

    #[test]
    fn php_plp_preserves_flags() {
        // SEC; SEI; PHP; CLC; CLI; PLP → flags restored
        let (mut cpu, mut bus) = setup(&[
            0x38, // SEC
            0x78, // SEI
            0x08, // PHP
            0x18, // CLC
            0x58, // CLI
            0x28, // PLP
        ]);
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // SEI
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(INTERRUPT_FLAG));
        cpu.step(&mut bus); // PHP
        cpu.step(&mut bus); // CLC
        cpu.step(&mut bus); // CLI
        assert!(!cpu.regs.get_flag(CARRY_FLAG));
        assert!(!cpu.regs.get_flag(INTERRUPT_FLAG));
        cpu.step(&mut bus); // PLP
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(INTERRUPT_FLAG));
    }

    #[test]
    fn pha_pla_preserves_accumulator() {
        // LDA #$42; PHA; LDA #$00; PLA → A = $42
        let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
        cpu.step(&mut bus); // LDA #$42
        cpu.step(&mut bus); // PHA
        cpu.step(&mut bus); // LDA #$00
        assert_eq!(cpu.regs.a, 0x00);
        cpu.step(&mut bus); // PLA
        assert_eq!(cpu.regs.a, 0x42);
    }

    // ── Transfer Tests ──────────────────────────────────────────────────

    #[test]
    fn tax_tay_transfer() {
        // LDA #$42; TAX; TAY → X=$42, Y=$42
        let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xAA, 0xA8]);
        cpu.step(&mut bus); // LDA #$42
        cpu.step(&mut bus); // TAX
        assert_eq!(cpu.regs.x, 0x42);
        cpu.step(&mut bus); // TAY
        assert_eq!(cpu.regs.y, 0x42);
    }

    // ── JSR / RTS ───────────────────────────────────────────────────────

    #[test]
    fn jsr_rts_round_trip() {
        // JSR $8005; NOP → at $8005: LDA #$42; RTS → returns, NOP at $8003
        let (mut cpu, mut bus) = setup(&[
            0x20, 0x05, 0x80, // JSR $8005
            0xEA,             // NOP (return here)
            0xEA,             // NOP (padding)
            0xA9, 0x42,       // LDA #$42 (at $8005)
            0x60,             // RTS
        ]);
        cpu.step(&mut bus); // JSR $8005
        assert_eq!(cpu.pc, 0x8005);
        cpu.step(&mut bus); // LDA #$42
        assert_eq!(cpu.regs.a, 0x42);
        cpu.step(&mut bus); // RTS
        assert_eq!(cpu.pc, 0x8003); // returns to instruction after JSR
    }

    // ── BIT test ────────────────────────────────────────────────────────

    #[test]
    fn bit_test_flags() {
        // Store $C0 at ZP $10; LDA #$FF; BIT $10 → Z=0 (A&$C0=$C0≠0), V=1 (bit6), N=1 (bit7)
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0xC0, 0x85, 0x10,
            0xA9, 0xFF, 0x24, 0x10,
        ]);
        cpu.step(&mut bus); // LDA #$C0
        cpu.step(&mut bus); // STA $10
        cpu.step(&mut bus); // LDA #$FF
        cpu.step(&mut bus); // BIT $10
        assert!(!cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(OVERFLOW_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    #[test]
    fn bit_test_zero_result() {
        // Store $C0 at ZP $10; LDA #$00; BIT $10 → Z=1, V=1, N=1
        let (mut cpu, mut bus) = setup(&[
            0xA9, 0xC0, 0x85, 0x10,
            0xA9, 0x00, 0x24, 0x10,
        ]);
        cpu.step(&mut bus); // LDA #$C0
        cpu.step(&mut bus); // STA $10
        cpu.step(&mut bus); // LDA #$00
        cpu.step(&mut bus); // BIT $10
        assert!(cpu.regs.get_flag(ZERO_FLAG));
        assert!(cpu.regs.get_flag(OVERFLOW_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    // ── Shift/Rotate Tests ──────────────────────────────────────────────

    #[test]
    fn asl_accumulator() {
        // LDA #$81; ASL A → $02, C=1
        let (mut cpu, mut bus) = setup(&[0xA9, 0x81, 0x0A]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x02);
        assert!(cpu.regs.get_flag(CARRY_FLAG));
    }

    #[test]
    fn ror_accumulator_with_carry() {
        // SEC; LDA #$01; ROR A → $80, C=1 (old bit 0 goes to carry)
        let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x01, 0x6A]);
        cpu.step(&mut bus); // SEC
        cpu.step(&mut bus); // LDA #$01
        cpu.step(&mut bus); // ROR A
        assert_eq!(cpu.regs.a, 0x80);
        assert!(cpu.regs.get_flag(CARRY_FLAG));
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }

    // ── INC/DEC Tests ───────────────────────────────────────────────────

    #[test]
    fn inx_overflow_to_zero() {
        // LDX #$FF; INX → X=$00, Z=1
        let (mut cpu, mut bus) = setup(&[0xA2, 0xFF, 0xE8]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.x, 0x00);
        assert!(cpu.regs.get_flag(ZERO_FLAG));
    }

    #[test]
    fn dey_underflow() {
        // LDY #$00; DEY → Y=$FF, N=1
        let (mut cpu, mut bus) = setup(&[0xA0, 0x00, 0x88]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.y, 0xFF);
        assert!(cpu.regs.get_flag(NEGATIVE_FLAG));
    }
}

