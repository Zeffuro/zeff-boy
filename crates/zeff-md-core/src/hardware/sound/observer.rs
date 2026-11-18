use std::cell::{Cell, RefCell};

mod arena;
mod ledger;
mod registry;
mod token;
use ledger::*;
#[cfg(any(test, feature = "ram-copy-census"))]
pub(crate) use ledger::{Metric, Report};
use registry::Registry;
pub(super) use token::deep_clone;
pub(crate) use token::{Attempt, Token};

struct Slot {
    registry: RefCell<Registry>,
    missed: Cell<bool>,
}

thread_local! {
    static SLOT: Slot = const { Slot {
        registry: RefCell::new(Registry::new()), missed: Cell::new(false),
    } };
}

#[cfg(feature = "ram-copy-census")]
pub(crate) fn idle() -> bool {
    SLOT.try_with(|slot| {
        slot.registry
            .try_borrow()
            .is_ok_and(|registry| registry.session == 0)
    })
    .unwrap_or(false)
}

fn tracked<R>(operation: impl FnOnce(&mut Registry) -> R) -> Option<R> {
    SLOT.try_with(|slot| {
        let Ok(mut registry) = slot.registry.try_borrow_mut() else {
            slot.missed.set(true);
            return None;
        };
        if registry.session == 0 {
            return None;
        }
        if slot.missed.replace(false) {
            registry.flags |= REENTRANT;
        }
        Some(operation(&mut registry))
    })
    .ok()
    .flatten()
}

fn observed<R>(operation: impl FnOnce(&mut Registry) -> R) -> Option<R> {
    tracked(|registry| (registry.suppressed == 0).then(|| operation(registry))).flatten()
}

pub(super) fn allocation(address: usize) -> Token {
    observed(|registry| registry.allocation(address)).unwrap_or_else(Token::empty)
}

#[cfg(test)]
pub(super) fn enroll(token: &mut Token, address: usize) {
    if token.session == 0 {
        *token = allocation(address);
    } else {
        observed(|registry| registry.flags |= INVALID_SOURCE);
    }
}

pub(super) fn snapshot(source: &Token) -> Token {
    observed(|registry| registry.snapshot(source)).unwrap_or_else(Token::empty)
}

pub(super) fn image(owner: &Token, parent: Option<&Token>) -> Token {
    observed(|registry| registry.image(owner, parent)).unwrap_or_else(Token::empty)
}

pub(super) fn before_mut(source: &Token, address: usize, strong: usize, weak: usize) -> u32 {
    observed(|registry| {
        registry.before_mut(source, address, strong, weak);
        registry.session
    })
    .unwrap_or(0)
}

pub(super) fn after_mut(source: &Token, session: u32, address: usize) {
    if session == 0 {
        return;
    }
    observed(|registry| {
        if session == registry.session {
            registry.after_mut(source, address);
        } else {
            registry.flags |= INVALID_SOURCE;
        }
    });
}

pub(super) fn access(address: u16, direction: zeff_m68k::bus::Direction) {
    observed(|registry| {
        registry.access(
            address <= crate::constants::SOUND_RAM_END,
            direction == zeff_m68k::bus::Direction::Write,
        )
    });
}

pub(super) fn publication(image: &Token) {
    observed(|registry| registry.publication(image));
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) struct Verification;

#[cfg(test)]
impl Verification {
    pub fn begin() -> Self {
        tracked(|registry| match registry.suppressed.checked_add(1) {
            Some(depth) if depth <= DEPTH => registry.suppressed = depth,
            _ => registry.flags |= EXHAUSTED,
        });
        Self
    }
}

#[cfg(test)]
impl Drop for Verification {
    fn drop(&mut self) {
        tracked(|registry| {
            if registry.suppressed == 0 {
                registry.flags |= BAD_ORDER;
            } else {
                registry.suppressed -= 1;
            }
        });
    }
}

#[cfg(any(test, feature = "ram-copy-census"))]
pub(crate) struct Session {
    id: u32,
    finished: bool,
    owner_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(any(test, feature = "ram-copy-census"))]
impl Session {
    pub(crate) fn begin() -> Self {
        use std::sync::atomic::AtomicU32;
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let id = next_session(&NEXT).unwrap_or(0);
        SLOT.with(|slot| {
            let Ok(mut registry) = slot.registry.try_borrow_mut() else {
                slot.missed.set(true);
                return Self {
                    id: 0,
                    finished: false,
                    owner_thread: std::marker::PhantomData,
                };
            };
            if registry.session != 0 {
                registry.flags |= BAD_ORDER;
                return Self {
                    id: 0,
                    finished: false,
                    owner_thread: std::marker::PhantomData,
                };
            }
            *registry = Registry::new();
            registry.session = id;
            if id == 0 {
                registry.flags |= EXHAUSTED;
            }
            slot.missed.set(false);
            Self {
                id,
                finished: false,
                owner_thread: std::marker::PhantomData,
            }
        })
    }

    pub(crate) fn report(&self) -> ledger::Report {
        SLOT.with(|slot| {
            let Ok(mut registry) = slot.registry.try_borrow_mut() else {
                slot.missed.set(true);
                return ledger::Report {
                    scope: "DirectedOwnerThreadOnly",
                    full_coverage: false,
                    flags: REENTRANT,
                    live_owners: 0,
                    live_images: 0,
                    counters: [0; METRICS],
                };
            };
            if slot.missed.replace(false) {
                registry.flags |= REENTRANT;
            }
            if registry.session != self.id || self.id == 0 {
                registry.flags |= INVALID_SOURCE;
            }
            registry.report()
        })
    }

    pub(crate) fn finish(mut self) -> ledger::Report {
        let mut report = self.report();
        if self.id == 0 {
            self.finished = true;
            report.flags |= INVALID_SOURCE;
            return report;
        }
        tracked(|registry| {
            if registry.session != self.id {
                report.flags |= INVALID_SOURCE;
                return;
            }
            if report.live_owners != 0
                || report.live_images != 0
                || registry.attempt_depth != 0
                || registry.parent_depth != 0
                || registry.suppressed != 0
            {
                report.flags |= INCOMPLETE;
            }
            registry.session = 0;
        });
        self.finished = true;
        report
    }
}

#[cfg(any(test, feature = "ram-copy-census"))]
impl Drop for Session {
    fn drop(&mut self) {
        if !self.finished && self.id != 0 {
            tracked(|registry| {
                if registry.session == self.id {
                    registry.session = 0;
                }
            });
        }
    }
}

#[cfg(any(test, feature = "ram-copy-census"))]
fn next_session(next: &std::sync::atomic::AtomicU32) -> Option<u32> {
    use std::sync::atomic::Ordering;
    next.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
}
