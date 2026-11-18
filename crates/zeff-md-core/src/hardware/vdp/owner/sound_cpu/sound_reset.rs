use super::super::sound_clock::SoundClock;
use super::SoundPins;
use super::m1::RunningM1;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SoundReset {
    initial: bool,
    asserted_at: u64,
    widths: Option<(u64, u64)>,
    clock_high: bool,
    request: bool,
    reset: bool,
    reset_low: bool,
    reset_high: bool,
    internal: bool,
    reset_copy_high: bool,
    reset_copy_low: bool,
    request_low: bool,
    request_high: bool,
    grant: bool,
    grant_gate: bool,
    ack: bool,
    ack_high: bool,
    running: Option<RunningM1>,
    waiting_repeat: bool,
}

impl SoundReset {
    pub fn held_origin() -> Self {
        Self {
            initial: true,
            asserted_at: 0,
            widths: None,
            clock_high: false,
            request: false,
            reset: true,
            reset_low: true,
            reset_high: true,
            internal: true,
            reset_copy_high: true,
            reset_copy_low: true,
            request_low: false,
            request_high: false,
            grant: false,
            grant_gate: false,
            ack: false,
            ack_high: false,
            running: None,
            waiting_repeat: false,
        }
    }

    pub fn granted(clock: &SoundClock) -> Result<Self> {
        let widths = clock.widths();
        ensure!(
            widths.is_some(),
            "RESET has no qualified SOUND clock period"
        );
        Ok(Self {
            initial: false,
            asserted_at: clock.half(),
            widths,
            clock_high: clock.high(),
            request: true,
            reset: false,
            reset_low: false,
            reset_high: false,
            internal: false,
            reset_copy_high: false,
            reset_copy_low: false,
            request_low: true,
            request_high: true,
            grant: true,
            grant_gate: false,
            ack: true,
            ack_high: true,
            running: None,
            waiting_repeat: false,
        })
    }

