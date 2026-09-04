use std::cell::RefCell;

use anyhow::bail;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::{IoWriteCycle, Z80Bus};

use crate::ExpansionHardware;
use crate::constants::{
    BIOS_END, BIOS_SIZE, BIOS_START, CARTRIDGE_END, CARTRIDGE_START, CONTROLLER_PSG_PORT_END,
    CONTROLLER_PSG_PORT_START, EXPANSION_END, EXPANSION_START, IO_OPEN_BUS_VALUE,
    JOYSTICK_MODE_PORT_END, JOYSTICK_MODE_PORT_START, KEYPAD_MODE_PORT_END, KEYPAD_MODE_PORT_START,
    MEMORY_OPEN_BUS_VALUE, VDP_PORT_END, VDP_PORT_START, WORK_RAM_END, WORK_RAM_SIZE,
    WORK_RAM_START,
};
use crate::input::ControllerPorts;
use crate::psg::Psg;
use crate::vdp::Vdp;

const VDP_PORT_SELECT_MASK: u8 = 0x01;
const VDP_DATA_PORT_SELECT: u8 = 0x00;
const CONTROLLER_INDEX_SHIFT: u8 = 1;
const CONTROLLER_INDEX_MASK: u8 = 0x01;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CpuAccessTraceMode {
    #[default]
    None,
    Writes,
    All,
}

#[derive(Clone)]
pub struct Bus {
    bios: Box<[u8; BIOS_SIZE]>,
    cartridge: Vec<u8>,
    work_ram: [u8; WORK_RAM_SIZE],
    input: ControllerPorts,
    vdp: Vdp,
    psg: Psg,
    nmi_line: bool,
    nmi_pending: bool,
    pending_psg_write_cycle: Option<IoWriteCycle>,
    debug_trace_mode: CpuAccessTraceMode,
    debug_trace_events: RefCell<Vec<BusAccessEvent>>,
}

pub(crate) struct TracingBus<'a>(&'a mut Bus);

impl Bus {
    pub fn new(bios: &[u8], cartridge: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let bios: Box<[u8; BIOS_SIZE]> =
            bios.to_vec()
                .into_boxed_slice()
                .try_into()
                .map_err(|bytes: Box<[u8]>| {
                    anyhow::anyhow!(
                        "ColecoVision BIOS must be exactly {BIOS_SIZE} bytes, got {}",
                        bytes.len()
                    )
                })?;
        Ok(Self {
            bios,
            cartridge: cartridge.to_vec(),
            work_ram: [0; WORK_RAM_SIZE],
            input: ControllerPorts::new(),
            vdp: Vdp::new(),
            psg: Psg::new_with_sample_rate(sample_rate),
            nmi_line: false,
            nmi_pending: false,
            pending_psg_write_cycle: None,
            debug_trace_mode: CpuAccessTraceMode::None,
            debug_trace_events: RefCell::new(Vec::new()),
        })
    }

    pub fn reset(&mut self) {
        self.work_ram.fill(0);
        self.input = ControllerPorts::new();
        self.vdp.reset();
        self.psg.reset();
        self.nmi_line = false;
        self.nmi_pending = false;
        self.pending_psg_write_cycle = None;
        self.debug_trace_mode = CpuAccessTraceMode::None;
        self.debug_trace_events.borrow_mut().clear();
    }

