use anyhow::{Result, bail, ensure};
use serde_json::{Value, json};
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::MasterTicks;

use zeff_audio_discovery::huge::isolation::IsolatedRom;

const ROM_END: usize = 0x8000;
const ADDRESS_COUNT: usize = 0x1_0000;

pub struct AccessAudit {
    readable_rom: Box<[bool; ROM_END]>,
    executable_rom: Box<[bool; ROM_END]>,
    initialized: Box<[bool; ADDRESS_COUNT]>,
    reads: Box<[u32; ADDRESS_COUNT]>,
    writes: Box<[u32; ADDRESS_COUNT]>,
    ram_start: u16,
    ram_end: u16,
    access_count: u64,
    last_at: Option<MasterTicks>,
    min_stack_write: Option<u16>,
}

impl AccessAudit {
    pub fn new(plan: &IsolatedRom) -> Self {
        let mut readable_rom = Box::new([false; ROM_END]);
        for span in plan.copied_spans.iter().chain(&plan.bootstrap_spans) {
            mark_span(
                &mut readable_rom,
                span.offset as usize,
                span.byte_len as usize,
            );
        }
        let mut executable_rom = Box::new([false; ROM_END]);
        for span in &plan.bootstrap_spans {
            mark_span(
                &mut executable_rom,
                span.offset as usize,
                span.byte_len as usize,
            );
        }
        mark_span(
            &mut executable_rom,
            usize::from(plan.driver_start),
            usize::from(plan.driver_end.saturating_sub(plan.driver_start)),
        );
        Self {
            readable_rom,
            executable_rom,
            initialized: Box::new([false; ADDRESS_COUNT]),
            reads: Box::new([0; ADDRESS_COUNT]),
            writes: Box::new([0; ADDRESS_COUNT]),
            ram_start: plan.ram_start,
            ram_end: plan.ram_end,
            access_count: 0,
            last_at: None,
            min_stack_write: None,
        }
    }

    pub fn observe(&mut self, event: BusAccessEvent) -> Result<()> {
        let (at, addr) = match event {
            BusAccessEvent::Read {
                at,
                space,
                addr,
                width,
                ..
            } => {
                validate_shape(space, width)?;
                (required_time(at)?, address(addr)?)
            }
            BusAccessEvent::Write {
                at,
                space,
                addr,
                width,
                ..
            } => {
                validate_shape(space, width)?;
                (required_time(at)?, address(addr)?)
            }
        };
        if self
            .last_at
            .is_some_and(|previous| at.get() < previous.get())
        {
            bail!("CPU access timestamps regress at {addr:#06x}");
        }
        self.last_at = Some(at);
        self.access_count += 1;
        match event {
            BusAccessEvent::Read { .. } => self.read(addr),
            BusAccessEvent::Write { .. } => self.write(addr),
        }
    }

    pub fn check_pc(&self, pc: u16) -> Result<()> {
        ensure!(
            usize::from(pc) < ROM_END && self.executable_rom[usize::from(pc)],
            "executed PC outside bootstrap/driver closure: {pc:#06x}"
        );
        Ok(())
    }

    pub fn report(&self) -> Value {
        json!({
            "bus_access_count": self.access_count,
            "ram_initialized": self.ram_initialized(),
            "min_stack_write_addr": self.min_stack_write,
            "read_ranges": compact_ranges(&self.reads),
            "write_ranges": compact_ranges(&self.writes),
        })
    }

    fn read(&mut self, addr: u16) -> Result<()> {
        match addr {
            0x0000..=0x7fff => ensure!(
                self.readable_rom[usize::from(addr)],
                "ROM read outside copied/bootstrap closure: {addr:#06x}"
            ),
            0xc000..=0xcfff => {
                self.require_driver_ram(addr)?;
                self.require_initialized(addr)?;
            }
            0xff80..=0xff81 | 0xffc0..=0xfffd => self.require_initialized(addr)?,
            0xff0f | 0xffff => {}
            0xff10..=0xff26 | 0xff30..=0xff3f => self.require_initialized(addr)?,
            _ => bail!("CPU read outside hUGE closure: {addr:#06x}"),
        }
        self.reads[usize::from(addr)] = self.reads[usize::from(addr)].saturating_add(1);
        Ok(())
    }

    fn write(&mut self, addr: u16) -> Result<()> {
        match addr {
            0x0000..=0x7fff => bail!("CPU ROM write outside MBC0 closure: {addr:#06x}"),
            0xc000..=0xcfff => {
                self.require_driver_ram(addr)?;
                self.initialized[usize::from(addr)] = true;
            }
            0xff80..=0xff81 => self.initialized[usize::from(addr)] = true,
            0xffc0..=0xfffd => {
                self.initialized[usize::from(addr)] = true;
                self.min_stack_write = Some(self.min_stack_write.map_or(addr, |min| min.min(addr)));
            }
            0xff0f | 0xffff => {}
            0xff10..=0xff26 | 0xff30..=0xff3f => self.initialized[usize::from(addr)] = true,
            _ => bail!("CPU write outside hUGE closure: {addr:#06x}"),
        }
        self.writes[usize::from(addr)] = self.writes[usize::from(addr)].saturating_add(1);
        Ok(())
    }

    fn require_driver_ram(&self, addr: u16) -> Result<()> {
        ensure!(
            (self.ram_start..self.ram_end).contains(&addr),
            "WRAM access outside hUGE state: {addr:#06x}"
        );
        Ok(())
    }

    fn require_initialized(&self, addr: u16) -> Result<()> {
        ensure!(
            self.initialized[usize::from(addr)],
            "CPU read before write: {addr:#06x}"
        );
        Ok(())
    }

