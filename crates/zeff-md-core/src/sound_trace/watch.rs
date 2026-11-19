use super::attempt::Attempt;
use zeff_emu_common::debug::WatchType;

pub const MAX_SOUND_WATCHPOINTS: usize = 64;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundWatchSpace {
    Memory,
    Io,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundWatchRule {
    pub space: SoundWatchSpace,
    pub start: u16,
    pub end: u16,
    pub kind: WatchType,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundAccessKind {
    Instruction,
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundAccess {
    pub space: SoundWatchSpace,
    pub kind: SoundAccessKind,
    pub address: u16,
    pub value: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundWatchpoint {
    pub rule: SoundWatchRule,
    pub hits: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundWatchHit {
    pub pc: u16,
    pub cycle: u64,
    pub ordinal: u8,
    pub access: SoundAccess,
}
#[derive(Clone, Default)]
pub(super) struct WatchState {
    pub rules: Vec<SoundWatchpoint>,
    pub first: Option<SoundWatchHit>,
    pub gaps: u64,
    pub blocked: bool,
}
impl WatchState {
    pub fn armed(&self) -> bool {
        !self.rules.is_empty()
    }
    pub fn lost(&mut self) {
        if self.armed() {
            self.gaps = self.gaps.saturating_add(1);
        }
    }
    pub fn gap(&mut self) {
        self.lost();
        self.first = None;
    }
    pub fn edit(&mut self, rule: SoundWatchRule, add: bool) -> Result<bool, &'static str> {
        if rule.start > rule.end || (rule.space == SoundWatchSpace::Io && rule.end > 255) {
            return Err("invalid SOUND watch range");
        }
        let old = self.rules.iter().position(|watch| watch.rule == rule);
        match (old, add) {
            (Some(_), true) | (None, false) => return Ok(false),
            (Some(index), false) => {
                self.rules.remove(index);
            }
            (None, true) => {
                if self.rules.len() == MAX_SOUND_WATCHPOINTS {
                    return Err("SOUND watchpoint capacity reached");
                }
                self.rules.push(SoundWatchpoint { rule, hits: 0 });
            }
        }
        self.first = None;
        self.blocked = false;
        Ok(true)
    }
    pub fn accept(&mut self, attempt: &Attempt, cycle: u64, interrupt: bool) {
        if !self.armed() || self.blocked {
            return;
        }
        let accesses = attempt.accesses.borrow();
        if accesses.overflow {
            self.lost();
            self.blocked = true;
            return;
        }
        if attempt.halted && !interrupt {
            return;
        }
        let pc = attempt.record.borrow().pc as u16;
        for (ordinal, access) in accesses
            .items
            .iter()
            .enumerate()
            .filter_map(|(n, a)| a.map(|a| (n, a)))
        {
            if access.kind == SoundAccessKind::Instruction {
                continue;
            }
            for watch in &mut self.rules {
                let rule = watch.rule;
                if access.space == rule.space
                    && (rule.start..=rule.end).contains(&access.address)
                    && matches!(
                        (rule.kind, access.kind),
                        (WatchType::ReadWrite, _)
                            | (WatchType::Read, SoundAccessKind::Read)
                            | (WatchType::Write, SoundAccessKind::Write)
                    )
                {
                    let Some(count) = watch.hits.checked_add(1) else {
                        self.gaps = self.gaps.saturating_add(1);
                        self.blocked = true;
                        return;
                    };
                    watch.hits = count;
                    self.first.get_or_insert(SoundWatchHit {
                        pc,
                        cycle,
                        ordinal: ordinal as u8,
                        access,
                    });
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct AccessLog {
    pub items: [Option<SoundAccess>; 8],
    pub overflow: bool,
    reads: u8,
    writes: u8,
    next: usize,
}
impl AccessLog {
    pub fn push(&mut self, access: SoundAccess) {
        if access.kind == SoundAccessKind::Write {
            self.writes = self.writes.saturating_add(1);
        } else {
            self.reads = self.reads.saturating_add(1);
        }
        if self.reads > 6 || self.writes > 2 {
            self.overflow = true;
        }
        if let Some(slot) = self.items.get_mut(self.next) {
            *slot = Some(access);
            self.next += 1;
        } else {
            self.overflow = true;
        }
    }
}

#[cfg(test)]
mod tests;
