use super::Bus;

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
        if self.oam_dma_blocks_cpu_access(addr) {
            if self.trace_cpu_accesses {
                self.cpu_read_trace.push((addr, 0xFF));
            }
            return 0xFF;
        }
        self.cpu_read_byte_unblocked(addr)
    }

    pub fn cpu_write_byte(&mut self, addr: u16, value: u8) -> u64 {
        if self.oam_dma_blocks_cpu_access(addr) {
            return 0;
        }
        self.cpu_write_byte_unblocked(addr, value)
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
