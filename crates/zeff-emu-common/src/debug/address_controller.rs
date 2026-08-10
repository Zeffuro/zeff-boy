use crate::address::Address;

use super::{AddressWatchHit, AddressWatchpoint, WatchType};

pub struct AddressDebugController {
    breakpoints: Vec<Address>,
    pub watchpoints: Vec<AddressWatchpoint>,
    pub break_on_next: bool,
    pub hit_breakpoint: Option<Address>,
    pub hit_watchpoint: Option<AddressWatchHit>,
}

impl std::fmt::Debug for AddressDebugController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AddressDebugController")
            .field("breakpoints", &self.breakpoints)
            .field("watchpoints", &self.watchpoints)
            .field("break_on_next", &self.break_on_next)
            .field("hit_breakpoint", &self.hit_breakpoint)
            .field("hit_watchpoint", &self.hit_watchpoint)
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
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
        }
    }

    pub fn add_breakpoint(&mut self, addr: Address) {
        if !self.breakpoints.contains(&addr) {
            self.breakpoints.push(addr);
            self.breakpoints.sort_unstable();
        }
    }

    pub fn remove_breakpoint(&mut self, addr: Address) {
        self.breakpoints.retain(|&bp| bp != addr);
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

    pub fn add_watchpoint(&mut self, addr: Address, watch_type: WatchType) {
        if self
            .watchpoints
            .iter()
            .any(|w| w.address == addr && w.watch_type == watch_type)
        {
            return;
        }
        self.watchpoints.push(AddressWatchpoint {
            address: addr,
            watch_type,
            last_value: None,
        });
    }

    pub fn should_break(&mut self, pc: Address) -> bool {
        if self.breakpoints.binary_search(&pc).is_ok() {
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
            if watch.address == addr
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
            if watch.address == addr
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
    }
}
