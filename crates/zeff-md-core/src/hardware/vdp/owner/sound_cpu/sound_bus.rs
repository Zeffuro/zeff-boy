use super::super::board::SoundPins;
use super::sound_clock::Edge;
use super::sound_semantics::{CopyPhase, JrPhase, Plan, Shape};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Ready,
    Active,
    GrantPending,
    Granted,
    ReturnHigh,
    ReturnLow,
    ResetReady,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SoundBus {
    pub phase: Phase,
    pub plan: Option<Plan>,
    pub t: u32,
    pub high: bool,
    pub waiting: bool,
    pub waits: u32,
    pub sample: Option<u8>,
    pub grant_pending: bool,
    pub held_address: u16,
    pub word_pending: bool,
    pub jr_pending: bool,
    pub djnz_pending: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Progress {
    pub sample: bool,
    pub write: bool,
    pub complete: bool,
}

impl SoundBus {
    pub fn software_origin() -> Self {
        Self {
            phase: Phase::Ready,
            plan: None,
            t: 0,
            high: false,
            waiting: false,
            waits: 0,
            sample: None,
            grant_pending: false,
            held_address: 0,
            word_pending: false,
            jr_pending: false,
            djnz_pending: false,
        }
    }

    pub fn pins(&self, reset: bool) -> SoundPins {
        let granted = matches!(self.phase, Phase::Granted | Phase::ReturnHigh);
        let floating = granted || matches!(self.phase, Phase::ReturnLow | Phase::ResetReady);
        let mut pins = SoundPins {
            address: (!reset && !floating).then_some(self.held_address),
            data: None,
            mreq: false,
            read: false,
            write: false,
            m1: false,
            iorq: false,
            refresh: false,
            bus_ack: granted && !reset,
        };
        if reset || self.phase != Phase::Active {
            return pins;
        }
        let p = self.plan.expect("active SOUND plan");
        if p.shape == Shape::Internal {
            return pins;
        }
        let transfer = self.t < 3 || self.t == 3 && self.high;
        if p.shape == Shape::InterruptAck {
            pins.m1 = self.t < 5;
            pins.iorq = self.t == 3 && !self.high || self.t == 4;
            if self.t >= 5 {
                pins.address = Some(p.refresh);
                pins.refresh = self.t == 5 || self.t == 6 && self.high;
                pins.mreq = self.t == 5 && !self.high || self.t == 6 && self.high;
            }
        } else if p.shape == Shape::Fetch {
            pins.m1 = self.t < 3;
            pins.read = self.t == 1 && !self.high || self.t == 2;
            pins.mreq = pins.read;
            if self.t >= 3 {
                pins.address = Some(p.refresh);
                pins.refresh = self.t == 3 || self.t == 4 && self.high;
                pins.mreq = self.t == 3 && !self.high || self.t == 4 && self.high;
            }
        } else {
            pins.mreq = transfer && !(self.t == 1 && self.high);
            pins.read = p.shape == Shape::Read && pins.mreq;
            pins.write = p.shape == Shape::Write
                && (self.t == 2 && (!self.high || self.waiting) || self.t == 3 && self.high);
            if p.shape == Shape::Write && !(self.t == 1 && self.high) {
                pins.data = Some(p.value);
            }
        }
        pins
    }

    pub fn edge(&mut self, edge: Edge, wait: bool, request: bool) -> Result<Progress> {
        let mut out = Progress::default();
        if edge == Edge::None {
            return Ok(out);
        }
        let rising = edge == Edge::Rise;
        self.high = rising;
        match self.phase {
            Phase::Ready | Phase::ReturnLow | Phase::ResetReady if rising => {
                ensure!(
                    self.phase != Phase::ReturnLow || !request,
                    "SOUND request reassertion during accepted bus return"
                );
                if let Some(plan) = self.plan {
                    ensure!(
                        self.phase != Phase::ReturnLow || !request,
                        "SOUND request reassertion during accepted bus return"
                    );
                    self.phase = Phase::Active;
                    self.t = 1;
                    self.waits = 0;
                    self.waiting = false;
                    self.sample = None;
                    self.grant_pending = false;
                    self.held_address = plan.address;
                }
            }
            Phase::GrantPending if rising => {
                self.phase = if request {
                    Phase::Granted
                } else {
                    Phase::ReturnHigh
                };
            }
            Phase::Granted if rising && !request => self.phase = Phase::ReturnHigh,
            Phase::ReturnHigh if !rising => {
                ensure!(
                    !request,
                    "SOUND request reassertion during accepted bus return"
                );
                self.phase = Phase::ReturnLow;
            }
            Phase::Active => {
                let plan = self
                    .plan
                    .ok_or_else(|| anyhow::anyhow!("missing active SOUND plan"))?;
                ensure!(
                    !request || !plan.opaque_tail,
                    "BUSREQ during an unclassified local SOUND aggregate"
                );
                if plan.shape == Shape::Internal {
                    if rising {
                        self.t += 1;
                        if (plan.jr == Some(JrPhase::Internal)
                            || plan.djnz == Some(super::sound_semantics::DjnzPhase::Internal)
                            || plan.ed_word.is_some()
                            || plan.indexed == Some(super::sound_semantics::IndexedPhase::Address)
                            || plan.copy == Some(CopyPhase::Repeat)
                            || plan.word.is_some_and(|p| p.internal()))
                            && self.t == plan.cycles
                        {
                            self.grant_pending = request;
                        }
                    } else if self.t == plan.cycles {
                        out.complete = true;
                        self.jr_pending = false;
                        self.djnz_pending = false;
                        self.word_pending =
                            plan.word == Some(super::sound_semantics::WordPhase::Internal4);
                        self.phase = if self.grant_pending {
                            Phase::GrantPending
                        } else {
                            Phase::Ready
                        };
                    }
                    return Ok(out);
                }
                if rising {
                    if self.waiting {
                        self.waits = self
                            .waits
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("SOUND wait overflow"))?;
                    } else {
                        self.t += 1;
                    }
                    if self.t == 3 && !self.waiting && plan.shape == Shape::Fetch {
                        out.sample = true;
                    }
                    if self.t == plan.cycles && !self.waiting {
                        self.grant_pending = request;
                    }
                } else if self.t
                    == if plan.shape == Shape::InterruptAck {
                        4
                    } else {
                        2
                    }
                {
                    self.waiting = wait;
                } else if self.t >= 3 {
                    if self.t == 3 && plan.shape == Shape::Read {
                        out.sample = true;
                    }
                    if self.t == 3 && plan.shape == Shape::Write {
                        out.write = true;
                    }
                    if self.t == plan.cycles {
                        self.jr_pending = plan.jr == Some(JrPhase::Read) && !plan.finishes;
                        self.djnz_pending = plan.djnz.is_some() && !plan.finishes;
                        self.word_pending =
                            plan.word == Some(super::sound_semantics::WordPhase::M1);
                        out.complete = true;
                        self.phase = if self.grant_pending {
                            Phase::GrantPending
                        } else {
                            Phase::Ready
                        };
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }

    pub fn clear_completed(&mut self) {
        if let Some(plan) = self.plan {
            self.held_address = if matches!(plan.shape, Shape::Fetch | Shape::InterruptAck) {
                plan.refresh
            } else {
                plan.address
            };
        }
        self.plan = None;
        self.t = 0;
        self.waiting = false;
        self.waits = 0;
        self.sample = None;
        self.grant_pending = false;
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !matches!(
                self.phase,
                Phase::GrantPending | Phase::ReturnLow | Phase::ResetReady
            ) || !self.high,
            "invalid low SOUND ownership phase"
        );
        ensure!(
            self.phase != Phase::ReturnHigh || self.high,
            "invalid high SOUND return phase"
        );
        if let Some(plan) = self.plan {
            plan.validate()?;
            ensure!(
                self.djnz_pending
                    == plan
                        .djnz
                        .is_some_and(|p| p != super::sound_semantics::DjnzPhase::M1),
                "DJNZ marker differs from active plan"
            );
            ensure!(
                self.word_pending == plan.word.is_some_and(|p| p.internal()),
                "ADD16 marker differs from active plan"
            );
            ensure!(
                self.jr_pending == (plan.jr == Some(JrPhase::Internal)),
                "JR pending marker differs from active plan"
            );
        } else {
            ensure!(
                !self.djnz_pending
                    || matches!(
                        self.phase,
                        Phase::GrantPending | Phase::Granted | Phase::ReturnHigh
                    ),
                "DJNZ marker lacks plan or grant"
            );
            ensure!(
                !self.word_pending
                    || matches!(
                        self.phase,
                        Phase::GrantPending | Phase::Granted | Phase::ReturnHigh
                    ),
                "ADD16 marker lacks plan or grant"
            );
            ensure!(
                !self.jr_pending
                    || matches!(
                        self.phase,
                        Phase::GrantPending | Phase::Granted | Phase::ReturnHigh
                    ),
                "JR pending marker without an internal plan or grant"
            );
        }
        if self.phase == Phase::Active {
            let p = self
                .plan
                .ok_or_else(|| anyhow::anyhow!("active SOUND cycle has no plan"))?;
            ensure!(
                self.t >= 1 && self.t <= p.cycles,
                "invalid SOUND cycle phase"
            );
            if p.jr == Some(JrPhase::Internal)
                || p.djnz == Some(super::sound_semantics::DjnzPhase::Internal)
                || p.ed_word.is_some()
                || p.shape == Shape::Internal
                    && p.indexed == Some(super::sound_semantics::IndexedPhase::Address)
                || p.copy == Some(CopyPhase::Repeat)
                || p.word.is_some_and(|p| p.internal())
            {
                ensure!(
                    !self.waiting && self.waits == 0 && self.sample.is_none(),
                    "internal cycle has wait/data history"
                );
            }
            let wait_t = if p.shape == Shape::InterruptAck { 4 } else { 2 };
            ensure!(
                !self.waiting || self.t == wait_t,
                "SOUND wait outside its aperture"
            );
            ensure!(
                self.t >= wait_t || self.waits == 0,
                "SOUND waits before sampling aperture"
            );
            ensure!(
                !(self.t == wait_t && self.high && self.waits != 0) || self.waiting,
                "SOUND Tw high lost its sampled WAIT"
            );
            let sampled = p.shape == Shape::Fetch && self.t >= 3
                || p.shape == Shape::Read && (self.t > 3 || self.t == 3 && !self.high);
            ensure!(
                self.sample.is_some() == sampled,
                "SOUND frozen sample differs from phase"
            );
            ensure!(
                !self.grant_pending || self.t == p.cycles && self.high,
                "invalid sampled SOUND grant decision"
            );
        } else {
            ensure!(
                self.t == 0
                    && !self.waiting
                    && self.waits == 0
                    && self.sample.is_none()
                    && !self.grant_pending,
                "stale inactive SOUND cycle"
            );
            ensure!(
                self.plan.is_none()
                    || matches!(
                        self.phase,
                        Phase::Ready | Phase::ReturnLow | Phase::ResetReady
                    ),
                "plan while SOUND bus granted"
            );
        }
        Ok(())
    }

    pub fn validate_request(&self, request_low: bool) -> Result<()> {
        if self.phase == Phase::Active
            && self.high
            && !self.waiting
            && self
                .plan
                .is_some_and(|plan| !plan.opaque_tail && self.t == plan.cycles)
        {
            ensure!(
                self.grant_pending == request_low,
                "typed final-high grant differs from its held request"
            );
        }
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) -> Result<()> {
        self.validate()?;
        w.write_u8(match self.phase {
            Phase::Ready => 0,
            Phase::Active => 1,
            Phase::GrantPending => 2,
            Phase::Granted => 3,
            Phase::ReturnHigh => 4,
            Phase::ReturnLow => 5,
            Phase::ResetReady => 6,
        });
        w.write_bool(self.plan.is_some());
        if let Some(p) = self.plan {
            p.write_state(w);
        }
        w.write_u32(self.t);
        w.write_bool(self.high);
        w.write_bool(self.waiting);
        w.write_u32(self.waits);
        w.write_bool(self.sample.is_some());
        if let Some(v) = self.sample {
            w.write_u8(v);
        }
        w.write_bool(self.grant_pending);
        w.write_u16(self.held_address);
        w.write_bool(self.jr_pending);
        w.write_bool(self.word_pending);
        w.write_bool(self.djnz_pending);
        Ok(())
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let phase = match r.read_u8()? {
            0 => Phase::Ready,
            1 => Phase::Active,
            2 => Phase::GrantPending,
            3 => Phase::Granted,
            4 => Phase::ReturnHigh,
            5 => Phase::ReturnLow,
            6 => Phase::ResetReady,
            _ => anyhow::bail!("invalid SOUND ownership phase"),
        };
        let plan = if r.read_bool()? {
            Some(Plan::read_state(r)?)
        } else {
            None
        };
        let s = Self {
            phase,
            plan,
            t: r.read_u32()?,
            high: r.read_bool()?,
            waiting: r.read_bool()?,
            waits: r.read_u32()?,
            sample: if r.read_bool()? {
                Some(r.read_u8()?)
            } else {
                None
            },
            grant_pending: r.read_bool()?,
            held_address: r.read_u16()?,
            jr_pending: r.read_bool()?,
            word_pending: r.read_bool()?,
            djnz_pending: r.read_bool()?,
        };
        s.validate()?;
        Ok(s)
    }
}
