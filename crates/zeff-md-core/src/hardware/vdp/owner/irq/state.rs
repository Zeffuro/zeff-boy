use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

impl IrqCone {
    pub fn write_state(&self, w: &mut StateWriter) {
        for count in self.count {
            w.write_u8(count as u8);
        }
        for pair in [
            self.overflow,
            self.ack_delay,
            self.a,
            self.b,
            self.ack_external,
            self.ack_h,
            self.ack_v,
            self.horizontal_zero,
            self.vertical_zero,
            self.vertical_last,
        ] {
            for value in pair {
                w.write_bool(value);
            }
        }
        for value in [
            self.t5,
            self.l10,
            self.l11,
            self.t6,
            self.clear,
            self.hp,
            self.vp,
            self.sound,
            self.priority_h,
            self.priority_v,
            self.ack_reset,
            self.vint_set,
            self.active,
        ] {
            w.write_bool(value);
        }
    }

    pub fn read_state(r: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let mut value = Self::default();
        for count in &mut value.count {
            *count = i32::from(r.read_u8()?);
        }
        for pair in [
            &mut value.overflow,
            &mut value.ack_delay,
            &mut value.a,
            &mut value.b,
            &mut value.ack_external,
            &mut value.ack_h,
            &mut value.ack_v,
            &mut value.horizontal_zero,
            &mut value.vertical_zero,
            &mut value.vertical_last,
        ] {
            for item in pair {
                *item = r.read_bool()?;
            }
        }
        for item in [
            &mut value.t5,
            &mut value.l10,
            &mut value.l11,
            &mut value.t6,
            &mut value.clear,
            &mut value.hp,
            &mut value.vp,
            &mut value.sound,
            &mut value.priority_h,
            &mut value.priority_v,
            &mut value.ack_reset,
            &mut value.vint_set,
            &mut value.active,
        ] {
            *item = r.read_bool()?;
        }
        Ok(value)
    }
}