    #[inline]
    pub fn cpu_read(&self, addr: u16) -> u8 {
        self.cpu_peek(addr)
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            BIOS_START..=BIOS_END => self.bios[usize::from(addr - BIOS_START)],
            EXPANSION_START..=EXPANSION_END => MEMORY_OPEN_BUS_VALUE,
            WORK_RAM_START..=WORK_RAM_END => self.work_ram[usize::from(addr) & (WORK_RAM_SIZE - 1)],
            CARTRIDGE_START..=CARTRIDGE_END => self
                .cartridge
                .get(usize::from(addr - CARTRIDGE_START))
                .copied()
                .unwrap_or(MEMORY_OPEN_BUS_VALUE),
        }
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, value: u8) {
        if (WORK_RAM_START..=WORK_RAM_END).contains(&addr) {
            self.work_ram[usize::from(addr) & (WORK_RAM_SIZE - 1)] = value;
        }
    }

    #[inline]
    pub fn io_read(&mut self, port: u8) -> u8 {
        let value = match port {
            VDP_PORT_START..=VDP_PORT_END
                if port & VDP_PORT_SELECT_MASK == VDP_DATA_PORT_SELECT =>
            {
                self.vdp.read_data()
            }
            VDP_PORT_START..=VDP_PORT_END => self.vdp.read_status(),
            CONTROLLER_PSG_PORT_START..=CONTROLLER_PSG_PORT_END => self
                .input
                .read_player(usize::from(
                    (port >> CONTROLLER_INDEX_SHIFT) & CONTROLLER_INDEX_MASK,
                ))
                .unwrap_or(IO_OPEN_BUS_VALUE),
            _ => IO_OPEN_BUS_VALUE,
        };
        self.refresh_nmi_edge();
        value
    }

    #[inline]
    pub fn io_write(&mut self, port: u8, value: u8) {
        match port {
            KEYPAD_MODE_PORT_START..=KEYPAD_MODE_PORT_END
            | JOYSTICK_MODE_PORT_START..=JOYSTICK_MODE_PORT_END => {
                self.input.write_output_port(port);
            }
            VDP_PORT_START..=VDP_PORT_END
                if port & VDP_PORT_SELECT_MASK == VDP_DATA_PORT_SELECT =>
            {
                self.vdp.write_data(value);
            }
            VDP_PORT_START..=VDP_PORT_END => self.vdp.write_control(value),
            CONTROLLER_PSG_PORT_START..=CONTROLLER_PSG_PORT_END => self.psg.write(value),
            _ => {}
        }
        self.refresh_nmi_edge();
    }

    pub(crate) fn is_psg_write_port(port: u8) -> bool {
        (CONTROLLER_PSG_PORT_START..=CONTROLLER_PSG_PORT_END).contains(&port)
    }

    pub(crate) fn take_pending_psg_write_cycle(&mut self) -> Option<IoWriteCycle> {
        self.pending_psg_write_cycle.take()
    }

    pub fn step_cycles(&mut self, cycles: u32) -> u64 {
        let before = self.vdp.frame_count();
        self.vdp.step_cycles(cycles);
        self.psg.step_cycles(cycles);
        self.refresh_nmi_edge();
        self.vdp.frame_count().wrapping_sub(before)
    }

    pub const fn work_ram(&self) -> &[u8; WORK_RAM_SIZE] {
        &self.work_ram
    }

    pub fn cartridge(&self) -> &[u8] {
        &self.cartridge
    }

    pub const fn expansion_hardware(&self) -> ExpansionHardware {
        ExpansionHardware::Absent
    }

    pub fn input(&self) -> &ControllerPorts {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut ControllerPorts {
        &mut self.input
    }

    pub fn vdp(&self) -> &Vdp {
        &self.vdp
    }

    pub fn vdp_mut(&mut self) -> &mut Vdp {
        &mut self.vdp
    }

    pub fn psg(&self) -> &Psg {
        &self.psg
    }

    pub fn psg_mut(&mut self) -> &mut Psg {
        &mut self.psg
    }

    pub fn rom_offset_for_cpu_address(&self, addr: u16) -> Option<usize> {
        let offset = usize::from(addr.checked_sub(CARTRIDGE_START)?);
        (offset < self.cartridge.len()).then_some(offset)
    }

    pub const fn rom_mapping_token(&self) -> u64 {
        0
    }

    pub(crate) fn tracing(&mut self) -> TracingBus<'_> {
        TracingBus(self)
    }

    pub(crate) fn begin_cpu_access_trace(&mut self) {
        self.debug_trace_mode = CpuAccessTraceMode::All;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub(crate) fn begin_cpu_write_trace(&mut self) {
        self.debug_trace_mode = CpuAccessTraceMode::Writes;
        self.debug_trace_events.borrow_mut().clear();
    }

    pub(crate) fn drain_cpu_access_trace(&mut self) -> Vec<BusAccessEvent> {
        self.debug_trace_mode = CpuAccessTraceMode::None;
        std::mem::take(&mut *self.debug_trace_events.borrow_mut())
    }

    pub(crate) fn recycle_cpu_access_trace(&mut self, mut events: Vec<BusAccessEvent>) {
        events.clear();
        *self.debug_trace_events.borrow_mut() = events;
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_vec(&self.work_ram);
        self.input.write_state(w);
        self.vdp.write_state(w);
        self.psg.write_state(w);
        w.write_bool(self.nmi_line);
        w.write_bool(self.nmi_pending);
    }

    pub(crate) fn write_portable_ram_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.work_ram);
    }

    pub(crate) fn read_portable_ram_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        let mut work_ram = [0; WORK_RAM_SIZE];
        r.read_exact(&mut work_ram)?;
        self.work_ram = work_ram;
        Ok(())
    }

    pub(crate) fn write_portable_input_state(&self, w: &mut StateWriter) {
        self.input.write_state(w);
    }

    pub(crate) fn read_portable_input_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.input.read_state(r)
    }

    pub(crate) fn write_portable_vdp_state(&self, w: &mut StateWriter) {
        self.vdp.write_state(w);
    }

    pub(crate) fn read_portable_vdp_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.vdp.read_state(r)
    }

    pub(crate) fn write_portable_psg_state(&self, w: &mut StateWriter) {
        self.psg.write_state(w);
    }

    pub(crate) fn read_portable_psg_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.psg.read_state(r)
    }

    pub(crate) fn write_portable_interrupt_state(&self, w: &mut StateWriter) {
        w.write_bool(self.nmi_line);
        w.write_bool(self.nmi_pending);
    }

    pub(crate) fn read_portable_interrupt_state(
        &mut self,
        r: &mut StateReader<'_>,
    ) -> anyhow::Result<()> {
        self.nmi_line = r.read_bool()?;
        self.nmi_pending = r.read_bool()?;
        if self.nmi_line != self.vdp.nmi_line() {
            bail!("ColecoVision portable state contains an inconsistent VDP NMI line");
        }
        self.pending_psg_write_cycle = None;
        self.debug_trace_mode = CpuAccessTraceMode::None;
        self.debug_trace_events.borrow_mut().clear();
        Ok(())
    }

    pub fn read_state(&mut self, r: &mut StateReader<'_>) -> anyhow::Result<()> {
        let work_ram = r.read_vec(WORK_RAM_SIZE)?;
        if work_ram.len() != WORK_RAM_SIZE {
            bail!(
                "ColecoVision save-state RAM size mismatch: expected {WORK_RAM_SIZE}, got {}",
                work_ram.len()
            );
        }
        self.work_ram.copy_from_slice(&work_ram);
        self.input.read_state(r)?;
        self.vdp.read_state(r)?;
        self.psg.read_state(r)?;
        self.nmi_line = r.read_bool()?;
        self.nmi_pending = r.read_bool()?;
        self.pending_psg_write_cycle = None;
        if self.nmi_line != self.vdp.nmi_line() {
            bail!("ColecoVision save-state contains an inconsistent VDP NMI line");
        }
        self.debug_trace_mode = CpuAccessTraceMode::None;
        self.debug_trace_events.borrow_mut().clear();
        Ok(())
    }

    fn refresh_nmi_edge(&mut self) {
        let line = self.vdp.nmi_line();
        if line && !self.nmi_line {
            self.nmi_pending = true;
        }
        self.nmi_line = line;
    }

    fn record_cpu_read(&self, addr: u16, value: u8) {
        if self.debug_trace_mode == CpuAccessTraceMode::All {
            self.debug_trace_events
                .borrow_mut()
                .push(BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Memory,
                    addr: u32::from(addr),
                    value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }

    fn record_cpu_write(&self, addr: u16, old_value: u8, written_value: u8, new_value: u8) {
        if self.debug_trace_mode != CpuAccessTraceMode::None {
            self.debug_trace_events
                .borrow_mut()
                .push(BusAccessEvent::Write {
                    at: None,
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

    fn record_io_read(&self, port: u8, value: u8) {
        if self.debug_trace_mode == CpuAccessTraceMode::All {
            self.debug_trace_events
                .borrow_mut()
                .push(BusAccessEvent::Read {
                    at: None,
                    space: TraceWriteKind::Io,
                    addr: u32::from(port),
                    value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }

    fn record_io_write(&self, port: u8, value: u8) {
        if self.debug_trace_mode != CpuAccessTraceMode::None {
            self.debug_trace_events
                .borrow_mut()
                .push(BusAccessEvent::Write {
                    at: None,
                    space: TraceWriteKind::Io,
                    addr: u32::from(port),
                    old_value: u32::from(value),
                    written_value: u32::from(value),
                    new_value: u32::from(value),
                    width: TraceWriteWidth::Byte,
                    mapped_addr: None,
                });
        }
    }
}

impl Z80Bus for Bus {
    fn cpu_read(&self, addr: u16) -> u8 {
        Self::cpu_read(self, addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        Self::cpu_write(self, addr, value);
    }

    fn io_read(&mut self, port: u8) -> u8 {
        Self::io_read(self, port)
    }

    fn io_write(&mut self, port: u8, value: u8) {
        Self::io_write(self, port, value);
    }

    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        if Self::is_psg_write_port(cycle.port) {
            self.pending_psg_write_cycle = Some(cycle);
        } else {
            Self::io_write(self, cycle.port, cycle.value);
        }
    }

    fn maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.nmi_pending
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.nmi_pending)
    }
}

impl Z80Bus for TracingBus<'_> {
    fn cpu_read(&self, addr: u16) -> u8 {
        let value = self.0.cpu_read(addr);
        self.0.record_cpu_read(addr, value);
        value
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        let old_value = self.0.cpu_peek(addr);
        self.0.cpu_write(addr, value);
        let new_value = self.0.cpu_peek(addr);
        self.0.record_cpu_write(addr, old_value, value, new_value);
    }

    fn io_read(&mut self, port: u8) -> u8 {
        let value = self.0.io_read(port);
        self.0.record_io_read(port, value);
        value
    }

    fn io_write(&mut self, port: u8, value: u8) {
        self.0.io_write(port, value);
        self.0.record_io_write(port, value);
    }

    fn io_write_cycle(&mut self, cycle: IoWriteCycle) {
        <Bus as Z80Bus>::io_write_cycle(self.0, cycle);
        self.0.record_io_write(cycle.port, cycle.value);
    }

    fn maskable_interrupt_pending(&self) -> bool {
        false
    }

    fn non_maskable_interrupt_pending(&self) -> bool {
        self.0.nmi_pending
    }

    fn acknowledge_non_maskable_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.0.nmi_pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::DEFAULT_SAMPLE_RATE;
    use crate::input::KeypadKey;

    fn bus() -> Bus {
        let mut bios = vec![0; BIOS_SIZE];
        bios[0x123] = 0x5A;
        let mut cartridge = vec![0; 8 * 1024];
        cartridge[0x321] = 0xA5;
        Bus::new(&bios, &cartridge, DEFAULT_SAMPLE_RATE).unwrap()
    }

    #[test]
    fn maps_bios_open_bus_mirrored_ram_and_cartridge() {
        let mut bus = bus();
        assert_eq!(bus.expansion_hardware(), ExpansionHardware::Absent);
        assert_eq!(bus.cpu_read(0x0123), 0x5A);
        assert_eq!(bus.cpu_read(EXPANSION_START), MEMORY_OPEN_BUS_VALUE);
        assert_eq!(bus.cpu_read(EXPANSION_END), MEMORY_OPEN_BUS_VALUE);
        bus.cpu_write(0x6123, 0x3C);
        assert_eq!(bus.cpu_read(0x6523), 0x3C);
        assert_eq!(bus.cpu_read(0x8321), 0xA5);
        assert_eq!(bus.cpu_read(0xA000), MEMORY_OPEN_BUS_VALUE);
    }

    #[test]
    fn unmapped_io_ranges_read_as_open_bus() {
        let mut bus = bus();

        for port in [
            0x00,
            KEYPAD_MODE_PORT_START,
            KEYPAD_MODE_PORT_END,
            JOYSTICK_MODE_PORT_START,
            JOYSTICK_MODE_PORT_END,
        ] {
            assert_eq!(bus.io_read(port), IO_OPEN_BUS_VALUE);
        }
    }

    #[test]
    fn decodes_controller_vdp_and_psg_port_ranges() {
        let mut bus = bus();
        bus.input_mut().player_mut(0).unwrap().keypad = Some(KeypadKey::One);
        assert_eq!(bus.io_read(0xE0), 0x7D);
        bus.io_write(0xC0, 0);
        bus.input_mut().player_mut(1).unwrap().right = true;
        assert_eq!(bus.io_read(0xE2), 0x7D);

        bus.io_write(0xE0, 0x9F);
        assert_eq!(bus.psg().last_write(), Some(0x9F));
        bus.io_write(0xA1, 0x00);
        bus.io_write(0xA1, 0x40);
        bus.io_write(0xA0, 0xA5);
        assert_eq!(bus.vdp().vram()[0], 0xA5);
    }
}
