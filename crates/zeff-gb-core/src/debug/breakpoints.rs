#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchType {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug)]
pub struct Watchpoint {
    pub address: u16,
    pub watch_type: WatchType,
    pub last_value: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct WatchHit {
    pub address: u16,
    pub old_value: u8,
    pub new_value: u8,
    pub watch_type: WatchType,
}

pub struct DebugController {
    breakpoints: Box<[bool; 65536]>,
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
            breakpoint_count: 0,
            watchpoints: Vec::new(),
            break_on_next: false,
            hit_breakpoint: None,
            hit_watchpoint: None,
            breakpoints_active: false,
            watchpoints_active: false,
        }
    }

    #[allow(dead_code)]
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

    pub fn add_breakpoint(&mut self, addr: u16) {
        if !self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = true;
            self.breakpoint_count += 1;
        }
        self.breakpoints_active = true;
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        if self.breakpoints[addr as usize] {
            self.breakpoints[addr as usize] = false;
            self.breakpoint_count -= 1;
        }
        self.breakpoints_active = self.breakpoint_count > 0;
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        let slot = &mut self.breakpoints[addr as usize];
        if *slot {
            *slot = false;
            self.breakpoint_count -= 1;
        } else {
            *slot = true;
            self.breakpoint_count += 1;
        }
        self.breakpoints_active = self.breakpoint_count > 0;
    }

    pub fn add_watchpoint(&mut self, addr: u16, watch_type: WatchType) {
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
        self.watchpoints_active = true;
    }

    #[inline]
    pub fn should_break(&mut self, pc: u16) -> bool {
        if !self.breakpoints_active && !self.break_on_next {
            return false;
        }

        if self.breakpoints[pc as usize] {
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

    pub fn check_watch_read(&mut self, addr: u16, value: u8) {
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

    pub fn check_watch_write(&mut self, addr: u16, old_val: u8, new_val: u8) {
        if self.watchpoints.is_empty() {
            return;
        }

        for watch in &mut self.watchpoints {
            if watch.address != addr {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x0100);
        assert!(dc.has_breakpoint(0x0100));
        dc.remove_breakpoint(0x0100);
        assert!(!dc.has_breakpoint(0x0100));
    }

    #[test]
    fn toggle_breakpoint_adds_and_removes() {
        let mut dc = DebugController::new();
        dc.toggle_breakpoint(0x0150);
        assert!(dc.has_breakpoint(0x0150));
        dc.toggle_breakpoint(0x0150);
        assert!(!dc.has_breakpoint(0x0150));
    }

    #[test]
    fn should_break_at_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x0200);
        assert!(dc.should_break(0x0200));
        assert_eq!(dc.hit_breakpoint, Some(0x0200));
    }

    #[test]
    fn should_not_break_without_breakpoints() {
        let mut dc = DebugController::new();
        assert!(!dc.should_break(0x0200));
    }

    #[test]
    fn break_on_next_fires_once() {
        let mut dc = DebugController::new();
        dc.break_on_next = true;
        assert!(dc.should_break(0x0300));
        assert!(!dc.should_break(0x0301));
    }

    #[test]
    fn watchpoint_write_detects_change() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0xC000, WatchType::Write);
        dc.check_watch_write(0xC000, 0x00, 0x42);
        let hit = dc.hit_watchpoint.unwrap();
        assert_eq!(hit.address, 0xC000);
        assert_eq!(hit.old_value, 0x00);
        assert_eq!(hit.new_value, 0x42);
    }

    #[test]
    fn watchpoint_write_ignores_same_value() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0xC000, WatchType::Write);
        dc.check_watch_write(0xC000, 0x42, 0x42);
        assert!(dc.hit_watchpoint.is_none());
    }

    #[test]
    fn watchpoint_read_fires() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0xC000, WatchType::Read);
        dc.check_watch_read(0xC000, 0x55);
        let hit = dc.hit_watchpoint.unwrap();
        assert_eq!(hit.address, 0xC000);
    }

    #[test]
    fn duplicate_watchpoint_not_added() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0xC000, WatchType::Write);
        dc.add_watchpoint(0xC000, WatchType::Write);
        assert_eq!(dc.watchpoints.len(), 1);
    }

    #[test]
    fn clear_hits_resets_state() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x0100);
        dc.should_break(0x0100);
        dc.clear_hits();
        assert!(dc.hit_breakpoint.is_none());
        assert!(dc.hit_watchpoint.is_none());
    }
}

