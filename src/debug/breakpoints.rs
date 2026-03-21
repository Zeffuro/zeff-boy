use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub(crate) struct Watchpoint {
    pub(crate) address: u16,
    pub(crate) watch_type: WatchType,
    pub(crate) last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WatchHit {
    pub(crate) address: u16,
    pub(crate) old_value: u8,
    pub(crate) new_value: u8,
    pub(crate) watch_type: WatchType,
}

pub(crate) struct DebugController {
    pub(crate) breakpoints: HashSet<u16>,
    pub(crate) watchpoints: Vec<Watchpoint>,
    pub(crate) break_on_next: bool,
    pub(crate) hit_breakpoint: Option<u16>,
    pub(crate) hit_watchpoint: Option<WatchHit>,
}

impl DebugController {
    pub(crate) fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
        }
    }

    pub(crate) fn add_breakpoint(&mut self, addr: u16) {
        self.breakpoints.insert(addr);
    }

    pub(crate) fn remove_breakpoint(&mut self, addr: u16) {
        self.breakpoints.remove(&addr);
    }

    pub(crate) fn toggle_breakpoint(&mut self, addr: u16) {
        if !self.breakpoints.remove(&addr) {
            self.breakpoints.insert(addr);
        }
    }

    pub(crate) fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
        if self
            .watchpoints
            .iter()
            .any(|w| w.address == addr && w.watch_type == watch_type)
        {
            return;
        }
        self.watchpoints.push(Watchpoint {
            address: addr,
            watch_type,
            last_value: None,
        });
    }

    pub(crate) fn should_break(&mut self, pc: u16) -> bool {
        if self.breakpoints.is_empty() && !self.break_on_next {
            return false;
        }

        if self.breakpoints.contains(&pc) {
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

    pub(crate) fn has_watchpoints(&self) -> bool {
        !self.watchpoints.is_empty()
    }

    pub(crate) fn check_watch_read(&mut self, addr: u16, value: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if watch.address != addr {
                continue;
            }
            if matches!(watch.watch_type, WatchType::Read | WatchType::ReadWrite) {
                let old_value = watch.last_value.unwrap_or(value);
                watch.last_value = Some(value);
                self.hit_watchpoint = Some(WatchHit {
                    address: addr,
                    old_value,
                    new_value: value,
                    watch_type: WatchType::Read,
                });
                return;
            }
        }
    }

    pub(crate) fn check_watch_write(&mut self, addr: u16, old_val: u8, new_val: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if watch.address != addr {
                continue;
            }
            if matches!(watch.watch_type, WatchType::Write | WatchType::ReadWrite) && old_val != new_val {
                watch.last_value = Some(new_val);
                self.hit_watchpoint = Some(WatchHit {
                    address: addr,
                    old_value: old_val,
                    new_value: new_val,
                    watch_type: WatchType::Write,
                });
                return;
            }
        }
    }

    pub(crate) fn clear_hits(&mut self) {
        self.hit_breakpoint = None;
        self.hit_watchpoint = None;
    }
}

