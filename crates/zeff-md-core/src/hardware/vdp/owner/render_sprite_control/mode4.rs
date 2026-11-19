use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Mode4 {
    pub(super) y_delay: [bool; 2],
    pub(super) serial_y: u8,
    pub(super) scan_delay: [u8; 4],
    pub(super) row_queue: [[u8; 20]; 2],
    pub(super) serial_slot: [bool; 2],
    pub(super) pattern_slot: [bool; 2],
    pub(super) ready_delay: [bool; 2],
    pub(super) availability: [u16; 2],
    pub(super) x: u8,
}

impl Mode4 {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        scan: u16,
        slot: bool,
        pattern: bool,
        ready: bool,
        available: bool,
        append: bool,
        row: u8,
    ) {
        self.scan_delay[0] = (scan & 31) as u8;
        self.scan_delay[2] = self.scan_delay[1];
        self.serial_slot[0] = slot;
        self.pattern_slot[0] = pattern;
        self.ready_delay[0] = ready;
        self.availability[0] = ((self.availability[1] << 1) | u16::from(available)) & 1023;
        self.row_queue[0] = self.row_queue[1];
        if append {
            self.row_queue[0].copy_within(0..19, 1);
            self.row_queue[0][0] = row & 15;
        }
    }

    pub(super) fn commit(&mut self) {
        self.scan_delay[1] = self.scan_delay[0];
        self.scan_delay[3] = self.scan_delay[2];
        self.serial_slot[1] = self.serial_slot[0];
        self.pattern_slot[1] = self.pattern_slot[0];
        self.ready_delay[1] = self.ready_delay[0];
        self.availability[1] = self.availability[0];
        self.row_queue[1] = self.row_queue[0];
    }

    pub(super) fn write(&self, w: &mut StateWriter) {
        for pair in [
            self.y_delay,
            self.serial_slot,
            self.pattern_slot,
            self.ready_delay,
        ] {
            for value in pair {
                w.write_bool(value);
            }
        }
        w.write_u8(self.serial_y);
        w.write_bytes(&self.scan_delay);
        for row in self.row_queue {
            w.write_bytes(&row);
        }
        for value in self.availability {
            w.write_u16(value);
        }
        w.write_u8(self.x);
    }

    pub(super) fn read(r: &mut StateReader<'_>) -> Result<Self> {
        let mut s = Self::default();
        for pair in [
            &mut s.y_delay,
            &mut s.serial_slot,
            &mut s.pattern_slot,
            &mut s.ready_delay,
        ] {
            for value in pair {
                *value = r.read_bool()?;
            }
        }
        s.serial_y = r.read_u8()?;
        r.read_exact(&mut s.scan_delay)?;
        for row in &mut s.row_queue {
            r.read_exact(row)?;
        }
        for value in &mut s.availability {
            *value = r.read_u16()?;
        }
        s.x = r.read_u8()?;
        ensure!(
            s.scan_delay.into_iter().all(|v| v < 32)
                && s.row_queue.iter().flatten().all(|v| *v < 16)
                && s.availability.into_iter().all(|v| v < 1024),
            "invalid M4 sprite history"
        );
        Ok(s)
    }
}
