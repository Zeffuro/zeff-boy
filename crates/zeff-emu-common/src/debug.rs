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

    #[inline]
    pub fn any_active(&self) -> bool {
        self.breakpoints_active || self.break_on_next || self.watchpoints_active
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

const OPCODE_LOG_CAPACITY: usize = 32;
const OPCODE_LOG_MASK: usize = OPCODE_LOG_CAPACITY - 1;

pub struct OpcodeLog<E: Copy + Default> {
    entries: [E; OPCODE_LOG_CAPACITY],
    cursor: usize,
    count: usize,
    pub enabled: bool,
}

impl<E: Copy + Default> std::fmt::Debug for OpcodeLog<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcodeLog")
            .field("count", &self.count)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl<E: Copy + Default> Default for OpcodeLog<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Copy + Default> OpcodeLog<E> {
    pub fn new() -> Self {
        Self {
            entries: core::array::from_fn(|_| E::default()),
            cursor: 0,
            count: 0,
            enabled: false,
        }
    }

    #[inline]
    pub fn push(&mut self, entry: E) {
        if !self.enabled {
            return;
        }
        self.entries[self.cursor] = entry;
        self.cursor = (self.cursor + 1) & OPCODE_LOG_MASK;
        if self.count < OPCODE_LOG_CAPACITY {
            self.count += 1;
        }
    }

    pub fn recent(&self, n: usize) -> Vec<E> {
        let take = n.min(self.count);
        let mut result = Vec::with_capacity(take);
        for i in 0..take {
            let idx = (self.cursor.wrapping_sub(1 + i)) & OPCODE_LOG_MASK;
            result.push(self.entries[idx]);
        }
        result
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.cursor = 0;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x1234);
        assert!(dc.has_breakpoint(0x1234));
        dc.remove_breakpoint(0x1234);
        assert!(!dc.has_breakpoint(0x1234));
    }

    #[test]
    fn toggle_breakpoint() {
        let mut dc = DebugController::new();
        assert!(!dc.has_breakpoint(0x100));
        dc.toggle_breakpoint(0x100);
        assert!(dc.has_breakpoint(0x100));
        dc.toggle_breakpoint(0x100);
        assert!(!dc.has_breakpoint(0x100));
    }

    #[test]
    fn should_break_on_breakpoint() {
        let mut dc = DebugController::new();
        dc.add_breakpoint(0x200);
        assert!(dc.should_break(0x200));
        assert_eq!(dc.hit_breakpoint, Some(0x200));
    }

    #[test]
    fn break_on_next() {
        let mut dc = DebugController::new();
        dc.break_on_next = true;
        assert!(dc.should_break(0x0));
        assert!(!dc.break_on_next);
    }

    #[test]
    fn watchpoint_write() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x300, WatchType::Write);
        assert!(dc.has_watchpoints());
        dc.check_watch_write(0x300, 10, 20);
        assert!(dc.hit_watchpoint.is_some());
        let hit = dc
            .hit_watchpoint
            .expect("watchpoint should have been triggered");
        assert_eq!(hit.address, 0x300);
        assert_eq!(hit.old_value, 10);
        assert_eq!(hit.new_value, 20);
    }

    #[test]
    fn watchpoint_no_trigger_on_same_value() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x400, WatchType::Write);
        dc.check_watch_write(0x400, 5, 5);
        assert!(dc.hit_watchpoint.is_none());
    }

    #[test]
    fn duplicate_watchpoint_not_added() {
        let mut dc = DebugController::new();
        dc.add_watchpoint(0x500, WatchType::Read);
        dc.add_watchpoint(0x500, WatchType::Read);
        assert_eq!(dc.watchpoints.len(), 1);
    }

    #[test]
    fn opcode_log_push_and_recent() {
        let mut log = OpcodeLog::<(u16, u8)>::new();
        log.set_enabled(true);
        log.push((0x100, 0xAB));
        log.push((0x102, 0xCD));
        let recent = log.recent(10);
        assert_eq!(recent, vec![(0x102, 0xCD), (0x100, 0xAB)]);
    }

    #[test]
    fn opcode_log_disabled_ignores_push() {
        let mut log = OpcodeLog::<(u16, u8)>::new();
        log.set_enabled(false);
        log.push((0x100, 0xAB));
        assert!(log.recent(10).is_empty());
    }

    #[test]
    fn opcode_log_clear_resets() {
        let mut log = OpcodeLog::<(u16, u8, bool)>::new();
        log.set_enabled(true);
        log.push((0x100, 0xAB, false));
        log.push((0x102, 0xCB, true));
        log.clear();
        assert!(log.recent(10).is_empty());
    }

    #[test]
    fn opcode_log_wraps_at_capacity() {
        let mut log = OpcodeLog::<(u16, u8)>::new();
        log.set_enabled(true);
        for i in 0..64u16 {
            log.push((i, i as u8));
        }
        let recent = log.recent(32);
        assert_eq!(recent.len(), 32);
        assert_eq!(recent[0], (63, 63));
        assert_eq!(recent[31], (32, 32));
    }
}
