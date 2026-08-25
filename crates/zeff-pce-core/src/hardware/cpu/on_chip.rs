use super::super::bus::{OPEN_BUS_VALUE, PhysicalRegion, decode_physical_region};
use super::{Cpu, CpuBus, CpuStep, CpuTrap, IrqPort, StatusFlags, TimerPort, VdcPort};
use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const TIMER_MASTER_TICKS: u64 = 3_072;
pub const UNINITIALIZED_TIMER_COUNTER_READ: u8 = 0;
pub const PROVISIONAL_INTERRUPT_ENTRY_CYCLES: u32 = 8;

const INTERRUPT_MASK: u8 = 0x07;
const IRQ2_BIT: u8 = 1 << 0;
const IRQ1_BIT: u8 = 1 << 1;
const TIMER_BIT: u8 = 1 << 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineLevel {
    Low,
    #[default]
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptSource {
    Nmi,
    Timer,
    Irq1,
    Irq2,
}

impl InterruptSource {
    #[inline]
    const fn vector_low(self) -> u16 {
        match self {
            Self::Nmi => 0xFFFC,
            Self::Timer => 0xFFFA,
            Self::Irq1 => 0xFFF8,
            Self::Irq2 => 0xFFF6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterruptStep {
    pub source: InterruptSource,
    pub cycles: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnChipIo {
    io_data_buffer: u8,
    timer_reload: u8,
    timer_counter: Option<u8>,
    timer_running: bool,
    timer_prescaler: u16,
    timer_irq_pending: bool,
    interrupt_disable: u8,
    irq1_line: LineLevel,
    irq2_line: LineLevel,
    nmi_line: LineLevel,
    nmi_pending: bool,
}

impl Default for OnChipIo {
    fn default() -> Self {
        Self::new()
    }
}

impl OnChipIo {
    pub const fn new() -> Self {
        Self {
            io_data_buffer: OPEN_BUS_VALUE,
            timer_reload: 0,
            timer_counter: None,
            timer_running: false,
            timer_prescaler: 0,
            timer_irq_pending: false,
            interrupt_disable: 0,
            irq1_line: LineLevel::High,
            irq2_line: LineLevel::High,
            nmi_line: LineLevel::High,
            nmi_pending: false,
        }
    }

    pub fn reset(&mut self) {
        let irq1_line = self.irq1_line;
        let irq2_line = self.irq2_line;
        let nmi_line = self.nmi_line;
        *self = Self {
            irq1_line,
            irq2_line,
            nmi_line,
            ..Self::new()
        };
    }

    #[inline]
    pub const fn io_data_buffer(&self) -> u8 {
        self.io_data_buffer
    }

    pub fn advance_master_ticks(&mut self, ticks: u64) {
        if !self.timer_running {
            return;
        }

        let phase = u64::from(self.timer_prescaler) + ticks % TIMER_MASTER_TICKS;
        let decrements = ticks / TIMER_MASTER_TICKS + phase / TIMER_MASTER_TICKS;
        self.timer_prescaler = (phase % TIMER_MASTER_TICKS) as u16;
        if decrements == 0 {
            return;
        }

        let counter = u64::from(
            self.timer_counter
                .expect("running timer always has a counter"),
        );
        if decrements <= counter {
            self.timer_counter = Some((counter - decrements) as u8);
            return;
        }

        self.timer_irq_pending = true;
        let remaining = decrements - counter - 1;
        let period = u64::from(self.timer_reload) + 1;
        let phase = remaining % period;
        self.timer_counter = Some((u64::from(self.timer_reload) - phase) as u8);
    }

    #[inline]
    pub fn read_timer_counter(&self) -> u8 {
        let Some(counter) = self.timer_counter else {
            return UNINITIALIZED_TIMER_COUNTER_READ;
        };
        let ticks_until_decrement = TIMER_MASTER_TICKS - u64::from(self.timer_prescaler);
        if self.timer_running && counter == 0 && ticks_until_decrement <= 15 {
            0x7F
        } else {
            counter
        }
    }

    pub fn write_timer(&mut self, port: TimerPort, value: u8) {
        match port {
            TimerPort::CounterReload => self.timer_reload = value & 0x7F,
            TimerPort::Control => {
                let running = value & 1 != 0;
                if running && !self.timer_running {
                    self.timer_counter = Some(self.timer_reload);
                }
                if !running {
                    self.timer_prescaler = 0;
                }
                self.timer_running = running;
            }
        }
    }

    #[inline]
    pub const fn timer_reload(&self) -> u8 {
        self.timer_reload
    }

    #[inline]
    pub const fn timer_running(&self) -> bool {
        self.timer_running
    }

    #[inline]
    pub const fn timer_prescaler_ticks(&self) -> u16 {
        self.timer_prescaler
    }

    #[inline]
    pub const fn timer_irq_pending(&self) -> bool {
        self.timer_irq_pending
    }

    #[inline]
    pub const fn read_irq(&self, port: IrqPort) -> u8 {
        match port {
            IrqPort::Disable => self.interrupt_disable,
            IrqPort::Request => {
                (if matches!(self.irq2_line, LineLevel::Low) {
                    IRQ2_BIT
                } else {
                    0
                }) | (if matches!(self.irq1_line, LineLevel::Low) {
                    IRQ1_BIT
                } else {
                    0
                }) | (if self.timer_irq_pending { TIMER_BIT } else { 0 })
            }
        }
    }

    pub fn write_irq(&mut self, port: IrqPort, value: u8) {
        match port {
            IrqPort::Disable => self.interrupt_disable = value & INTERRUPT_MASK,
            IrqPort::Request => self.timer_irq_pending = false,
        }
    }

    #[inline]
    pub fn set_irq1_line(&mut self, level: LineLevel) {
        self.irq1_line = level;
    }

    #[inline]
    pub fn set_irq2_line(&mut self, level: LineLevel) {
        self.irq2_line = level;
    }

    pub fn set_nmi_line(&mut self, level: LineLevel) {
        if self.nmi_line == LineLevel::High && level == LineLevel::Low {
            self.nmi_pending = true;
        }
        self.nmi_line = level;
    }

    #[inline]
    pub const fn unmasked_request_pending(&self, source: InterruptSource) -> bool {
        match source {
            InterruptSource::Nmi => self.nmi_pending,
            InterruptSource::Timer => {
                self.timer_irq_pending && self.interrupt_disable & TIMER_BIT == 0
            }
            InterruptSource::Irq1 => {
                matches!(self.irq1_line, LineLevel::Low) && self.interrupt_disable & IRQ1_BIT == 0
            }
            InterruptSource::Irq2 => {
                matches!(self.irq2_line, LineLevel::Low) && self.interrupt_disable & IRQ2_BIT == 0
            }
        }
    }

    pub const fn highest_priority_unmasked_request(&self) -> Option<InterruptSource> {
        if self.unmasked_request_pending(InterruptSource::Nmi) {
            Some(InterruptSource::Nmi)
        } else if self.unmasked_request_pending(InterruptSource::Timer) {
            Some(InterruptSource::Timer)
        } else if self.unmasked_request_pending(InterruptSource::Irq1) {
            Some(InterruptSource::Irq1)
        } else if self.unmasked_request_pending(InterruptSource::Irq2) {
            Some(InterruptSource::Irq2)
        } else {
            None
        }
    }

    #[inline]
    pub const fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    fn consume_nmi_pending(&mut self) -> bool {
        std::mem::take(&mut self.nmi_pending)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HuC6280 {
    cpu: Cpu,
    on_chip_io: OnChipIo,
    interrupt_poll_disable: Option<bool>,
    sampled_interrupt: Option<InterruptSource>,
}

impl Default for HuC6280 {
    fn default() -> Self {
        Self::new()
    }
}

impl HuC6280 {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            on_chip_io: OnChipIo::new(),
            interrupt_poll_disable: None,
            sampled_interrupt: None,
        }
    }

    pub fn reset<B: CpuBus>(&mut self, bus: &mut B) {
        self.on_chip_io.reset();
        self.interrupt_poll_disable = None;
        self.sampled_interrupt = None;
        let mut bus = OnChipBus::new(&mut self.on_chip_io, bus);
        self.cpu.reset(&mut bus);
    }

    pub(crate) fn step_instruction<B: CpuBus>(&mut self, bus: &mut B) -> Result<CpuStep, CpuTrap> {
        assert!(self.interrupt_poll_disable.is_none());
        assert!(self.sampled_interrupt.is_none());
        let old_interrupt_disable = self.cpu.registers().status.contains(StatusFlags::INTERRUPT);
        let mut bus = OnChipBus::new(&mut self.on_chip_io, bus);
        let step = self.cpu.step(&mut bus)?;
        let final_interrupt_disable = self.cpu.registers().status.contains(StatusFlags::INTERRUPT);
        self.interrupt_poll_disable = Some(match step.opcode {
            0x28 | 0x58 | 0x78 => old_interrupt_disable,
            0x40 => final_interrupt_disable,
            _ => final_interrupt_disable,
        });
        Ok(step)
    }

    pub fn debug_write_logical<B: CpuBus>(&mut self, bus: &mut B, logical_addr: u16, value: u8) {
        let mut bus = OnChipBus::new(&mut self.on_chip_io, bus);
        self.cpu.write(&mut bus, logical_addr, value);
    }

    pub(crate) fn service_interrupt_boundary<B: CpuBus>(
        &mut self,
        bus: &mut B,
    ) -> Option<InterruptStep> {
        assert!(self.interrupt_poll_disable.is_none());
        let source = self.sampled_interrupt.take()?;

        let mut bus = OnChipBus::new(&mut self.on_chip_io, bus);
        self.cpu
            .enter_hardware_interrupt_provisional(&mut bus, source.vector_low());
        self.interrupt_poll_disable = Some(true);
        Some(InterruptStep {
            source,
            cycles: PROVISIONAL_INTERRUPT_ENTRY_CYCLES,
        })
    }

    pub(crate) fn sample_interrupts_after_action(&mut self) {
        let interrupt_disable = self
            .interrupt_poll_disable
            .take()
            .expect("completed CPU action provides an interrupt poll state");
        self.sample_interrupts(interrupt_disable);
    }

    fn sample_interrupts(&mut self, interrupt_disable: bool) {
        assert!(self.sampled_interrupt.is_none());
        self.sampled_interrupt = if self.on_chip_io.nmi_pending() {
            let consumed = self.on_chip_io.consume_nmi_pending();
            debug_assert!(consumed);
            Some(InterruptSource::Nmi)
        } else if interrupt_disable {
            None
        } else {
            self.on_chip_io.highest_priority_unmasked_request()
        };
    }

    #[cfg(test)]
    pub(super) fn sample_interrupts_for_test(&mut self, interrupt_disable: bool) {
        self.interrupt_poll_disable = None;
        self.sample_interrupts(interrupt_disable);
    }

    #[inline]
    pub const fn sampled_interrupt(&self) -> Option<InterruptSource> {
        self.sampled_interrupt
    }

    pub(crate) fn replace_sampled_interrupt(
        &mut self,
        sampled_interrupt: Option<InterruptSource>,
    ) -> Option<InterruptSource> {
        std::mem::replace(&mut self.sampled_interrupt, sampled_interrupt)
    }

    #[inline]
    pub const fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    #[inline]
    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    #[inline]
    pub const fn on_chip_io(&self) -> &OnChipIo {
        &self.on_chip_io
    }

    #[inline]
    pub fn on_chip_io_mut(&mut self) -> &mut OnChipIo {
        &mut self.on_chip_io
    }

    #[inline]
    pub fn advance_master_ticks(&mut self, ticks: u64) {
        self.on_chip_io.advance_master_ticks(ticks);
    }

    #[inline]
    pub fn set_irq1_line(&mut self, level: LineLevel) {
        self.on_chip_io.set_irq1_line(level);
    }

    #[inline]
    pub fn set_irq2_line(&mut self, level: LineLevel) {
        self.on_chip_io.set_irq2_line(level);
    }

    #[inline]
    pub fn set_nmi_line(&mut self, level: LineLevel) {
        self.on_chip_io.set_nmi_line(level);
    }

    #[inline]
    pub const fn highest_priority_unmasked_request(&self) -> Option<InterruptSource> {
        self.on_chip_io.highest_priority_unmasked_request()
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter, state_version: u32) {
        let registers = self.cpu.registers();
        writer.write_u8(registers.a);
        writer.write_u8(registers.x);
        writer.write_u8(registers.y);
        writer.write_u8(registers.sp);
        writer.write_u16(registers.pc);
        writer.write_u8(registers.status.bits());
        writer.write_bytes(&self.cpu.mapping_registers());
        writer.write_u8(match self.cpu.speed_mode() {
            super::SpeedMode::Low => 0,
            super::SpeedMode::High => 1,
        });

        writer.write_u8(self.on_chip_io.timer_reload);
        write_option_u8(writer, self.on_chip_io.timer_counter);
        writer.write_bool(self.on_chip_io.timer_running);
        writer.write_u16(self.on_chip_io.timer_prescaler);
        writer.write_bool(self.on_chip_io.timer_irq_pending);
        writer.write_u8(self.on_chip_io.interrupt_disable);
        writer.write_u8(line_level_to_tag(self.on_chip_io.irq1_line));
        writer.write_u8(line_level_to_tag(self.on_chip_io.irq2_line));
        writer.write_u8(line_level_to_tag(self.on_chip_io.nmi_line));
        writer.write_bool(self.on_chip_io.nmi_pending);
        if state_version >= 2 {
            writer.write_u8(self.on_chip_io.io_data_buffer);
        }
        write_option_bool(writer, self.interrupt_poll_disable);
        writer.write_u8(interrupt_to_tag(self.sampled_interrupt));
    }

    pub(crate) const fn at_action_boundary(&self) -> bool {
        self.interrupt_poll_disable.is_none()
    }

    pub(crate) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
        state_version: u32,
    ) -> anyhow::Result<()> {
        let registers = self.cpu.registers_mut();
        registers.a = reader.read_u8()?;
        registers.x = reader.read_u8()?;
        registers.y = reader.read_u8()?;
        registers.sp = reader.read_u8()?;
        registers.pc = reader.read_u16()?;
        registers.status = StatusFlags::from_bits_retain(reader.read_u8()?);
        let mut mpr = [0; 8];
        reader.read_exact(&mut mpr)?;
        for (index, value) in mpr.into_iter().enumerate() {
            self.cpu.set_mapping_register(index, value);
        }
        self.cpu.set_speed_mode(match reader.read_u8()? {
            0 => super::SpeedMode::Low,
            1 => super::SpeedMode::High,
            tag => bail!("invalid HuC6280 speed-mode tag in save-state: {tag}"),
        });

        let timer_reload = reader.read_u8()?;
        if timer_reload > 0x7F {
            bail!("invalid HuC6280 timer reload in save-state: {timer_reload}");
        }
        let timer_counter = read_option_u8(reader)?;
        if timer_counter.is_some_and(|counter| counter > 0x7F) {
            bail!("invalid HuC6280 timer counter in save-state");
        }
        let timer_running = reader.read_bool()?;
        if timer_running && timer_counter.is_none() {
            bail!("running HuC6280 timer has no counter in save-state");
        }
        let timer_prescaler = reader.read_u16()?;
        if timer_prescaler >= TIMER_MASTER_TICKS as u16 {
            bail!("invalid HuC6280 timer prescaler in save-state: {timer_prescaler}");
        }
        let timer_irq_pending = reader.read_bool()?;
        let interrupt_disable = reader.read_u8()?;
        if interrupt_disable & !INTERRUPT_MASK != 0 {
            bail!("invalid HuC6280 interrupt mask in save-state: {interrupt_disable}");
        }
        let irq1_line = tag_to_line_level(reader.read_u8()?)?;
        let irq2_line = tag_to_line_level(reader.read_u8()?)?;
        let nmi_line = tag_to_line_level(reader.read_u8()?)?;
        let nmi_pending = reader.read_bool()?;
        let io_data_buffer = if state_version >= 2 {
            reader.read_u8()?
        } else {
            OPEN_BUS_VALUE
        };
        self.on_chip_io = OnChipIo {
            io_data_buffer,
            timer_reload,
            timer_counter,
            timer_running,
            timer_prescaler,
            timer_irq_pending,
            interrupt_disable,
            irq1_line,
            irq2_line,
            nmi_line,
            nmi_pending,
        };
        self.interrupt_poll_disable = read_option_bool(reader)?;
        self.sampled_interrupt = tag_to_interrupt(reader.read_u8()?)?;
        Ok(())
    }
}

fn write_option_u8(writer: &mut StateWriter, value: Option<u8>) {
    match value {
        None => writer.write_u8(0),
        Some(value) => {
            writer.write_u8(1);
            writer.write_u8(value);
        }
    }
}

fn read_option_u8(reader: &mut StateReader<'_>) -> anyhow::Result<Option<u8>> {
    Ok(match reader.read_u8()? {
        0 => None,
        1 => Some(reader.read_u8()?),
        tag => bail!("invalid optional-byte tag in HuC6280 save-state: {tag}"),
    })
}

fn write_option_bool(writer: &mut StateWriter, value: Option<bool>) {
    writer.write_u8(match value {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    });
}

fn read_option_bool(reader: &mut StateReader<'_>) -> anyhow::Result<Option<bool>> {
    Ok(match reader.read_u8()? {
        0 => None,
        1 => Some(false),
        2 => Some(true),
        tag => bail!("invalid optional-boolean tag in HuC6280 save-state: {tag}"),
    })
}

const fn line_level_to_tag(level: LineLevel) -> u8 {
    match level {
        LineLevel::Low => 0,
        LineLevel::High => 1,
    }
}

fn tag_to_line_level(tag: u8) -> anyhow::Result<LineLevel> {
    Ok(match tag {
        0 => LineLevel::Low,
        1 => LineLevel::High,
        _ => bail!("invalid HuC6280 line-level tag in save-state: {tag}"),
    })
}

const fn interrupt_to_tag(source: Option<InterruptSource>) -> u8 {
    match source {
        None => 0,
        Some(InterruptSource::Nmi) => 1,
        Some(InterruptSource::Timer) => 2,
        Some(InterruptSource::Irq1) => 3,
        Some(InterruptSource::Irq2) => 4,
    }
}

fn tag_to_interrupt(tag: u8) -> anyhow::Result<Option<InterruptSource>> {
    Ok(match tag {
        0 => None,
        1 => Some(InterruptSource::Nmi),
        2 => Some(InterruptSource::Timer),
        3 => Some(InterruptSource::Irq1),
        4 => Some(InterruptSource::Irq2),
        _ => bail!("invalid HuC6280 sampled-interrupt tag in save-state: {tag}"),
    })
}

struct OnChipBus<'a, B> {
    on_chip_io: &'a mut OnChipIo,
    inner: &'a mut B,
}

impl<'a, B> OnChipBus<'a, B> {
    #[inline]
    fn new(on_chip_io: &'a mut OnChipIo, inner: &'a mut B) -> Self {
        Self { on_chip_io, inner }
    }
}

impl<B: CpuBus> OnChipBus<'_, B> {
    fn advance_elapsed_time(&mut self) {
        self.on_chip_io
            .advance_master_ticks(self.inner.take_elapsed_master_ticks());
    }

    fn internal_read(&mut self, physical_addr: u32, dummy: bool) -> Option<u8> {
        if !matches!(
            decode_physical_region(physical_addr),
            PhysicalRegion::Timer(_) | PhysicalRegion::Irq(_)
        ) {
            return None;
        }
        let completed = self.inner.advance_internal_access(physical_addr, false);
        self.advance_elapsed_time();
        if !completed {
            return Some(OPEN_BUS_VALUE);
        }
        let value = match decode_physical_region(physical_addr) {
            PhysicalRegion::Timer(TimerPort::CounterReload) => {
                self.on_chip_io.read_timer_counter() | (self.on_chip_io.io_data_buffer & 0x80)
            }
            PhysicalRegion::Timer(TimerPort::Control) => OPEN_BUS_VALUE,
            PhysicalRegion::Irq(port) => {
                self.on_chip_io.read_irq(port) | (self.on_chip_io.io_data_buffer & 0xF8)
            }
            _ => unreachable!(),
        };
        self.on_chip_io.io_data_buffer = value;
        self.inner
            .observe_internal_read(physical_addr, value, dummy);
        Some(value)
    }

    fn internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) -> bool {
        if !matches!(
            decode_physical_region(physical_addr),
            PhysicalRegion::Timer(_) | PhysicalRegion::Irq(_)
        ) {
            return false;
        }
        let completed = self.inner.advance_internal_access(physical_addr, true);
        self.advance_elapsed_time();
        if !completed {
            return true;
        }
        match decode_physical_region(physical_addr) {
            PhysicalRegion::Timer(port) => self.on_chip_io.write_timer(port, value),
            PhysicalRegion::Irq(port) => self.on_chip_io.write_irq(port, value),
            _ => unreachable!(),
        }
        self.on_chip_io.io_data_buffer = value;
        self.inner
            .observe_internal_write(physical_addr, value, dummy);
        true
    }
}

impl<B: CpuBus> CpuBus for OnChipBus<'_, B> {
    fn read(&mut self, physical_addr: u32) -> u8 {
        if let Some(value) = self.internal_read(physical_addr, false) {
            value
        } else {
            let value = self.inner.read(physical_addr);
            self.advance_elapsed_time();
            match decode_physical_region(physical_addr) {
                PhysicalRegion::Psg(_) => self.on_chip_io.io_data_buffer,
                PhysicalRegion::Controller => {
                    self.on_chip_io.io_data_buffer = value;
                    value
                }
                _ => value,
            }
        }
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        if !self.internal_write(physical_addr, value, false) {
            self.inner.write(physical_addr, value);
            self.advance_elapsed_time();
            if matches!(
                decode_physical_region(physical_addr),
                PhysicalRegion::Psg(_) | PhysicalRegion::Controller
            ) {
                self.on_chip_io.io_data_buffer = value;
            }
        }
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        if let Some(value) = self.internal_read(physical_addr, true) {
            value
        } else {
            let value = self.inner.dummy_read(physical_addr);
            self.advance_elapsed_time();
            match decode_physical_region(physical_addr) {
                PhysicalRegion::Psg(_) => self.on_chip_io.io_data_buffer,
                PhysicalRegion::Controller => {
                    self.on_chip_io.io_data_buffer = value;
                    value
                }
                _ => value,
            }
        }
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        if !self.internal_write(physical_addr, value, true) {
            self.inner.dummy_write(physical_addr, value);
            self.advance_elapsed_time();
            if matches!(
                decode_physical_region(physical_addr),
                PhysicalRegion::Psg(_) | PhysicalRegion::Controller
            ) {
                self.on_chip_io.io_data_buffer = value;
            }
        }
    }

    #[inline]
    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        self.inner.write_vdc(port, value);
        self.advance_elapsed_time();
    }

    #[inline]
    fn observe_internal_read(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_read(physical_addr, value, dummy);
    }

    #[inline]
    fn observe_internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_write(physical_addr, value, dummy);
    }

    #[inline]
    fn observe_logical_read(
        &mut self,
        logical_addr: u16,
        physical_addr: u32,
        value: u8,
        dummy: bool,
    ) {
        self.inner
            .observe_logical_read(logical_addr, physical_addr, value, dummy);
    }

    #[inline]
    fn observe_logical_write(
        &mut self,
        logical_addr: u16,
        physical_addr: u32,
        value: u8,
        dummy: bool,
    ) {
        self.inner
            .observe_logical_write(logical_addr, physical_addr, value, dummy);
    }

    #[inline]
    fn observe_instruction_byte(&mut self, logical_addr: u16, physical_addr: u32, value: u8) {
        self.inner
            .observe_instruction_byte(logical_addr, physical_addr, value);
    }

    #[inline]
    fn idle(&mut self) {
        self.inner.idle();
        self.advance_elapsed_time();
    }
}
