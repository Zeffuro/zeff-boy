use super::Cpu;
use crate::hardware::bus::Bus;

impl Cpu {
    pub(super) fn execute_software_interrupt(&mut self, bus: &mut Bus, function: u32) {
        match function {
            0x06 => self.swi_div(),
            0x07 => self.swi_div_arm(),
            0x08 => self.swi_sqrt(),
            0x10 => self.swi_bit_unpack(bus),
            0x11 | 0x12 => self.swi_lz77_uncomp(bus),
            0x0B => self.swi_cpu_set(bus, false),
            0x0C => self.swi_cpu_set(bus, true),
            0x14 | 0x15 => self.swi_rl_uncomp(bus),
            0x02..=0x05 => {}
            _ => {}
        }
        self.cycles = self.cycles.wrapping_add(4);
    }

    fn swi_div(&mut self) {
        let numerator = self.regs[0] as i32;
        let denominator = self.regs[1] as i32;
        if denominator == 0 {
            return;
        }
        let quotient = numerator.wrapping_div(denominator);
        let remainder = numerator.wrapping_rem(denominator);
        self.regs[0] = quotient as u32;
        self.regs[1] = remainder as u32;
        self.regs[3] = quotient.wrapping_abs() as u32;
    }

    fn swi_div_arm(&mut self) {
        self.regs.swap(0, 1);
        self.swi_div();
    }

    fn swi_sqrt(&mut self) {
        self.regs[0] = (self.regs[0] as f64).sqrt() as u32;
    }

    fn swi_cpu_set(&mut self, bus: &mut Bus, fast: bool) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let mode = self.regs[2];
        let fixed_source = mode & (1 << 24) != 0;
        let word = fast || mode & (1 << 26) != 0;
        let count = mode & 0x1F_FFFF;
        if word {
            let fill = bus.read32(src);
            let units = if fast { (count + 7) & !7 } else { count };
            for _ in 0..units {
                let value = if fixed_source { fill } else { bus.read32(src) };
                bus.write32(dst, value);
                if !fixed_source {
                    src = src.wrapping_add(4);
                }
                dst = dst.wrapping_add(4);
            }
        } else {
            let fill = bus.read16(src);
            for _ in 0..count {
                let value = if fixed_source { fill } else { bus.read16(src) };
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

    fn swi_lz77_uncomp(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
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
        for value in out {
            bus.write8(dst, value);
            dst = dst.wrapping_add(1);
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }

    fn swi_rl_uncomp(&mut self, bus: &mut Bus) {
        let mut src = self.regs[0];
        let mut dst = self.regs[1];
        let header = bus.read32(src);
        src = src.wrapping_add(4);
        if header & 0xFF != 0x30 {
            return;
        }
        let out_len = (header >> 8) as usize;
        let mut written = 0usize;
        while written < out_len {
            let control = bus.read8(src);
            src = src.wrapping_add(1);
            if control & 0x80 != 0 {
                let count = usize::from(control & 0x7F) + 3;
                let value = bus.read8(src);
                src = src.wrapping_add(1);
                for _ in 0..count {
                    if written >= out_len {
                        break;
                    }
                    bus.write8(dst, value);
                    dst = dst.wrapping_add(1);
                    written += 1;
                }
            } else {
                let count = usize::from(control) + 1;
                for _ in 0..count {
                    if written >= out_len {
                        break;
                    }
                    let value = bus.read8(src);
                    src = src.wrapping_add(1);
                    bus.write8(dst, value);
                    dst = dst.wrapping_add(1);
                    written += 1;
                }
            }
        }
        self.regs[0] = src;
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
                    bus.write8(dst, accum as u8);
                    dst = dst.wrapping_add(1);
                    accum >>= 8;
                    accum_bits -= 8;
                }
                bit += source_width;
            }
        }
        if accum_bits != 0 {
            bus.write8(dst, accum as u8);
            dst = dst.wrapping_add(1);
        }
        self.regs[0] = src;
        self.regs[1] = dst;
    }
}
