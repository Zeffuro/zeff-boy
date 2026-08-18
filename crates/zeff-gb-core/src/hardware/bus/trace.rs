use super::Bus;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::MasterTicks;

pub type CpuAccessTraceEvent = BusAccessEvent;

impl Bus {
    #[inline]
    pub fn cpu_read_byte(&mut self, addr: u16) -> u8 {
        if self.oam_dma_blocks_cpu_access(addr) {
            if self.trace_cpu_accesses {
                self.trace_cpu_read(0, addr, 0xFF);
            }
            return 0xFF;
        }
        self.cpu_read_byte_unblocked(addr, 0)
    }

    pub fn cpu_write_byte(&mut self, addr: u16, value: u8) -> u64 {
        if self.oam_dma_blocks_cpu_access(addr) {
            return 0;
        }
        self.cpu_write_byte_unblocked(addr, value, 0)
    }

    pub fn begin_cpu_access_trace(&mut self) {
        self.begin_cpu_access_trace_at(MasterTicks::ZERO);
    }

    pub fn begin_cpu_access_trace_at(&mut self, at: MasterTicks) {
        self.cpu_access_trace_origin = at;
        self.cpu_access_trace.clear();
    }

    pub fn drain_cpu_access_trace(&mut self, mut on_event: impl FnMut(CpuAccessTraceEvent)) {
        self.cpu_access_trace.drain(..).for_each(&mut on_event);
    }

    #[inline]
    pub(super) fn traces_cpu_writes(&self) -> bool {
        self.trace_cpu_accesses || self.trace_cpu_writes
    }

    pub(super) fn trace_cpu_read(&mut self, offset: u64, addr: u16, value: u8) {
        let at = MasterTicks::new(self.cpu_access_trace_origin.get().wrapping_add(offset));
        self.cpu_access_trace.push(CpuAccessTraceEvent::Read {
            at: Some(at),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            value: u32::from(value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
    }

    pub(super) fn trace_cpu_write(
        &mut self,
        offset: u64,
        addr: u16,
        old_value: u8,
        written_value: u8,
        new_value: u8,
    ) {
        let at = MasterTicks::new(self.cpu_access_trace_origin.get().wrapping_add(offset));
        self.cpu_access_trace.push(CpuAccessTraceEvent::Write {
            at: Some(at),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            old_value: u32::from(old_value),
            written_value: u32::from(written_value),
            new_value: u32::from(new_value),
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        });
    }
}
