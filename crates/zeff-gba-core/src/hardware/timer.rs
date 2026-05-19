#[derive(Clone, Copy, Debug, Default)]
pub struct Timer {
    pub reload: u16,
    pub counter: u16,
    pub control: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timers {
    timers: [Timer; 4],
}

impl Timers {
    pub fn read16(&self, index: usize, control: bool) -> u16 {
        let timer = self.timers.get(index).copied().unwrap_or_default();
        if control {
            timer.control
        } else {
            timer.counter
        }
    }

    pub fn write16(&mut self, index: usize, control: bool, value: u16) {
        if let Some(timer) = self.timers.get_mut(index) {
            if control {
                timer.control = value & 0x00C7;
            } else {
                timer.reload = value;
                timer.counter = value;
            }
        }
    }

    pub fn all(&self) -> [Timer; 4] {
        self.timers
    }

    pub fn set_all(&mut self, timers: [Timer; 4]) {
        self.timers = timers;
    }
}
