use super::Cpu;
use crate::hardware::bus::Bus;

impl Cpu {
    pub(super) fn execute_software_interrupt(&mut self, bus: &mut Bus, function: u32) {
        if bus.has_external_bios() {
            self.enter_software_interrupt_exception();
            return;
        }

        match function {
            0x00 => self.swi_soft_reset(bus),
            0x01 => bus.register_ram_reset(self.regs[0] as u8),
            0x02 => self.state = super::CpuState::Halted,
            0x04 => self.swi_intr_wait(bus, self.regs[0] != 0, self.regs[1] as u16),
            0x05 => self.swi_intr_wait(bus, true, 1),
            0x06 => self.swi_div(),
            0x07 => self.swi_div_arm(),
            0x08 => self.swi_sqrt(),
            0x09 => self.swi_arc_tan(),
            0x0A => self.swi_arc_tan2(),
            0x0E => self.swi_bg_affine_set(bus),
            0x0F => self.swi_obj_affine_set(bus),
            0x10 => self.swi_bit_unpack(bus),
            0x11 => self.swi_lz77_uncomp(bus, DecompWriteMode::Byte),
            0x12 => self.swi_lz77_uncomp(bus, DecompWriteMode::Halfword),
            0x13 => self.swi_huff_uncomp(bus),
            0x0B => self.swi_cpu_set(bus, false),
            0x0C => self.swi_cpu_set(bus, true),
            0x14 => self.swi_rl_uncomp(bus, DecompWriteMode::Byte),
            0x15 => self.swi_rl_uncomp(bus, DecompWriteMode::Halfword),
            0x16 => self.swi_diff_8bit_unfilter(bus, DecompWriteMode::Byte),
            0x17 => self.swi_diff_8bit_unfilter(bus, DecompWriteMode::Halfword),
            0x18 => self.swi_diff_16bit_unfilter(bus),
            0x19 => bus.set_sound_bias_level(self.regs[0] != 0),
            _ => {}
        }
        self.bios_protected_read_latch = super::POST_SWI_BIOS_READ_LATCH;
        self.cycles = self.cycles.wrapping_add(4);
    }

    fn enter_software_interrupt_exception(&mut self) {
        let old_cpsr = self.cpsr;
        let return_pc = self.pc();
        self.set_cpsr(
            (old_cpsr & !(super::CPSR_MODE_MASK | super::CPSR_THUMB))
                | super::CPSR_IRQ_DISABLE
                | 0x13,
        );
        self.spsr = old_cpsr;
        self.regs[14] = return_pc;
        self.set_pc(0x0000_0008);
        self.next_fetch_sequential = false;
        self.state = super::CpuState::Running;
    }

    fn swi_soft_reset(&mut self, bus: &mut Bus) {
        let return_to_wram = bus.read8(0x0300_7FFA) != 0;

        bus.iwram[0x7E00..0x8000].fill(0);

        self.regs = [0; 16];
        self.banked_r8_r12 = [[0; 5]; super::R8_R12_BANKS];
        self.banked_lr = [0; super::CPU_BANKS];
        self.banked_spsr = [0; super::CPU_BANKS];
        self.banked_sp = [0; super::CPU_BANKS];
        self.banked_sp[super::BANK_USER_SYSTEM] = 0x0300_7F00;
        self.banked_sp[super::BANK_IRQ] = 0x0300_7FA0;
        self.banked_sp[super::BANK_SUPERVISOR] = 0x0300_7FE0;
        self.cpsr = super::POST_BIOS_CPSR;
        self.spsr = 0;
        self.regs[13] = 0x0300_7F00;
        self.regs[15] = if return_to_wram {
            0x0200_0000
        } else {
            super::RESET_VECTOR
        };
        self.state = super::CpuState::Running;
        self.swi_wait_return_pc = None;
        self.next_fetch_sequential = false;
        self.flush_prefetch_queue();
    }

    fn swi_intr_wait(&mut self, bus: &mut Bus, discard_old_flags: bool, mask: u16) {
        let mask = mask & 0x3FFF;
        if mask == 0 {
            return;
        }

        if discard_old_flags {
            bus.clear_bios_irq_flags(mask);
        }

        let ready = (bus.bios_irq_flags() | bus.enabled_interrupt_flags()) & mask;
        if ready != 0 {
            bus.clear_bios_irq_flags(ready);
            return;
        }

        bus.enable_master_interrupts();
        self.swi_wait_return_pc = Some(self.pc());
        self.state = super::CpuState::Halted;
    }

