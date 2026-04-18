use super::Bus;
use crate::hardware::types::constants::{HRAM_END, HRAM_START};

pub enum CpuAccessTraceEvent {
    Read {
        addr: u16,
        value: u8,
    },
    Write {
        addr: u16,
        old_value: u8,
        new_value: u8,
    },
}

impl Bus {
    #[inline]
    pub fn cpu_read_byte(&mut self, addr: u16) -> u8 {
        if self.oam_dma_active && !is_hram_addr(addr) {
            return 0xFF;
        }
        let value = self.read_byte(addr);
        if self.trace_cpu_accesses {
            self.cpu_read_trace.push((addr, value));
        }
        value
    }

    pub fn cpu_write_byte(&mut self, addr: u16, value: u8) -> u64 {
        if self.oam_dma_active && !is_hram_addr(addr) {
            return 0;
        }
        let old_value = if self.trace_cpu_accesses {
            self.read_byte(addr)
        } else {
            0
        };
        let extra_t_cycles = self.write_byte(addr, value);
        if self.trace_cpu_accesses {
            let new_value = self.read_byte(addr);
            self.cpu_write_trace.push((addr, old_value, new_value));
        }
        extra_t_cycles
    }

    pub fn begin_cpu_access_trace(&mut self) {
        self.cpu_read_trace.clear();
        self.cpu_write_trace.clear();
    }

    pub fn drain_cpu_access_trace(&mut self, mut on_event: impl FnMut(CpuAccessTraceEvent)) {
        for &(addr, value) in &self.cpu_read_trace {
            on_event(CpuAccessTraceEvent::Read { addr, value });
        }
        for &(addr, old_value, new_value) in &self.cpu_write_trace {
            on_event(CpuAccessTraceEvent::Write {
                addr,
                old_value,
                new_value,
            });
        }
        self.cpu_read_trace.clear();
        self.cpu_write_trace.clear();
    }
}

fn is_hram_addr(addr: u16) -> bool {
    (HRAM_START..=HRAM_END).contains(&addr)
}