    pub fn late_repeat(clock: &SoundClock) -> Result<Self> {
        let mut value = Self::granted(clock)?;
        value.request = false;
        value.request_low = false;
        value.request_high = false;
        value.grant = false;
        value.ack = false;
        value.ack_high = false;
        value.waiting_repeat = true;
        Ok(value)
    }
    pub fn waiting_repeat(&self) -> bool {
        self.waiting_repeat
    }
    pub fn is_repeat_origin(&self) -> bool {
        self.running.as_ref().is_some_and(|r| r.repeat_origin)
    }
    pub fn repeat_origin_matches(&self, cells: &super::m1::M1Cells) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| r.repeat_origin_matches(cells))
    }
    pub fn finish_repeat(&mut self, running: RunningM1) -> Result<()> {
        ensure!(
            self.waiting_repeat
                && !self.clock_high
                && self.reset
                && self.reset_low
                && !self.reset_high
                && !self.internal
                && !self.reset_copy_high
                && !self.reset_copy_low
                && !self.ack
                && !self.ack_high
                && running.repeat_origin,
            "late repeat RESET retirement aperture differs"
        );
        self.waiting_repeat = false;
        self.running = Some(running);
        Ok(())
    }
    pub fn running(clock: &SoundClock, running: RunningM1) -> Result<Self> {
        ensure!(!clock.high(), "running RESET entry is not low");
        let mut value = Self::granted(clock)?;
        value.request = false;
        value.request_low = false;
        value.request_high = false;
        value.grant = false;
        value.ack = false;
        value.ack_high = false;
        value.running = Some(running);
        Ok(value)
    }

    pub fn running_plan(&self) -> Option<super::super::sound_semantics::Plan> {
        self.running.as_ref().map(|r| r.plan)
    }

    pub fn pins(&self) -> SoundPins {
        self.running.as_ref().map_or(
            SoundPins {
                address: None,
                data: None,
                mreq: false,
                read: false,
                write: false,
                m1: false,
                refresh: false,
                iorq: false,
                bus_ack: self.ack,
            },
            |r| r.cells.pins(),
        )
    }
    pub fn initial(&self) -> bool {
        self.initial
    }

    pub fn observe(
        &mut self,
        clock: &SoundClock,
        request: bool,
        reset: bool,
        wait: bool,
    ) -> Result<bool> {
        ensure!(
            self.running.is_none() && !self.waiting_repeat || !request && !wait,
            "BUSREQ or WAIT collided with interrupted local M1"
        );
        if self.widths.is_none() {
            self.widths = clock.widths();
        }
        self.validate_clock(clock)?;
        let released = self.reset && !reset;
        if released {
            let (high, low) = self.widths.ok_or_else(|| {
                anyhow::anyhow!("RESET released before SOUND clock qualification")
            })?;
            let minimum = high
                .checked_add(low)
                .and_then(|n| n.checked_mul(3))
                .ok_or_else(|| anyhow::anyhow!("SOUND reset width overflow"))?;
            ensure!(
                clock.half() - self.asserted_at >= minimum,
                "SOUND RESET pulse shorter than three complete clocks"
            );
            ensure!(
                self.internal
                    && self.reset_copy_high
                    && self.reset_copy_low
                    && !self.ack
                    && !self.ack_high,
                "SOUND RESET released before the held-reset state"
            );
        }
        ensure!(
            !reset
                || self.reset
                || !self.initial
                    && !self.internal
                    && (self.ack || self.running.is_some() || self.waiting_repeat),
            "SOUND RESET reasserted during release"
        );
        if (self.clock_high, self.request, self.reset) != (clock.high(), request, reset) {
            self.clock_high = clock.high();
            self.request = request;
            self.reset = reset;
            for _ in 0..2 {
                self.settle();
            }
        }
        ensure!(
            !reset
                || self.internal
                || self.grant
                || self.reset_copy_low
                || self.running.is_some()
                || self.waiting_repeat,
            "SOUND grant ended before RESET inhibition"
        );
        if self.running.as_ref().is_some_and(|r| r.cells.quiescent()) {
            ensure!(
                self.internal && self.reset_copy_low && self.clock_high,
                "M1 pins quiesced before reset inhibition"
            );
            self.running = None;
        }
        Ok(released)
    }

    fn settle(&mut self) {
        if self.clock_high {
            self.reset_high = self.reset_low;
            self.request_high = self.request_low;
        } else {
            self.reset_low = self.reset;
            if self.running.as_ref().is_none_or(|r| r.cells.reset_enabled) {
                self.internal = self.reset_high;
            }
            self.reset_copy_low = self.reset_copy_high;
        }
        let set_grant = !self.clock_high && self.grant_gate && self.request_high;
        let clear_request = !self.clock_high && !self.request_high;
        if self.reset_copy_low || clear_request {
            self.grant = false;
        }
        if set_grant {
            self.grant = true;
        }
        if self.clock_high {
            self.reset_copy_high = self.internal;
        } else {
            self.request_low = self.request;
        }
        self.ack = !set_grant && self.grant && !clear_request;
        if self.clock_high {
            self.ack_high = self.ack;
            self.grant_gate = !self.reset_copy_low && self.internal;
        }
        let blocked = self.grant || self.reset_copy_low || set_grant;
        if let Some(running) = &mut self.running {
            running.settle(self.clock_high, self.internal, blocked);
        }
    }

    pub fn ready(&self) -> bool {
        self.running.is_none()
            && !self.waiting_repeat
            && !self.clock_high
            && !self.reset
            && !self.internal
            && !self.reset_copy_low
            && !self.grant
            && !self.ack_high
    }

    fn validate_clock(&self, clock: &SoundClock) -> Result<()> {
        ensure!(
            self.asserted_at <= clock.half(),
            "future SOUND reset assertion"
        );
        if let Some(widths) = self.widths {
            ensure!(
                clock.widths() == Some(widths),
                "SOUND clock changed during RESET"
            );
            let phase_width = if clock.high() { widths.0 } else { widths.1 };
            ensure!(
                clock.phase_age() <= phase_width,
                "SOUND clock stalled during RESET"
            );
        } else {
            ensure!(
                clock.widths().is_none(),
                "missing observed SOUND reset period"
            );
        }
        Ok(())
    }

    pub fn validate(&self, clock: &SoundClock, request: bool, reset: bool) -> Result<()> {
        self.validate_clock(clock)?;
        ensure!(
            (self.clock_high, self.request, self.reset) == (clock.high(), request, reset),
            "SOUND reset inputs differ from observed pins"
        );
        ensure!(
            !self.initial || self.asserted_at == 0,
            "invalid initial SOUND reset origin"
        );
        ensure!(
            self.initial || self.asserted_at != 0 && self.widths.is_some(),
            "invalid runtime SOUND reset origin"
        );
        if !self.reset {
            let (high, low) = self
                .widths
                .ok_or_else(|| anyhow::anyhow!("released SOUND reset has no period"))?;
            let minimum = high
                .checked_add(low)
                .and_then(|n| n.checked_mul(3))
                .ok_or_else(|| anyhow::anyhow!("SOUND reset width overflow"))?;
            ensure!(
                clock.half() - self.asserted_at >= minimum,
                "unqualified saved SOUND RESET release"
            );
        }
        let mut settled = self.clone();
        for _ in 0..2 {
            settled.settle();
        }
        ensure!(settled == *self, "unsettled SOUND reset cone");
        ensure!(!self.ready(), "retired SOUND reset cone retained");
        if self.waiting_repeat {
            ensure!(
                self.clock_high
                    && self.reset
                    && !self.initial
                    && !self.request
                    && !self.internal
                    && !self.reset_low
                    && !self.reset_high
                    && !self.reset_copy_high
                    && !self.reset_copy_low
                    && !self.ack
                    && !self.ack_high
                    && self.running.is_none(),
                "invalid pending repeat RESET"
            );
        }
        if let Some(running) = &self.running {
            ensure!(
                self.reset
                    && !self.initial
                    && !self.request
                    && !self.ack
                    && !self.ack_high
                    && running.cancelled == self.internal,
                "invalid interrupted M1 reset state"
            );
            running.validate(
                self.clock_high,
                self.internal,
                self.reset_copy_high,
                self.reset_copy_low,
            )?;
        }
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bool(self.initial);
        w.write_u64(self.asserted_at);
        w.write_bool(self.widths.is_some());
        if let Some((high, low)) = self.widths {
            w.write_u64(high);
            w.write_u64(low);
        }
        for value in [
            self.clock_high,
            self.request,
            self.reset,
            self.reset_low,
            self.reset_high,
            self.internal,
            self.reset_copy_high,
            self.reset_copy_low,
            self.request_low,
            self.request_high,
            self.grant,
            self.grant_gate,
            self.ack,
            self.ack_high,
        ] {
            w.write_bool(value);
        }
        w.write_bool(self.waiting_repeat);
        w.write_bool(self.running.is_some());
        if let Some(running) = &self.running {
            running.write_state(w);
        }
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let initial = r.read_bool()?;
        let asserted_at = r.read_u64()?;
        let widths = if r.read_bool()? {
            let pair = (r.read_u64()?, r.read_u64()?);
            ensure!(pair.0 != 0 && pair.1 != 0, "invalid SOUND reset period");
            Some(pair)
        } else {
            None
        };
        Ok(Self {
            initial,
            asserted_at,
            widths,
            clock_high: r.read_bool()?,
            request: r.read_bool()?,
            reset: r.read_bool()?,
            reset_low: r.read_bool()?,
            reset_high: r.read_bool()?,
            internal: r.read_bool()?,
            reset_copy_high: r.read_bool()?,
            reset_copy_low: r.read_bool()?,
            request_low: r.read_bool()?,
            request_high: r.read_bool()?,
            grant: r.read_bool()?,
            grant_gate: r.read_bool()?,
            ack: r.read_bool()?,
            ack_high: r.read_bool()?,
            waiting_repeat: r.read_bool()?,
            running: if r.read_bool()? {
                Some(RunningM1::read_state(r)?)
            } else {
                None
            },
        })
    }
}
