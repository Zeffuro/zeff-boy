use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

fn sample_write(s: core::Sample, w: &mut StateWriter) {
    w.write_u16(s.v);
    w.write_u8(s.c);
    for value in [
        s.select, s.reload, s.q, s.z, s.last, s.h, s.e, s.overflow, s.a, s.b,
    ] {
        w.write_bool(value);
    }
}
fn sample_read(r: &mut StateReader<'_>) -> Result<core::Sample> {
    Ok(core::Sample {
        v: r.read_u16()?,
        c: r.read_u8()?,
        select: r.read_bool()?,
        reload: r.read_bool()?,
        q: r.read_bool()?,
        z: r.read_bool()?,
        last: r.read_bool()?,
        h: r.read_bool()?,
        e: r.read_bool()?,
        overflow: r.read_bool()?,
        a: r.read_bool()?,
        b: r.read_bool()?,
    })
}

impl BlankHistory {
    pub fn write_state(self, w: &mut StateWriter) {
        for value in [
            self.active,
            self.eligible,
            self.port_ready,
            self.cancel_control,
        ] {
            w.write_bool(value);
        }
        for value in [self.now, self.next_clock] {
            w.write_u64(value.get());
        }
        w.write_bool(self.last_release.is_some());
        if let Some(at) = self.last_release {
            w.write_u64(at.get());
        }
        w.write_bool(self.port.is_some());
        if let Some(p) = self.port {
            w.write_u8(if p.kind == PortKind::Status { 0 } else { 1 });
            for value in [p.begin, p.assertion, p.sample, p.end] {
                w.write_u64(value.get());
            }
            w.write_bool(p.staged);
            w.write_bool(p.captured.is_some());
            if let Some(v) = p.captured {
                w.write_u16(v);
            }
        }
        let c = self.core;
        for at in [c.epoch, c.updated_at, c.field_start, c.line_start] {
            w.write_u64(at.get());
        }
        w.write_u64(c.field_id);
        w.write_u16(c.v);
        w.write_u16(c.row);
        w.write_u8(c.c);
        for value in [
            c.select,
            c.reload,
            c.q,
            c.z,
            c.last,
            c.h,
            c.e,
            c.overflow,
            c.active,
            c.hp,
            c.vp,
            c.sound,
            c.t6,
            c.a,
            c.b,
            c.clear,
            c.wrap,
            c.coverage,
            c.canonical,
        ] {
            w.write_bool(value);
        }
        sample_write(c.prepared, w);
        self.write_bridge(w);
        self.write_vertical(w);
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let active = r.read_bool()?;
        let eligible = r.read_bool()?;
        let port_ready = r.read_bool()?;
        let cancel_control = r.read_bool()?;
        let now = MasterTicks::new(r.read_u64()?);
        let next_clock = MasterTicks::new(r.read_u64()?);
        let last_release = if r.read_bool()? {
            Some(MasterTicks::new(r.read_u64()?))
        } else {
            None
        };
        let port = if r.read_bool()? {
            let kind = match r.read_u8()? {
                0 => PortKind::Status,
                1 => PortKind::Control,
                _ => anyhow::bail!("invalid VDP port kind"),
            };
            let begin = MasterTicks::new(r.read_u64()?);
            let assertion = MasterTicks::new(r.read_u64()?);
            let sample = MasterTicks::new(r.read_u64()?);
            let end = MasterTicks::new(r.read_u64()?);
            let staged = r.read_bool()?;
            let captured = if r.read_bool()? {
                Some(r.read_u16()?)
            } else {
                None
            };
            Some(PortCycle {
                kind,
                begin,
                assertion,
                sample,
                end,
                staged,
                captured,
            })
        } else {
            None
        };
        let epoch = MasterTicks::new(r.read_u64()?);
        let updated_at = MasterTicks::new(r.read_u64()?);
        let field_start = MasterTicks::new(r.read_u64()?);
        let line_start = MasterTicks::new(r.read_u64()?);
        let field_id = r.read_u64()?;
        let v = r.read_u16()?;
        let row = r.read_u16()?;
        let c = r.read_u8()?;
        let core = core::Core {
            epoch,
            updated_at,
            field_start,
            line_start,
            field_id,
            v,
            row,
            c,
            select: r.read_bool()?,
            reload: r.read_bool()?,
            q: r.read_bool()?,
            z: r.read_bool()?,
            last: r.read_bool()?,
            h: r.read_bool()?,
            e: r.read_bool()?,
            overflow: r.read_bool()?,
            active: r.read_bool()?,
            hp: r.read_bool()?,
            vp: r.read_bool()?,
            sound: r.read_bool()?,
            t6: r.read_bool()?,
            a: r.read_bool()?,
            b: r.read_bool()?,
            clear: r.read_bool()?,
            wrap: r.read_bool()?,
            coverage: r.read_bool()?,
            canonical: r.read_bool()?,
            prepared: sample_read(r)?,
        };
        let mut value = Self {
            active,
            eligible,
            port_ready,
            cancel_control,
            now,
            next_clock,
            last_release,
            port,
            data_inflight: None,
            quiet_since: None,
            control_completed: None,
            core,
            vertical: None,
        };
        {
            value.read_bridge(r)?;
        }
        {
            value.read_vertical(r)?;
        }
        value.validate(now)?;
        Ok(value)
    }
    pub fn validate(&self, now: MasterTicks) -> Result<()> {
        let c = self.core;
        ensure!(
            self.now == now && c.updated_at == now,
            "VDP blank history clock mismatch"
        );
        ensure!(c.v < 512 && c.prepared.v < 512, "invalid VDP blank counter");
        ensure!(
            c.epoch <= c.field_start && c.field_start <= c.line_start && c.line_start <= now,
            "invalid VDP blank field origin"
        );
        if self.vertical.is_some() {
            self.validate_vertical()?;
        } else {
            ensure!(
                (c.line_start.get() - c.epoch.get()).is_multiple_of(H32_LINE_TICKS),
                "invalid VDP blank horizontal origin"
            );
            ensure!(
                now.get() - c.line_start.get() < H32_LINE_TICKS,
                "invalid VDP blank line phase"
            );
            ensure!(
                now.get()
                    .checked_add(H32_DOT_TICKS - (now.get() - c.epoch.get()) % H32_DOT_TICKS)
                    == Some(self.next_clock.get()),
                "invalid VDP blank clock deadline"
            );
            ensure!(
                u64::from(c.row).checked_mul(H32_LINE_TICKS)
                    == Some(c.line_start.get() - c.field_start.get()),
                "invalid VDP blank row"
            );
        }
        ensure!(
            self.authoritative() || self.eligible,
            "retired inactive VDP history"
        );
        ensure!(c.clear == (c.b && !c.a), "invalid VDP status clear phase");
        ensure!(!self.active || self.eligible, "retired VDP blank history");
        ensure!(!c.coverage || c.canonical, "invalid VDP blank coverage");
        self.validate_bridge(now)?;
        ensure!(
            self.last_release.is_none_or(|at| at <= now),
            "VDP port release in future"
        );
        if let Some(p) = self.port {
            ensure!(
                (self.port_ready || self.quiet_since.is_some()) && p.begin <= now && now <= p.end,
                "invalid VDP timed port lifetime"
            );
            ensure!(
                p.begin.get().checked_add(28) == Some(p.end.get())
                    && p.end.get().checked_sub(3) == Some(p.sample.get()),
                "invalid VDP timed port span"
            );
            ensure!(
                p.begin
                    .get()
                    .checked_add(if p.kind == PortKind::Status { 7 } else { 14 })
                    == Some(p.assertion.get()),
                "invalid VDP port assertion"
            );
            ensure!(
                p.kind == PortKind::Status || p.captured.is_none(),
                "control port has status data"
            );
            ensure!(
                p.kind != PortKind::Status || p.captured.is_some() == (now >= p.sample),
                "invalid VDP status capture"
            );
            ensure!(
                !self.authoritative() || p.staged,
                "missing staged VDP capture owner"
            );
            ensure!(
                self.last_release.is_none_or(|release| release <= p.begin
                    && release
                        .get()
                        .checked_add(10)
                        .is_some_and(|ready| p.assertion.get() >= ready)),
                "invalid VDP port ready history"
            );
        }
        Ok(())
    }
    pub fn validate_registers(
        &self,
        registers: &VdpRegisterPipeline,
        region: Region,
    ) -> Result<()> {
        ensure!(
            [0, 1, 10, 12]
                .into_iter()
                .all(|i| registers.registers()[i].is_some()),
            "unknown VDP blank history input"
        );
        if self.vertical.is_some() {
            ensure!(
                self.vertical_supported(registers, region),
                "invalid M5 vertical inputs"
            );
            return self.validate_vertical_registers(registers);
        }
        if self.data_inflight.is_some() || self.quiet_since.is_some() {
            ensure!(
                !self.active
                    && self.eligible
                    && registers.registers()[0].is_some_and(|r| r & 1 == 0)
                    && registers.registers()[1].is_some_and(
                        |r| r & MODE5_ENABLE != 0 && r & (V30_ENABLE | VRAM_128K_ENABLE) == 0
                    )
                    && registers.registers()[12] == Some(0),
                "unsupported VDP data bridge registers"
            );
            ensure!(
                !registers
                    .pending()
                    .is_some_and(|w| w.index == MODE_REGISTER && w.value & MODE5_ENABLE == 0),
                "VDP activation overlaps data bridge"
            );
        }
        ensure!(
            registers.registers()[1].unwrap() & MODE5_ENABLE != 0
                || !self.core.coverage && !self.core.canonical,
            "invalid M4 presentation coverage"
        );
        let mut expected = self.core;
        expected.prepare(region, registers, self.core.h_counter(self.now));
        let mut sample = self.core.prepared;
        sample.a = expected.prepared.a;
        ensure!(
            sample == expected.prepared,
            "invalid prepared VDP blank stage"
        );
        Ok(())
    }
}
