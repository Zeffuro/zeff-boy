use crate::address::Address;

use super::{AddressWatchHit, AddressWatchpoint, BreakpointHitCondition, DebugEvent, WatchType};

pub struct AddressDebugController {
    breakpoints: Vec<Address>,
    one_shot_breakpoints: Vec<Address>,
    hit_conditions: Vec<BreakpointHitCondition>,
    event_breakpoints: [bool; DebugEvent::ALL.len()],
    pub watchpoints: Vec<AddressWatchpoint>,
    pub break_on_next: bool,
    pub hit_breakpoint: Option<Address>,
    pub hit_watchpoint: Option<AddressWatchHit>,
    pub hit_event: Option<DebugEvent>,
}

impl std::fmt::Debug for AddressDebugController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressDebugController")
            .field("breakpoints", &self.breakpoints)
            .field("watchpoints", &self.watchpoints)
            .field("break_on_next", &self.break_on_next)
            .field("hit_breakpoint", &self.hit_breakpoint)
            .field("hit_watchpoint", &self.hit_watchpoint)
            .field("hit_event", &self.hit_event)
            .finish()
    }
}

impl Default for AddressDebugController {
    fn default() -> Self {
        Self::new()
    }
}

impl AddressDebugController {
    pub fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
            one_shot_breakpoints: Vec::new(),
            hit_conditions: Vec::new(),
            event_breakpoints: [false; DebugEvent::ALL.len()],
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
            hit_event: None,
        }
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        self.one_shot_breakpoints.retain(|&bp| bp != addr);
        self.hit_conditions
            .retain(|condition| condition.address != addr);
        if !self.breakpoints.contains(&addr) {
            self.breakpoints.push(addr);
            self.breakpoints.sort_unstable();
        }
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: Address) {
        self.add_breakpoint(addr);
        if let Err(index) = self.one_shot_breakpoints.binary_search(&addr) {
            self.one_shot_breakpoints.insert(index, addr);
        }
    }

    pub fn add_breakpoint_after(&mut self, addr: Address, target_hits: u64) {
        self.add_breakpoint(addr);
        self.hit_conditions.push(BreakpointHitCondition {
            address: addr,
            target_hits: target_hits.max(1),
            hits: 0,
        });
    }

    pub fn set_event_breakpoint(&mut self, event: DebugEvent, enabled: bool) {
        self.event_breakpoints[event.index()] = enabled;
        if !enabled && self.hit_event == Some(event) {
            self.hit_event = None;
        }
    }

    pub fn has_event_breakpoint(&self, event: DebugEvent) -> bool {
        self.event_breakpoints[event.index()]
    }

    pub fn iter_event_breakpoints(&self) -> impl Iterator<Item = DebugEvent> + '_ {
        DebugEvent::ALL
            .into_iter()
            .filter(|event| self.has_event_breakpoint(*event))
    }

    pub fn check_event(&mut self, event: DebugEvent) -> bool {
        if !self.has_event_breakpoint(event) {
            return false;
        }
        self.hit_event = Some(event);
        self.break_on_next = false;
        true
    }

    pub fn remove_breakpoint(&mut self, addr: Address) {
        self.breakpoints.retain(|&bp| bp != addr);
        self.one_shot_breakpoints.retain(|&bp| bp != addr);
        self.hit_conditions
            .retain(|condition| condition.address != addr);
    }

    pub fn toggle_breakpoint(&mut self, addr: Address) {
        if self.breakpoints.contains(&addr) {
            self.remove_breakpoint(addr);
        } else {
            self.add_breakpoint(addr);
        }
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.breakpoints.iter().copied()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.one_shot_breakpoints.iter().copied()
    }

    pub fn iter_breakpoint_hit_conditions(
        &self,
    ) -> impl Iterator<Item = BreakpointHitCondition> + '_ {
        self.hit_conditions.iter().copied()
    }

    pub fn add_watchpoint(&mut self, addr: Address, watch_type: WatchType) {
        self.add_watchpoint_range(addr, addr, watch_type);
    }

    pub fn add_watchpoint_range(&mut self, start: Address, end: Address, watch_type: WatchType) {
        let (start, end) = ordered_range(start, end);
        if self
            .watchpoints
            .iter()
            .any(|w| w.address == start && w.end_address == end && w.watch_type == watch_type)
        {
            return;
        }
        self.watchpoints.push(AddressWatchpoint {
            address: start,
            end_address: end,
            watch_type,
            last_value: None,
        });
    }

    pub fn remove_watchpoint(&mut self, start: Address, end: Address, watch_type: WatchType) {
        let (start, end) = ordered_range(start, end);
        self.watchpoints.retain(|watch| {
            watch.address != start || watch.end_address != end || watch.watch_type != watch_type
        });
    }

    #[inline]
    pub fn should_break(&mut self, pc: Address) -> bool {
        if self.breakpoints.is_empty() && !self.break_on_next {
            return false;
        }
        if self.breakpoints.binary_search(&pc).is_ok() {
            if let Some(condition) = self
                .hit_conditions
                .iter_mut()
                .find(|condition| condition.address == pc)
            {
                condition.hits = condition.hits.saturating_add(1);
                if condition.hits < condition.target_hits {
                    self.hit_breakpoint = None;
                    return false;
                }
            }
            if self.one_shot_breakpoints.binary_search(&pc).is_ok() {
                self.remove_breakpoint(pc);
            }
            self.hit_breakpoint = Some(pc);
            self.break_on_next = false;
            return true;
        }
        if self.break_on_next {
            self.break_on_next = false;
            return true;
        }
        false
    }

    pub fn check_watch_read(&mut self, addr: Address, value: u8) {
        for watch in &mut self.watchpoints {
            if (watch.address..=watch.end_address).contains(&addr)
                && matches!(watch.watch_type, WatchType::Read | WatchType::ReadWrite)
            {
                let old_value = watch.last_value.unwrap_or(value);
                watch.last_value = Some(value);
                self.hit_watchpoint = Some(AddressWatchHit {
                    address: addr,
                    old_value,
                    new_value: value,
                    watch_type: WatchType::Read,
                });
                return;
            }
        }
    }

    pub fn check_watch_write(&mut self, addr: Address, old_val: u8, new_val: u8) {
        for watch in &mut self.watchpoints {
            if (watch.address..=watch.end_address).contains(&addr)
                && matches!(watch.watch_type, WatchType::Write | WatchType::ReadWrite)
                && old_val != new_val
            {
                watch.last_value = Some(new_val);
                self.hit_watchpoint = Some(AddressWatchHit {
                    address: addr,
                    old_value: old_val,
                    new_value: new_val,
                    watch_type: WatchType::Write,
                });
                return;
            }
        }
    }

    pub fn clear_hits(&mut self) {
        self.hit_breakpoint = None;
        self.hit_watchpoint = None;
        self.hit_event = None;
    }
}

fn ordered_range(start: Address, end: Address) -> (Address, Address) {
    (start.min(end), start.max(end))
}
