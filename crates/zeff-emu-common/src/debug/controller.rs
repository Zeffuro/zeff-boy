use super::{WatchHit, WatchType, Watchpoint};

pub struct DebugController {
    breakpoints: Box<[bool; 65536]>,
    one_shot_breakpoints: Box<[bool; 65536]>,
    breakpoint_count: usize,
    pub watchpoints: Vec<Watchpoint>,
    pub break_on_next: bool,
    pub hit_breakpoint: Option<u16>,
    pub hit_watchpoint: Option<WatchHit>,
    breakpoints_active: bool,
    watchpoints_active: bool,
}

impl std::fmt::Debug for DebugController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DebugController")
            .field("breakpoint_count", &self.breakpoint_count)
            .field("watchpoints", &self.watchpoints)
            .field("break_on_next", &self.break_on_next)
            .field("hit_breakpoint", &self.hit_breakpoint)
            .field("hit_watchpoint", &self.hit_watchpoint)
            .finish_non_exhaustive()
    }
}

impl Default for DebugController {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugController {
    pub fn new() -> Self {
        Self {
            breakpoints: Box::new([false; 65536]),
            one_shot_breakpoints: Box::new([false; 65536]),
            breakpoint_count: 0,
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
            breakpoints_active: false,
            watchpoints_active: false,
        }
    }

    pub fn has_breakpoint(&self, addr: u16) -> bool {
        self.breakpoints[addr as usize]
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.breakpoints
            .iter()
            .enumerate()
            .filter(|entry| *entry.1)
            .map(|entry| entry.0 as u16)
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.one_shot_breakpoints
            .iter()
            .enumerate()
            .filter(|entry| *entry.1)
            .map(|entry| entry.0 as u16)
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.one_shot_breakpoints[addr as usize] = false;
        if !self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = true;
            self.breakpoint_count += 1;
        }
        self.breakpoints_active = true;
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: u16) {
        self.add_breakpoint(addr);
        self.one_shot_breakpoints[addr as usize] = true;
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.one_shot_breakpoints[addr as usize] = false;
        if self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = false;
            self.breakpoint_count -= 1;
        }
        self.breakpoints_active = self.breakpoint_count > 0;
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        if self.breakpoints[addr as usize] {
            self.remove_breakpoint(addr);
        } else {
            self.add_breakpoint(addr);
        }
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
        self.add_watchpoint_range(addr, addr, watch_type);
    }

    pub fn add_watchpoint_range(&mut self, start: u16, end: u16, watch_type: WatchType) {
        let (start, end) = ordered_range(start, end);
        if self
            .watchpoints
            .iter()
            .any(|w| w.address == start && w.end_address == end && w.watch_type == watch_type)
        {
            return;
        }
        self.watchpoints.push(Watchpoint {
            address: start,
            end_address: end,
            watch_type,
            last_value: None,
        });
        self.watchpoints_active = true;
    }

    pub fn remove_watchpoint(&mut self, start: u16, end: u16, watch_type: WatchType) {
        let (start, end) = ordered_range(start, end);
        self.watchpoints.retain(|watch| {
            watch.address != start || watch.end_address != end || watch.watch_type != watch_type
        });
        self.watchpoints_active = !self.watchpoints.is_empty();
    }

    #[inline]
    pub fn should_break(&mut self, pc: u16) -> bool {
        if !self.breakpoints_active && !self.break_on_next {
            return false;
        }

        if self.breakpoints[pc as usize] {
            if self.one_shot_breakpoints[pc as usize] {
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

    #[inline]
    pub fn has_watchpoints(&self) -> bool {
        self.watchpoints_active
    }

    #[inline]
    pub fn any_active(&self) -> bool {
        self.breakpoints_active || self.break_on_next || self.watchpoints_active
    }

    pub fn check_watch_read(&mut self, addr: u16, value: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if !(watch.address..=watch.end_address).contains(&addr) {
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

    pub fn check_watch_write(&mut self, addr: u16, old_val: u8, new_val: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if !(watch.address..=watch.end_address).contains(&addr) {
                continue;
            }
            if matches!(watch.watch_type, WatchType::Write | WatchType::ReadWrite)
                && old_val != new_val
            {
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

    pub fn clear_hits(&mut self) {
        self.hit_breakpoint = None;
        self.hit_watchpoint = None;
    }
}

fn ordered_range(start: u16, end: u16) -> (u16, u16) {
    (start.min(end), start.max(end))
}