    fn swi_div(&mut self) {
        self.swi_div_operands(self.regs[0] as i32, self.regs[1] as i32);
    }

    fn swi_div_arm(&mut self) {
        self.swi_div_operands(self.regs[1] as i32, self.regs[0] as i32);
    }

    fn swi_div_operands(&mut self, numerator: i32, denominator: i32) {
        if denominator == 0 {
            self.regs[0] = if numerator < 0 { -1i32 } else { 1i32 } as u32;
            self.regs[1] = numerator as u32;
            self.regs[3] = 1;
            return;
        }

        if numerator == i32::MIN && denominator == -1 {
            self.regs[0] = i32::MIN as u32;
            self.regs[1] = 0;
            self.regs[3] = i32::MIN as u32;
            return;
        }

        let quotient = numerator / denominator;
        let remainder = numerator % denominator;
        self.regs[0] = quotient as u32;
        self.regs[1] = remainder as u32;
        self.regs[3] = quotient.wrapping_abs() as u32;
    }

    fn swi_sqrt(&mut self) {
        self.regs[0] = (self.regs[0] as f64).sqrt() as u32;
    }

    fn swi_arc_tan(&mut self) {
        let (result, r1, r3) = bios_arc_tan(self.regs[0] as i32);
        self.regs[0] = i32::from(result) as u32;
        self.regs[1] = r1 as u32;
        self.regs[3] = r3 as u32;
    }

    fn swi_arc_tan2(&mut self) {
        let (result, r1) = bios_arc_tan2(self.regs[0] as i32, self.regs[1] as i32);
        self.regs[0] = u32::from(result as u16);
        if let Some(r1) = r1 {
            self.regs[1] = r1 as u32;
        }
        self.regs[3] = 0x170;
    }