    fn ram_initialized(&self) -> usize {
        (self.ram_start..self.ram_end)
            .filter(|&addr| self.initialized[usize::from(addr)])
            .count()
    }
}

fn validate_shape(space: TraceWriteKind, width: TraceWriteWidth) -> Result<()> {
    ensure!(
        space == TraceWriteKind::Memory,
        "non-memory CPU access trace event"
    );
    ensure!(
        width == TraceWriteWidth::Byte,
        "non-byte CPU access trace event"
    );
    Ok(())
}

fn required_time(at: Option<MasterTicks>) -> Result<MasterTicks> {
    at.ok_or_else(|| anyhow::anyhow!("CPU access trace event has no timestamp"))
}

fn address(addr: u32) -> Result<u16> {
    u16::try_from(addr)
        .map_err(|_| anyhow::anyhow!("CPU access address exceeds 16 bits: {addr:#x}"))
}

fn mark_span(target: &mut [bool; ROM_END], start: usize, len: usize) {
    let end = start.saturating_add(len).min(ROM_END);
    if start < end {
        target[start..end].fill(true);
    }
}

fn compact_ranges(counts: &[u32; ADDRESS_COUNT]) -> Vec<Value> {
    let mut ranges = Vec::new();
    let mut start = None;
    let mut total = 0u64;
    for (address, &count) in counts.iter().enumerate() {
        if count == 0 {
            if let Some(start) = start.take() {
                ranges.push(json!({"start": start, "end": address - 1, "count": total}));
                total = 0;
            }
        } else {
            start.get_or_insert(address);
            total += u64::from(count);
        }
    }
    if let Some(start) = start {
        ranges.push(json!({"start": start, "end": ADDRESS_COUNT - 1, "count": total}));
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_audio_discovery::tracker::FileSpan;

    fn plan() -> IsolatedRom {
        IsolatedRom {
            bytes: vec![0; ROM_END],
            copied_spans: vec![FileSpan {
                offset: 0x0200,
                byte_len: 3,
            }],
            bootstrap_spans: vec![
                FileSpan {
                    offset: 0x0040,
                    byte_len: 3,
                },
                FileSpan {
                    offset: 0x0100,
                    byte_len: 4,
                },
                FileSpan {
                    offset: 0x0150,
                    byte_len: 4,
                },
            ],
            descriptor: 0x0200,
            driver_start: 0x0300,
            driver_end: 0x0303,
            ram_start: 0xc000,
            ram_end: 0xc064,
            init_call: 0x0150,
            update_call: 0x0040,
        }
    }

    fn read(at: u64, addr: u16) -> BusAccessEvent {
        BusAccessEvent::Read {
            at: Some(MasterTicks::new(at)),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            value: 0,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    }

    fn write(at: u64, addr: u16) -> BusAccessEvent {
        BusAccessEvent::Write {
            at: Some(MasterTicks::new(at)),
            space: TraceWriteKind::Memory,
            addr: u32::from(addr),
            old_value: 0,
            written_value: 1,
            new_value: 1,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    }

    #[test]
    fn rejects_poisoned_rom_data_and_execution() {
        let mut audit = AccessAudit::new(&plan());
        assert!(audit.observe(read(1, 0x0200)).is_ok());
        assert!(audit.observe(read(2, 0x0203)).is_err());
        assert!(audit.check_pc(0x0200).is_err());
        assert!(audit.check_pc(0x0301).is_ok());
    }

    #[test]
    fn traps_read_before_write_and_invalid_memory() {
        let mut audit = AccessAudit::new(&plan());
        assert!(audit.observe(read(1, 0xc000)).is_err());
        assert!(audit.observe(write(2, 0xc000)).is_ok());
        assert!(audit.observe(read(3, 0xc000)).is_ok());
        assert!(audit.observe(read(4, 0xe000)).is_err());
        assert!(audit.observe(write(5, 0x8000)).is_err());
    }

    #[test]
    fn requires_sound_reads_to_follow_cpu_writes() {
        let mut audit = AccessAudit::new(&plan());
        assert!(audit.observe(read(1, 0xff25)).is_err());
        assert!(audit.observe(write(2, 0xff25)).is_ok());
        assert!(audit.observe(read(3, 0xff25)).is_ok());
        assert!(audit.observe(read(4, 0xff0f)).is_ok());
        assert!(audit.observe(write(5, 0xff80)).is_ok());
        assert!(audit.observe(read(6, 0xff80)).is_ok());
    }

    #[test]
    fn rejects_missing_time_regression_and_outside_memory_contract() {
        let mut audit = AccessAudit::new(&plan());
        let mut missing = read(1, 0x200);
        if let BusAccessEvent::Read { at, .. } = &mut missing {
            *at = None;
        }
        assert!(audit.observe(missing).is_err());
        for (space, width, addr) in [
            (TraceWriteKind::Io, TraceWriteWidth::Byte, 0x200),
            (TraceWriteKind::Memory, TraceWriteWidth::Word, 0x200),
            (TraceWriteKind::Memory, TraceWriteWidth::Byte, 0x10000),
        ] {
            assert!(
                audit
                    .observe(BusAccessEvent::Read {
                        at: Some(MasterTicks::new(1)),
                        space,
                        width,
                        addr,
                        value: 0,
                        mapped_addr: None
                    })
                    .is_err()
            );
        }
        assert!(audit.observe(read(10, 0x200)).is_ok());
        assert!(audit.observe(read(9, 0x200)).is_err());
        for addr in [0x200, 0xc064, 0xd000, 0xfe00, 0xff46, 0xff82, 0xfffe] {
            assert!(audit.observe(write(11, addr)).is_err(), "{addr:04x}");
        }
        assert!(audit.observe(write(12, 0xfffd)).is_ok());
        assert!(audit.observe(read(13, 0xfffd)).is_ok());
    }
}
