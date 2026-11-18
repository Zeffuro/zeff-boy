use super::model::{Error, InstructionModel, Phase};
use super::{ServiceResource, ServiceRole};

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionalProfile {
    identity: u64,
    numerator: u32,
    denominator: u32,
    costs: [u16; 6],
}

impl FunctionalProfile {
    // Costs follow ServiceResource order; these are authored software timings.
    pub fn new(
        identity: u64,
        numerator: u32,
        denominator: u32,
        costs: [u16; 6],
    ) -> Result<Self, Error> {
        if identity == 0
            || numerator == 0
            || numerator > denominator
            || denominator > 1_000_000
            || costs.iter().any(|&cost| cost == 0 || cost > 4096)
        {
            return Err(Error::Image("functional clock profile bounds"));
        }
        Ok(Self {
            identity,
            numerator,
            denominator,
            costs,
        })
    }

    fn cost(self, resource: ServiceResource) -> u16 {
        self.costs[match resource {
            ServiceResource::ProgramIram => 0,
            ServiceResource::ProgramIrom => 1,
            ServiceResource::SharedRom => 2,
            ServiceResource::SharedDram => 3,
            ServiceResource::ExternalIram => 4,
            ServiceResource::Controller => 5,
        }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionalTiming {
    pub profile: FunctionalProfile,
    pub master_tick: u64,
    pub completed_grants: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FunctionalClock {
    profile: FunctionalProfile,
    origin: u64,
    now: u64,
    fraction: u32,
    remaining: u16,
    grants: u64,
    hold: bool,
}

impl FunctionalClock {
    fn new(profile: FunctionalProfile, origin: u64) -> Self {
        Self {
            profile,
            origin,
            now: origin,
            fraction: 0,
            remaining: profile.cost(ServiceResource::ProgramIrom),
            grants: 0,
            hold: false,
        }
    }
}

impl InstructionModel {
    pub(crate) fn enable_functional_clock(
        &mut self,
        profile: FunctionalProfile,
        origin: u64,
    ) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Reset) || self.services.is_none() || self.clock.is_some() {
            return Err(Error::Busy);
        }
        self.clock = Some(FunctionalClock::new(profile, origin));
        Ok(())
    }

    pub(super) fn reset_functional_clock(&mut self) {
        if let Some(clock) = &mut self.clock {
            *clock = FunctionalClock::new(clock.profile, 0);
        }
    }

    pub fn functional_timing(&self) -> Option<FunctionalTiming> {
        self.clock.as_ref().map(|clock| FunctionalTiming {
            profile: clock.profile,
            master_tick: clock.now,
            completed_grants: clock.grants,
        })
    }

    fn clock_paused(&self) -> bool {
        self.boundary_control == Some(true)
            || self
                .clock
                .as_ref()
                .is_some_and(|clock| clock.hold && matches!(self.phase, Phase::Fetch))
    }

    pub(crate) fn functional_event_tick(&self) -> Result<Option<u64>, Error> {
        let Some(clock) = &self.clock else {
            return Ok(None);
        };
        if self.clock_paused() {
            return Ok(None);
        }
        let units = u64::from(clock.remaining) * u64::from(clock.profile.denominator)
            - u64::from(clock.fraction);
        let ticks = units.div_ceil(u64::from(clock.profile.numerator));
        Ok(Some(
            clock
                .now
                .checked_add(ticks)
                .ok_or(Error::State("functional clock overflow".into()))?,
        ))
    }

    pub(crate) fn hold_functional_main(&mut self, hold: bool) -> bool {
        if let Some(clock) = &mut self.clock {
            clock.hold = hold;
            true
        } else {
            false
        }
    }

    pub(crate) fn functional_main_wait(&self) -> Result<Option<u64>, Error> {
        if self.clock.as_ref().is_some_and(|clock| clock.hold)
            && !matches!(self.phase, Phase::Fetch)
        {
            return self
                .functional_event_tick()
                .map(|tick| tick.map(|tick| tick - self.clock.as_ref().unwrap().now));
        }
        Ok(None)
    }

    fn functional_cost(&self) -> Result<u16, Error> {
        let request = match self.phase {
            Phase::Reset => self.program_request(0xfffc, ServiceRole::ResetVector),
            Phase::Fetch => self.program_request(self.core.pc, ServiceRole::Opcode),
            _ => self.pending_service()?.ok_or(Error::State(
                "functional clock has unsettled instruction".into(),
            ))?,
        };
        Ok(self.clock.as_ref().unwrap().profile.cost(request.resource))
    }

    pub fn advance_functional_to(&mut self, target: u64) -> Result<(), Error> {
        let mut candidate = self.clone();
        candidate.advance_functional_candidate(target)?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn advance_functional_candidate(&mut self, target: u64) -> Result<(), Error> {
        self.advance_functional_current(target, None)
    }

    pub(crate) fn advance_functional_current(
        &mut self,
        target: u64,
        rom: Option<super::history::CurrentRom<'_>>,
    ) -> Result<(), Error> {
        let Some(clock) = &self.clock else {
            return Err(Error::Image("missing functional clock"));
        };
        if target < clock.now {
            return Err(Error::State("functional clock moved backwards".into()));
        }
        while self.clock.as_ref().unwrap().now < target {
            let event = self.functional_event_tick()?;
            let end = event.map_or(target, |event| event.min(target));
            let paused = self.clock_paused();
            let clock = self.clock.as_mut().unwrap();
            let units = u128::from(end - clock.now) * u128::from(clock.profile.numerator)
                + u128::from(clock.fraction);
            clock.fraction = (units % u128::from(clock.profile.denominator)) as u32;
            clock.now = end;
            if paused {
                continue;
            }
            clock.remaining -= (units / u128::from(clock.profile.denominator)) as u16;
            if clock.remaining != 0 {
                continue;
            }
            let grants = clock
                .grants
                .checked_add(1)
                .ok_or(Error::State("functional grant count overflow".into()))?;
            self.advance_service_current(true, rom)?;
            for _ in 0..2 {
                if matches!(self.phase, Phase::Fetch) || self.pending_service()?.is_some() {
                    break;
                }
                self.advance_service_current(true, rom)?;
            }
            let remaining = self.functional_cost()?;
            let clock = self.clock.as_mut().unwrap();
            clock.grants = grants;
            clock.remaining = remaining;
        }
        Ok(())
    }
}
