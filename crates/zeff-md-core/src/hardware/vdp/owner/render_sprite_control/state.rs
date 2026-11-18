use super::*;

impl SpriteControl {
    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(4);
        self.cells.write(w);
        self.mode4.write(w);
        self.sat.write_state(w);
        for known in self.sat_known {
            w.write_bool(known);
        }
        w.write_bool(self.complement_known);
        for queue in self.queue {
            w.write_bytes(&queue);
        }
        self.descriptor_reads.write_state(w);
        self.selected.write(w);
        for v in [
            self.complement_size,
            self.captured_size,
            self.complement_y,
            self.subtract_y,
            self.captured_y,
            self.delayed_y,
            self.delta,
        ] {
            w.write_u16(v);
        }
        for v in [
            self.serial_y_lead,
            self.size_capture,
            self.stopped,
            self.overflow,
            self.overflow_output,
            self.drawing,
            self.zero_x,
            self.stride_enable,
            self.stride_step,
        ] {
            w.write_bool(v);
        }
        w.write_bytes(&self.stride);
        w.write_u8(self.size);
        w.write_u8(self.row);
        for v in self.serial_lead {
            w.write_bool(v);
        }
        for pair in self.serial_delay {
            for v in pair {
                w.write_bool(v);
            }
        }
        w.write_bytes(&self.attributes);
        w.write_bytes(&self.pattern);
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(r.read_u8()? == 4, "unsupported sprite control state");
        let mut s = Self::software_origin();
        s.cells = Cells::read(r)?;
        s.mode4 = Mode4::read(r)?;
        s.sat = SpriteAttributes::read_state(r)?;
        s.sat_known = [r.read_bool()?, r.read_bool()?];
        s.complement_known = r.read_bool()?;
        for q in &mut s.queue {
            r.read_exact(q)?;
        }
        s.descriptor_reads = FunctionalReadResponder::read_state(r)?;
        s.selected = FetchWord::read(r)?;
        for v in [
            &mut s.complement_size,
            &mut s.captured_size,
            &mut s.complement_y,
            &mut s.subtract_y,
            &mut s.captured_y,
            &mut s.delayed_y,
            &mut s.delta,
        ] {
            *v = r.read_u16()?;
        }
        for v in [
            &mut s.serial_y_lead,
            &mut s.size_capture,
            &mut s.stopped,
            &mut s.overflow,
            &mut s.overflow_output,
            &mut s.drawing,
            &mut s.zero_x,
            &mut s.stride_enable,
            &mut s.stride_step,
        ] {
            *v = r.read_bool()?;
        }
        r.read_exact(&mut s.stride)?;
        s.size = r.read_u8()?;
        s.row = r.read_u8()?;
        for v in &mut s.serial_lead {
            *v = r.read_bool()?;
        }
        for pair in &mut s.serial_delay {
            for v in pair {
                *v = r.read_bool()?;
            }
        }
        r.read_exact(&mut s.attributes)?;
        r.read_exact(&mut s.pattern)?;
        ensure!(
            s.complement_size < 2048
                && s.captured_size < 2048
                && s.complement_y < 1024
                && s.subtract_y < 1024
                && s.captured_y < 1024
                && s.delayed_y < 1024
                && s.delta < 1024
                && s.size < 16
                && s.row < 64
                && s.stride.into_iter().all(|v| v < 16),
            "invalid sprite control captures"
        );
        for (phase, known) in s.sat_known.into_iter().enumerate() {
            ensure!(
                known || s.cells.size_word[phase] == 0 && s.cells.ypos[phase] == 0,
                "noncanonical unknown SAT capture"
            );
        }
        ensure!(
            s.complement_known || s.complement_size == 0 && s.complement_y == 0,
            "noncanonical unknown SAT complement"
        );
        Ok(s)
    }
}
