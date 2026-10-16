use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Requester(pub u8);

impl Requester {
    pub const MAIN: Self = Self(0);
    pub const SOUND: Self = Self(1);
    pub const VDP: Self = Self(2);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reservation {
    pub requester: Requester,
    pub ticks: Range<u64>,
}

impl Reservation {
    pub fn wait_after(&self, occupied: &[Self], quantum: u64) -> Option<u64> {
        if quantum == 0
            || self.ticks.start > self.ticks.end
            || occupied
                .iter()
                .any(|slot| slot.ticks.start > slot.ticks.end)
        {
            return None;
        }
        if self.ticks.is_empty() {
            return Some(0);
        }
        let mut waits: u64 = 0;
        loop {
            let elapsed = waits.checked_mul(quantum)?;
            let start = self.ticks.start.checked_add(elapsed)?;
            let end = self.ticks.end.checked_add(elapsed)?;
            let release = occupied
                .iter()
                .filter(|slot| {
                    slot.requester != self.requester
                        && !slot.ticks.is_empty()
                        && slot.ticks.start < end
                        && start < slot.ticks.end
                })
                .map(|slot| slot.ticks.end)
                .max();
            let Some(release) = release else {
                return Some(waits);
            };
            // Each shift clears at least one finite reservation permanently.
            waits = (release - self.ticks.start).div_ceil(quantum);
        }
    }
}