    fn swi_cpu_set(&mut self, bus: &mut Bus, fast: bool) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let mode = self.regs[2];
        let fixed_source = mode & (1 << 24) != 0;
        let word = fast || mode & (1 << 26) != 0;
        let count = mode & 0x1F_FFFF;
        if word {
            let fill = cpu_set_read32(bus, src);
            let units = if fast { (count + 7) & !7 } else { count };
            for _ in 0..units {
                let value = if fixed_source {
                    fill
                } else {
                    cpu_set_read32(bus, src)
                };
                bus.write32(dst, value);
                if !fixed_source {
                    src = src.wrapping_add(4);
                }
                dst = dst.wrapping_add(4);
            }
        } else {
            let fill = cpu_set_read16(bus, src);
            for _ in 0..count {
                let value = if fixed_source {
                    fill
                } else {
                    cpu_set_read16(bus, src)
                };
                bus.write16(dst, value);
                if !fixed_source {
                    src = src.wrapping_add(2);
                }
                dst = dst.wrapping_add(2);
            }
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_bg_affine_set(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        for _ in 0..self.regs[2] {
            let center_x = read_i32(bus, src);
            let center_y = read_i32(bus, src.wrapping_add(4));
            let display_x = i32::from(read_i16(bus, src.wrapping_add(8)));
            let display_y = i32::from(read_i16(bus, src.wrapping_add(10)));
            let scale_x = read_i16(bus, src.wrapping_add(12));
            let scale_y = read_i16(bus, src.wrapping_add(14));
            let angle = bus.read16(src.wrapping_add(16));
            let params = affine_params(scale_x, scale_y, angle);
            let start_x = center_x
                .wrapping_sub(i32::from(params.pa).wrapping_mul(display_x))
                .wrapping_sub(i32::from(params.pb).wrapping_mul(display_y));
            let start_y = center_y
                .wrapping_sub(i32::from(params.pc).wrapping_mul(display_x))
                .wrapping_sub(i32::from(params.pd).wrapping_mul(display_y));

            write_i16(bus, dst, params.pa);
            write_i16(bus, dst.wrapping_add(2), params.pb);
            write_i16(bus, dst.wrapping_add(4), params.pc);
            write_i16(bus, dst.wrapping_add(6), params.pd);
            write_i32(bus, dst.wrapping_add(8), start_x);
            write_i32(bus, dst.wrapping_add(12), start_y);

            src = src.wrapping_add(20);
            dst = dst.wrapping_add(16);
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_obj_affine_set(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let offset = self.regs[3];
        for _ in 0..self.regs[2] {
            let scale_x = read_i16(bus, src);
            let scale_y = read_i16(bus, src.wrapping_add(2));
            let angle = bus.read16(src.wrapping_add(4));
            let params = affine_params(scale_x, scale_y, angle);

            write_i16(bus, dst, params.pa);
            write_i16(bus, dst.wrapping_add(offset), params.pb);
            write_i16(bus, dst.wrapping_add(offset.wrapping_mul(2)), params.pc);
            write_i16(bus, dst.wrapping_add(offset.wrapping_mul(3)), params.pd);

            src = src.wrapping_add(6);
            dst = dst.wrapping_add(offset.wrapping_mul(4));
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_lz77_uncomp(&mut self, bus: &mut Bus, write_mode: DecompWriteMode) {
        let mut src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if header & 0xFF != 0x10 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let mut out = Vec::with_capacity(out_len);
        while out.len() < out_len {
            let flags = bus.read8(src);
            src = src.wrapping_add(1);
            for bit in (0..8).rev() {
                if out.len() >= out_len {
                    break;
                }
                if flags & (1 << bit) == 0 {
                    out.push(bus.read8(src));
                    src = src.wrapping_add(1);
                } else {
                    let first = bus.read8(src);
                    let second = bus.read8(src.wrapping_add(1));
                    src = src.wrapping_add(2);
                    let len = usize::from(first >> 4) + 3;
                    let disp = ((usize::from(first & 0x0F) << 8) | usize::from(second)) + 1;
                    for _ in 0..len {
                        if out.len() >= out_len {
                            break;
                        }
                        let value = out.get(out.len().wrapping_sub(disp)).copied().unwrap_or(0);
                        out.push(value);
                    }
                }
            }
        }
        let dst = write_decompressed_output(bus, dst, &out, write_mode);
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_rl_uncomp(&mut self, bus: &mut Bus, write_mode: DecompWriteMode) {
        let mut src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if header & 0xFF != 0x30 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let mut out = Vec::with_capacity(out_len);
        while out.len() < out_len {
            let control = bus.read8(src);
            src = src.wrapping_add(1);
            if control & 0x80 != 0 {
                let count = usize::from(control & 0x7F) + 3;
                let value = bus.read8(src);
                src = src.wrapping_add(1);
                for _ in 0..count {
                    if out.len() >= out_len {
                        break;
                    }
                    out.push(value);
                }
            } else {
                let count = usize::from(control) + 1;
                for _ in 0..count {
                    if out.len() >= out_len {
                        break;
                    }
                    let value = bus.read8(src);
                    src = src.wrapping_add(1);
                    out.push(value);
                }
            }
        }
        let dst = write_decompressed_output(bus, dst, &out, write_mode);
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_huff_uncomp(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if (header >> 4) & 0xF != 0x2 {
            return;
        }
        let data_size = header & 0xF;
        if data_size != 4 && data_size != 8 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let tree_size = usize::from(bus.read8(src));
        let tree_header = src;
        let tree_base = src.wrapping_add(1);
        let bitstream = tree_header.wrapping_add(((tree_size + 1) * 2) as u32);
        let mut bit_addr = bitstream;
        let mut bit_mask = 0u32;
        let mut bit_word = 0u32;
        let mut node_addr = tree_base;
        let mut out = Vec::with_capacity(out_len);
        let mut nibble_accum: Option<u8> = None;

        while out.len() < out_len {
            if bit_mask == 0 {
                bit_word = bus.read32(bit_addr);
                bit_addr = bit_addr.wrapping_add(4);
                bit_mask = 1 << 31;
            }
            let bit = u32::from(bit_word & bit_mask != 0);
            bit_mask >>= 1;

            let node = bus.read8(node_addr);
            let child_addr = (node_addr & !1)
                .wrapping_add(u32::from(node & 0x3F) * 2)
                .wrapping_add(2 + bit);
            let child = bus.read8(child_addr);
            let terminal = if bit == 0 {
                node & 0x80 != 0
            } else {
                node & 0x40 != 0
            };

            if terminal {
                if data_size == 8 {
                    out.push(child);
                } else {
                    let nibble = child & 0x0F;
                    if let Some(lo) = nibble_accum.take() {
                        out.push(lo | (nibble << 4));
                    } else {
                        nibble_accum = Some(nibble);
                    }
                }
                node_addr = tree_base;
            } else {
                node_addr = child_addr;
            }
        }

        let dst = write_words(bus, dst, &out);
        self.regs[0] = bit_addr;
        self.regs[1] = dst;
    }

    fn swi_bit_unpack(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let info = self.regs[2];
        let source_len = u32::from(bus.read16(info));
        let source_width = u32::from(bus.read8(info.wrapping_add(2))).max(1);
        let dest_width = u32::from(bus.read8(info.wrapping_add(3))).max(1);
        let offset = bus.read32(info.wrapping_add(4));
        let offset_value = offset & 0x7FFF_FFFF;
        let zero_data = offset & 0x8000_0000 != 0;
        let source_mask = (1u32 << source_width.min(31)).wrapping_sub(1);
        let dest_mask = if dest_width >= 32 {
            u32::MAX
        } else {
            (1u32 << dest_width) - 1
        };
        let mut accum = 0u32;
        let mut accum_bits = 0u32;
        let mut out = Vec::new();
        for _ in 0..source_len {
            let byte = bus.read8(src);
            src = src.wrapping_add(1);
            let mut bit = 0u32;
            while bit < 8 {
                let mut value = (u32::from(byte) >> bit) & source_mask;
                if value != 0 || zero_data {
                    value = value.wrapping_add(offset_value) & dest_mask;
                }
                accum |= value << accum_bits;
                accum_bits += dest_width;
                while accum_bits >= 8 {
                    out.push(accum as u8);
                    accum >>= 8;
                    accum_bits -= 8;
                }
                bit += source_width;
            }
        }
        if accum_bits != 0 {
            out.push(accum as u8);
        }
        for chunk in out.chunks(4) {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            bus.write32(dst, u32::from_le_bytes(word));
            dst = dst.wrapping_add(4);
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_diff_8bit_unfilter(&mut self, bus: &mut Bus, write_mode: DecompWriteMode) {
        let mut src = self.regs[0];
        let dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if header & 0xFF != 0x81 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let mut out = Vec::with_capacity(out_len);
        let mut last = 0u8;
        for i in 0..out_len {
            let diff = bus.read8(src);
            src = src.wrapping_add(1);
            last = if i == 0 {
                diff
            } else {
                last.wrapping_add(diff)
            };
            out.push(last);
        }
        let dst = write_decompressed_output(bus, dst, &out, write_mode);
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_diff_16bit_unfilter(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if header & 0xFF != 0x82 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let mut last = 0u16;
        for i in (0..out_len).step_by(2) {
            let diff = bus.read16(src);
            src = src.wrapping_add(2);
            last = if i == 0 {
                diff
            } else {
                last.wrapping_add(diff)
            };
            bus.write16(dst, last);
            dst = dst.wrapping_add(2);
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }
}

#[derive(Clone, Copy)]
enum DecompWriteMode {
    Byte,
    Halfword,
}

struct AffineParams {
    pa: i16,
    pb: i16,
    pc: i16,
    pd: i16,
}

fn affine_params(scale_x: i16, scale_y: i16, angle: u16) -> AffineParams {
    let radians = f64::from(angle >> 8) * std::f64::consts::TAU / 256.0;
    let sin = radians.sin();
    let cos = radians.cos();
    AffineParams {
        pa: q8_8_mul(scale_x, cos),
        pb: q8_8_mul(scale_x, -sin),
        pc: q8_8_mul(scale_y, sin),
        pd: q8_8_mul(scale_y, cos),
    }
}

fn q8_8_mul(scale: i16, trig: f64) -> i16 {
    (f64::from(scale) * trig)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

fn bios_arc_tan(input: i32) -> (i16, i32, i32) {
    let square = input.wrapping_mul(input);
    let r1 = (square >> 14).wrapping_neg();
    let mut polynomial = bios_poly_mul_add(0xA9, r1, 0x390);
    polynomial = bios_poly_mul_add(polynomial, r1, 0x91C);
    polynomial = bios_poly_mul_add(polynomial, r1, 0xFB6);
    polynomial = bios_poly_mul_add(polynomial, r1, 0x16AA);
    polynomial = bios_poly_mul_add(polynomial, r1, 0x2081);
    polynomial = bios_poly_mul_add(polynomial, r1, 0x3651);
    polynomial = bios_poly_mul_add(polynomial, r1, 0xA2F9);

    let result = input.wrapping_mul(polynomial) >> 16;
    (result as i16, r1, polynomial)
}

fn bios_arc_tan2(x: i32, y: i32) -> (i16, Option<i32>) {
    if y == 0 {
        return (if x >= 0 { 0 } else { 0x8000u16 as i16 }, None);
    }
    if x == 0 {
        return (if y >= 0 { 0x4000 } else { 0xC000u16 as i16 }, None);
    }

    let mut r1 = 0;
    let result = if y >= 0 {
        if x >= 0 && x >= y {
            bios_arc_tan_result(y.wrapping_shl(14).wrapping_div(x), &mut r1)
        } else if x < 0 && x.wrapping_neg() >= y {
            bios_arc_tan_result(y.wrapping_shl(14).wrapping_div(x), &mut r1)
                .wrapping_add(0x8000u16 as i16)
        } else {
            (0x4000i16).wrapping_sub(bios_arc_tan_result(
                x.wrapping_shl(14).wrapping_div(y),
                &mut r1,
            ))
        }
    } else if x <= 0 && x.wrapping_neg() > y.wrapping_neg() {
        bios_arc_tan_result(y.wrapping_shl(14).wrapping_div(x), &mut r1)
            .wrapping_add(0x8000u16 as i16)
    } else if x > 0 && x >= y.wrapping_neg() {
        bios_arc_tan_result(y.wrapping_shl(14).wrapping_div(x), &mut r1)
    } else {
        (0xC000u16 as i16).wrapping_sub(bios_arc_tan_result(
            x.wrapping_shl(14).wrapping_div(y),
            &mut r1,
        ))
    };

    (result, Some(r1))
}

fn bios_arc_tan_result(input: i32, r1: &mut i32) -> i16 {
    let (result, next_r1, _) = bios_arc_tan(input);
    *r1 = next_r1;
    result
}

fn bios_poly_mul_add(lhs: i32, rhs: i32, add: i32) -> i32 {
    (lhs.wrapping_mul(rhs) >> 14).wrapping_add(add)
}

fn cpu_set_read32(bus: &Bus, addr: u32) -> u32 {
    if cpu_set_zero_read_addr(addr) {
        0
    } else {
        bus.read32(addr)
    }
}

fn cpu_set_read16(bus: &Bus, addr: u32) -> u16 {
    if cpu_set_zero_read_addr(addr) {
        0
    } else if addr & 1 == 0 {
        bus.read16(addr)
    } else {
        u16::from(bus.read8(addr))
    }
}

fn cpu_set_zero_read_addr(addr: u32) -> bool {
    matches!(addr, 0x0000_4000..=0x01FF_FFFF | 0x1000_0000..=0xFFFF_FFFF)
}

fn read_i16(bus: &Bus, addr: u32) -> i16 {
    bus.read16(addr) as i16
}

fn read_i32(bus: &Bus, addr: u32) -> i32 {
    bus.read32(addr) as i32
}

fn write_i16(bus: &mut Bus, addr: u32, value: i16) {
    bus.write16(addr, value as u16);
}

fn write_i32(bus: &mut Bus, addr: u32, value: i32) {
    bus.write32(addr, value as u32);
}

fn write_decompressed_output(
    bus: &mut Bus,
    mut dst: u32,
    out: &[u8],
    mode: DecompWriteMode,
) -> u32 {
    match mode {
        DecompWriteMode::Byte => {
            for &value in out {
                bus.write8(dst, value);
                dst = dst.wrapping_add(1);
            }
        }
        DecompWriteMode::Halfword => {
            for chunk in out.chunks(2) {
                let lo = chunk[0];
                let hi = chunk.get(1).copied().unwrap_or(0);
                bus.write16(dst, u16::from_le_bytes([lo, hi]));
                dst = dst.wrapping_add(2);
            }
        }
    }
    dst
}

fn write_words(bus: &mut Bus, mut dst: u32, out: &[u8]) -> u32 {
    for chunk in out.chunks(4) {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        bus.write32(dst, u32::from_le_bytes(word));
        dst = dst.wrapping_add(4);
    }
    dst
}
