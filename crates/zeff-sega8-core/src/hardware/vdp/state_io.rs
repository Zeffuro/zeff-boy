use super::*;

impl Vdp {
    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        w.write_vec(&self.vram);
        w.write_vec(&self.cram);
        w.write_vec(&self.registers);
        w.write_u16(self.address);
        w.write_u8(self.code);
        match self.control_latch {
            Some(value) => {
                w.write_bool(true);
                w.write_u8(value);
            }
            None => {
                w.write_bool(false);
                w.write_u8(0);
            }
        }
        w.write_u8(self.read_buffer);
        w.write_u8(self.status);
        w.write_u8(self.v_counter);
        w.write_u8(self.h_counter);
        w.write_u16(self.scanline);
        w.write_u32(self.scanline_cycle);
        for &enabled in &self.scanline_display_enabled {
            w.write_bool(enabled);
        }
        w.write_u8(self.line_counter);
        w.write_bool(self.line_interrupt_pending);
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
        has_scanline_display_history: bool,
    ) -> anyhow::Result<()> {
        read_fixed_vec(r, &mut self.vram, SMS_VRAM_SIZE, "VDP VRAM")?;
        read_fixed_vec(r, &mut self.cram, SMS_CRAM_SIZE, "VDP CRAM")?;
        read_fixed_vec(
            r,
            &mut self.registers,
            SMS_VDP_REGISTER_COUNT,
            "VDP registers",
        )?;
        self.address = r.read_u16()? & VDP_ADDRESS_MASK;
        self.code = r.read_u8()? & 0x03;
        self.control_latch = if r.read_bool()? {
            Some(r.read_u8()?)
        } else {
            let _unused = r.read_u8()?;
            None
        };
        self.read_buffer = r.read_u8()?;
        self.status = r.read_u8()?;
        self.v_counter = r.read_u8()?;
        self.h_counter = r.read_u8()?;
        self.scanline = r.read_u16()? % self.total_scanlines();
        self.scanline_cycle = r.read_u32()? % SMS_SCANLINE_Z80_CYCLES;
        if has_scanline_display_history {
            for enabled in &mut self.scanline_display_enabled {
                *enabled = r.read_bool()?;
            }
        } else {
            let display_enabled = self.display_enabled();
            self.scanline_display_enabled.fill(display_enabled);
        }
        self.clear_presented_frame_history();
        self.scanline_start_registers = self.registers;
        self.line_counter = r.read_u8()?;
        self.line_interrupt_pending = r.read_bool()?;
        Ok(())
    }
}

fn read_fixed_vec(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
    out: &mut [u8],
    expected_len: usize,
    label: &str,
) -> anyhow::Result<()> {
    let bytes = r.read_vec(expected_len)?;
    if bytes.len() != expected_len {
        anyhow::bail!(
            "Sega 8-bit save-state {label} size mismatch: expected {expected_len}, got {}",
            bytes.len()
        );
    }
    out.copy_from_slice(&bytes);
    Ok(())
}
