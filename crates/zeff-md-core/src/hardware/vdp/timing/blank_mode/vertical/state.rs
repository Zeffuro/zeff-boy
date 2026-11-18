use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

impl BlankHistory {
    pub fn validate_vertical_registers(&self, registers: &VdpRegisterPipeline) -> Result<()> {
        let v = self.vertical.unwrap();
        if self.now == v.line_start {
            ensure!(
                VerticalClock::selected(registers).ok() == Some(v.clock),
                "M5 vertical HC0 clock differs from published selection"
            );
        }
        if v.prepared_at == self.now {
            ensure!(
                registers.registers()[1] == Some(v.prepared_mode)
                    && registers.registers()[10] == Some(v.prepared_hint),
                "M5 vertical prepared inputs differ"
            );
        }
        self.validate_vertical_sample()
    }
    pub fn write_vertical(&self, w: &mut StateWriter) {
        w.write_bool(self.vertical.is_some());
        if let Some(v) = self.vertical {
            w.write_u8(if v.clock == LineClock::H32 { 0 } else { 1 });
            w.write_u64(v.line_start.get());
            w.write_u16(v.h32);
            w.write_u16(v.h40);
            w.write_u8(v.prepared_mode);
            w.write_u8(v.prepared_hint);
            w.write_u64(v.prepared_at.get());
        }
    }

    pub fn read_vertical(&mut self, r: &mut StateReader<'_>) -> Result<()> {
        self.vertical = if r.read_bool()? {
            Some(VerticalClock {
                clock: match r.read_u8()? {
                    0 => LineClock::H32,
                    1 => LineClock::InternalH40,
                    _ => anyhow::bail!("invalid M5 vertical line clock"),
                },
                line_start: MasterTicks::new(r.read_u64()?),
                h32: r.read_u16()?,
                h40: r.read_u16()?,
                prepared_mode: r.read_u8()?,
                prepared_hint: r.read_u8()?,
                prepared_at: MasterTicks::new(r.read_u64()?),
            })
        } else {
            None
        };
        Ok(())
    }

    pub fn validate_vertical(&self) -> Result<()> {
        let v = self.vertical.unwrap();
        let c = self.core;
        ensure!(
            !self.active && !self.eligible && c.row < 512,
            "invalid M5 vertical owner"
        );
        ensure!(
            v.line_start == c.line_start && v.h32.checked_add(v.h40) == Some(c.row),
            "M5 vertical line identity mismatch"
        );
        ensure!(
            u64::from(v.h32)
                .checked_mul(H32_LINE_TICKS)
                .and_then(|ticks| ticks.checked_add(
                    u64::from(v.h40) * super::super::super::clock::INTERNAL_LINE_TICKS
                ))
                .and_then(|ticks| c.field_start.get().checked_add(ticks))
                == Some(c.line_start.get()),
            "M5 vertical field duration mismatch"
        );
        ensure!(
            self.now.get() - v.line_start.get() < v.clock.ticks(),
            "invalid M5 vertical phase"
        );
        ensure!(
            v.prepared_at >= v.line_start
                && v.prepared_at <= self.now
                && (v.prepared_at.get() - v.line_start.get()).is_multiple_of(v.dot())
                && v.prepared_at.get().checked_add(v.dot()) == Some(self.next_clock.get())
                && self.now < self.next_clock,
            "invalid M5 vertical prepared clock"
        );
        let elapsed = c.field_start.get() - c.epoch.get();
        let minimum = 262 * super::super::super::clock::INTERNAL_LINE_TICKS;
        let maximum = 512 * H32_LINE_TICKS;
        ensure!(
            elapsed.div_ceil(maximum) <= c.field_id && c.field_id <= elapsed / minimum,
            "invalid M5 vertical field identity"
        );
        ensure!(
            v.prepared_mode & (MODE5_ENABLE | VRAM_128K_ENABLE) == MODE5_ENABLE,
            "invalid M5 vertical prepared mode"
        );
        self.validate_vertical_sample()
    }

    pub fn validate_vertical_sample(&self) -> Result<()> {
        let v = self.vertical.unwrap();
        let c = self.core;
        let p = c.prepared;
        let h = v
            .counter(v.prepared_at)
            .map_err(|e| anyhow::anyhow!("invalid M5 prepare phase: {e:?}"))?;
        let wide = v.clock != LineClock::H32;
        let v30 = v.prepared_mode & V30_ENABLE != 0;
        let next_v = if c.select && c.reload {
            if v30 { 469 } else { 485 }
        } else {
            (c.v + u16::from(c.select)) & 511
        };
        let active = c.active || c.last;
        let base = if !active || c.overflow {
            !v.prepared_hint
        } else {
            c.c
        };
        let total = u16::from(base) + u16::from(c.select);
        ensure!(
            p.v == next_v
                && p.select == (h == if wide { 328 } else { 264 })
                && p.reload == (!v30 && c.v == 234)
                && p.q == (c.v == if v30 { 240 } else { 224 })
                && p.z == (c.v == 0)
                && p.last == (c.v == 511)
                && p.h == (h == 0)
                && p.e == (h == if wide { 322 } else { 258 })
                && p.c == total as u8
                && p.overflow == (active && total > 255)
                && p.b == c.a,
            "invalid M5 vertical prepared sample"
        );
        Ok(())
    }
}
