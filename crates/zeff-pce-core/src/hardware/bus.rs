use std::error::Error;
use std::fmt::{Display, Formatter};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::cartridge::{
    POPULOUS_HUCARD_IMAGE_LEN, PceHuCard, PceHuCardBoard, SF2_CE_HUCARD_IMAGE_LEN,
    SYSTEM_CARD_V1_V2_IMAGE_LEN,
};
use super::cpu::{CpuBus, IrqPort, PHYSICAL_ADDRESS_MASK, TimerPort, VdcPort};
use super::vpc::VpcPort;

pub const HUCARD_ROM_REGION_LEN: usize = 0x10_0000;
pub const WORK_RAM_LEN: usize = 0x2000;
pub const SUPERGRAFX_WORK_RAM_LEN: usize = 0x8000;
pub const OPEN_BUS_VALUE: u8 = 0xFF;

const HUCARD_END: u32 = 0x0F_FFFF;
const WORK_RAM_START: u32 = 0x1F_0000;
const WORK_RAM_END: u32 = 0x1F_7FFF;
const VDC_START: u32 = 0x1F_E000;
const VDC_END: u32 = 0x1F_E3FF;
const VCE_START: u32 = 0x1F_E400;
const VCE_END: u32 = 0x1F_E7FF;
const PSG_START: u32 = 0x1F_E800;
const PSG_END: u32 = 0x1F_EBFF;
const TIMER_START: u32 = 0x1F_EC00;
const TIMER_END: u32 = 0x1F_EFFF;
const CONTROLLER_START: u32 = 0x1F_F000;
const CONTROLLER_END: u32 = 0x1F_F3FF;
const IRQ_START: u32 = 0x1F_F400;
const IRQ_END: u32 = 0x1F_F7FF;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceHardwareTopology {
    #[default]
    Base,
    SuperGrafx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalRegion {
    HuCard(u32),
    WorkRam(u16),
    Vdc(VdcPort),
    Vpc(VpcPort),
    Vdc2(VdcPort),
    Vce(VcePort),
    Psg(PsgPort),
    Timer(TimerPort),
    Controller,
    Irq(IrqPort),
    Unmapped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VcePort(u8);

impl VcePort {
    #[inline]
    pub const fn from_offset(offset: u8) -> Self {
        Self(offset & 7)
    }

    #[inline]
    pub const fn offset(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsgPort(u8);

impl PsgPort {
    #[inline]
    pub const fn from_offset(offset: u8) -> Self {
        Self(offset & 15)
    }

    #[inline]
    pub const fn offset(self) -> u8 {
        self.0
    }
}

#[inline]
pub const fn decode_physical_region(physical_addr: u32) -> PhysicalRegion {
    decode_physical_region_for(PceHardwareTopology::Base, physical_addr)
}

#[inline]
pub const fn decode_physical_region_for(
    topology: PceHardwareTopology,
    physical_addr: u32,
) -> PhysicalRegion {
    let physical_addr = physical_addr & PHYSICAL_ADDRESS_MASK;
    match physical_addr {
        0..=HUCARD_END => PhysicalRegion::HuCard(physical_addr),
        WORK_RAM_START..=WORK_RAM_END => PhysicalRegion::WorkRam(match topology {
            PceHardwareTopology::Base => (physical_addr as u16) & (WORK_RAM_LEN as u16 - 1),
            PceHardwareTopology::SuperGrafx => {
                (physical_addr as u16) & (SUPERGRAFX_WORK_RAM_LEN as u16 - 1)
            }
        }),
        VDC_START..=VDC_END => decode_video_region(topology, physical_addr),
        VCE_START..=VCE_END => PhysicalRegion::Vce(VcePort::from_offset(physical_addr as u8)),
        PSG_START..=PSG_END => PhysicalRegion::Psg(PsgPort::from_offset(physical_addr as u8)),
        TIMER_START..=TIMER_END => PhysicalRegion::Timer(if physical_addr & 1 == 0 {
            TimerPort::CounterReload
        } else {
            TimerPort::Control
        }),
        CONTROLLER_START..=CONTROLLER_END => PhysicalRegion::Controller,
        IRQ_START..=IRQ_END => match physical_addr & 3 {
            2 => PhysicalRegion::Irq(IrqPort::Disable),
            3 => PhysicalRegion::Irq(IrqPort::Request),
            _ => PhysicalRegion::Unmapped,
        },
        _ => PhysicalRegion::Unmapped,
    }
}

#[inline]
const fn decode_video_region(topology: PceHardwareTopology, physical_addr: u32) -> PhysicalRegion {
    if matches!(topology, PceHardwareTopology::Base) {
        return decode_vdc_port(physical_addr as u8, false);
    }
    match (physical_addr as u8) & 0x1F {
        offset @ 0x00..=0x07 => decode_vdc_port(offset, false),
        offset @ 0x08..=0x0F => PhysicalRegion::Vpc(VpcPort::from_offset(offset - 8)),
        offset @ 0x10..=0x17 => decode_vdc_port(offset, true),
        _ => PhysicalRegion::Unmapped,
    }
}

#[inline]
const fn decode_vdc_port(offset: u8, second: bool) -> PhysicalRegion {
    let port = match offset & 3 {
        0 => VdcPort::SelectOrStatus,
        1 => VdcPort::Unused,
        2 => VdcPort::DataLow,
        3 => VdcPort::DataHigh,
        _ => unreachable!(),
    };
    if second {
        PhysicalRegion::Vdc2(port)
    } else {
        PhysicalRegion::Vdc(port)
    }
}

pub trait BaseBusDevices {
    fn hardware_topology(&self) -> PceHardwareTopology {
        PceHardwareTopology::Base
    }

    fn read_vdc(&mut self, _port: VdcPort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_vdc(&mut self, _port: VdcPort, _value: u8) {}

    fn write_vdc_direct(&mut self, port: VdcPort, value: u8) {
        self.write_vdc(port, value);
    }

    fn read_vpc(&mut self, _port: VpcPort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_vpc(&mut self, _port: VpcPort, _value: u8) {}

    fn read_vdc2(&mut self, _port: VdcPort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_vdc2(&mut self, _port: VdcPort, _value: u8) {}

    fn read_vce(&mut self, _port: VcePort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_vce(&mut self, _port: VcePort, _value: u8) {}

    fn read_psg(&mut self, _port: PsgPort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_psg(&mut self, _port: PsgPort, _value: u8) {}

    fn read_timer_counter(&mut self) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_timer(&mut self, _port: TimerPort, _value: u8) {}

    fn read_controller(&mut self) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_controller(&mut self, _value: u8) {}

    fn read_irq(&mut self, _port: IrqPort) -> u8 {
        OPEN_BUS_VALUE
    }

    fn write_irq(&mut self, _port: IrqPort, _value: u8) {}

    fn observe_internal_read(&mut self, _physical_addr: u32, _value: u8, _dummy: bool) {}

    fn observe_internal_write(&mut self, _physical_addr: u32, _value: u8, _dummy: bool) {}

    fn peek_expansion(&self, _physical_addr: u32) -> Option<u8> {
        None
    }

    fn read_expansion(&mut self, _physical_addr: u32) -> Option<u8> {
        None
    }

    fn write_expansion(&mut self, _physical_addr: u32, _value: u8) -> bool {
        false
    }
}

impl BaseBusDevices for () {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseBusError {
    rom_len: usize,
    board: PceHuCardBoard,
    kind: BaseBusErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseBusErrorKind {
    InvalidHuCard,
    HardwareTopologyMismatch {
        requested: PceHardwareTopology,
        devices: PceHardwareTopology,
    },
}

impl BaseBusError {
    #[inline]
    pub const fn rom_len(self) -> usize {
        self.rom_len
    }

    #[inline]
    pub const fn board(self) -> PceHuCardBoard {
        self.board
    }

    #[inline]
    pub const fn kind(self) -> BaseBusErrorKind {
        self.kind
    }
}

impl Display for BaseBusError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        if let BaseBusErrorKind::HardwareTopologyMismatch { requested, devices } = self.kind {
            return write!(
                formatter,
                "PC Engine bus topology {requested:?} does not match devices {devices:?}"
            );
        }
        match self.board {
            PceHuCardBoard::Plain => write!(
                formatter,
                "plain HuCard ROM is {} bytes, maximum is {HUCARD_ROM_REGION_LEN}",
                self.rom_len
            ),
            PceHuCardBoard::Sf2Ce => write!(
                formatter,
                "Street Fighter II CE HuCard ROM is {} bytes, expected {SF2_CE_HUCARD_IMAGE_LEN}",
                self.rom_len
            ),
            PceHuCardBoard::Populous => write!(
                formatter,
                "Populous HuCard ROM is {} bytes, expected {POPULOUS_HUCARD_IMAGE_LEN}",
                self.rom_len
            ),
            PceHuCardBoard::SystemCardV1V2 => write!(
                formatter,
                "System Card v1/v2 ROM is {} bytes, expected {SYSTEM_CARD_V1_V2_IMAGE_LEN}",
                self.rom_len
            ),
            PceHuCardBoard::SystemCardV3 => write!(
                formatter,
                "invalid Super System Card image length {} (expected {})",
                self.rom_len, SYSTEM_CARD_V1_V2_IMAGE_LEN
            ),
        }
    }
}

impl Error for BaseBusError {}

#[derive(Debug)]
pub struct BaseBus<D> {
    hucard: PceHuCard,
    work_ram: WorkRam,
    topology: PceHardwareTopology,
    devices: D,
}

#[derive(Debug)]
enum WorkRam {
    Base(Box<[u8; WORK_RAM_LEN]>),
    SuperGrafx(Box<[u8; SUPERGRAFX_WORK_RAM_LEN]>),
}

impl WorkRam {
    fn new(topology: PceHardwareTopology) -> Self {
        match topology {
            PceHardwareTopology::Base => Self::Base(Box::new([0; WORK_RAM_LEN])),
            PceHardwareTopology::SuperGrafx => {
                Self::SuperGrafx(Box::new([0; SUPERGRAFX_WORK_RAM_LEN]))
            }
        }
    }

    fn mapped(&self) -> &[u8] {
        match self {
            Self::Base(ram) => &ram[..],
            Self::SuperGrafx(ram) => &ram[..],
        }
    }

    fn mapped_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Base(ram) => &mut ram[..],
            Self::SuperGrafx(ram) => &mut ram[..],
        }
    }

    fn base(&self) -> &[u8; WORK_RAM_LEN] {
        match self {
            Self::Base(ram) => ram,
            Self::SuperGrafx(ram) => (&ram[..WORK_RAM_LEN]).try_into().unwrap(),
        }
    }

    fn base_mut(&mut self) -> &mut [u8; WORK_RAM_LEN] {
        match self {
            Self::Base(ram) => ram,
            Self::SuperGrafx(ram) => (&mut ram[..WORK_RAM_LEN]).try_into().unwrap(),
        }
    }
}

impl<D> BaseBus<D> {
    pub fn new(hucard_rom: Vec<u8>, devices: D) -> Result<Self, BaseBusError>
    where
        D: BaseBusDevices,
    {
        Self::with_hucard(hucard_rom, PceHuCardBoard::Plain, devices)
    }

    pub fn with_hucard(
        hucard_rom: Vec<u8>,
        board: PceHuCardBoard,
        devices: D,
    ) -> Result<Self, BaseBusError>
    where
        D: BaseBusDevices,
    {
        Self::with_hucard_and_topology(hucard_rom, board, PceHardwareTopology::Base, devices)
    }

    pub fn with_hucard_and_topology(
        hucard_rom: Vec<u8>,
        board: PceHuCardBoard,
        topology: PceHardwareTopology,
        devices: D,
    ) -> Result<Self, BaseBusError>
    where
        D: BaseBusDevices,
    {
        let device_topology = devices.hardware_topology();
        if topology != device_topology {
            return Err(BaseBusError {
                rom_len: hucard_rom.len(),
                board,
                kind: BaseBusErrorKind::HardwareTopologyMismatch {
                    requested: topology,
                    devices: device_topology,
                },
            });
        }
        let valid = match board {
            PceHuCardBoard::Plain => hucard_rom.len() <= HUCARD_ROM_REGION_LEN,
            PceHuCardBoard::Sf2Ce => hucard_rom.len() == SF2_CE_HUCARD_IMAGE_LEN,
            PceHuCardBoard::Populous => hucard_rom.len() == POPULOUS_HUCARD_IMAGE_LEN,
            PceHuCardBoard::SystemCardV1V2 => hucard_rom.len() == SYSTEM_CARD_V1_V2_IMAGE_LEN,
            PceHuCardBoard::SystemCardV3 => hucard_rom.len() == SYSTEM_CARD_V1_V2_IMAGE_LEN,
        };
        if !valid {
            return Err(BaseBusError {
                rom_len: hucard_rom.len(),
                board,
                kind: BaseBusErrorKind::InvalidHuCard,
            });
        }
        Ok(Self {
            hucard: PceHuCard::new(hucard_rom, board),
            work_ram: WorkRam::new(topology),
            topology,
            devices,
        })
    }

    #[inline]
    pub fn hucard_rom(&self) -> &[u8] {
        self.hucard.image()
    }

    #[inline]
    pub fn hucard_board(&self) -> PceHuCardBoard {
        self.hucard.board()
    }

    #[inline]
    pub fn hucard_rom_offset(&self, physical_addr: u32) -> Option<u32> {
        match self.decode_physical_region(physical_addr) {
            PhysicalRegion::HuCard(offset) => self.hucard.rom_offset(offset),
            _ => None,
        }
    }

    #[inline]
    pub fn hucard_mapping_token(&self) -> u8 {
        self.hucard.mapping_token()
    }

    #[inline]
    pub fn hucard_ram(&self) -> Option<&[u8; super::cartridge::POPULOUS_HUCARD_RAM_LEN]> {
        self.hucard.ram()
    }

    #[cfg(test)]
    pub(crate) fn system_card_ram_mut(
        &mut self,
    ) -> Option<&mut [u8; super::cartridge::SUPER_SYSTEM_CARD_RAM_LEN]> {
        self.hucard.system_card_ram_mut()
    }

    #[inline]
    pub fn reset_hucard(&mut self) {
        self.hucard.reset();
    }

    #[inline]
    pub fn work_ram(&self) -> &[u8; WORK_RAM_LEN] {
        self.work_ram.base()
    }

    #[inline]
    pub fn work_ram_mut(&mut self) -> &mut [u8; WORK_RAM_LEN] {
        self.work_ram.base_mut()
    }

    #[inline]
    pub fn mapped_work_ram(&self) -> &[u8] {
        self.work_ram.mapped()
    }

    #[inline]
    pub fn mapped_work_ram_mut(&mut self) -> &mut [u8] {
        self.work_ram.mapped_mut()
    }

    #[inline]
    pub const fn topology(&self) -> PceHardwareTopology {
        self.topology
    }

    #[inline]
    pub const fn decode_physical_region(&self, physical_addr: u32) -> PhysicalRegion {
        decode_physical_region_for(self.topology, physical_addr)
    }

    #[inline]
    pub fn devices(&self) -> &D {
        &self.devices
    }

    #[inline]
    pub fn devices_mut(&mut self) -> &mut D {
        &mut self.devices
    }

    #[inline]
    pub fn into_devices(self) -> D {
        self.devices
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(self.work_ram.mapped());
        self.hucard.write_state(writer);
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        reader.read_exact(self.work_ram.mapped_mut())?;
        self.hucard.read_state(reader)
    }
}

impl<D: BaseBusDevices> BaseBus<D> {
    pub fn peek(&self, physical_addr: u32) -> u8 {
        match self.decode_physical_region(physical_addr) {
            PhysicalRegion::HuCard(offset) => self
                .devices
                .peek_expansion(physical_addr & PHYSICAL_ADDRESS_MASK)
                .unwrap_or_else(|| self.hucard.read(offset)),
            PhysicalRegion::WorkRam(offset) => self.work_ram.mapped()[usize::from(offset)],
            PhysicalRegion::Vdc(_)
            | PhysicalRegion::Vpc(_)
            | PhysicalRegion::Vdc2(_)
            | PhysicalRegion::Vce(_)
            | PhysicalRegion::Psg(_)
            | PhysicalRegion::Timer(_)
            | PhysicalRegion::Controller
            | PhysicalRegion::Irq(_) => OPEN_BUS_VALUE,
            PhysicalRegion::Unmapped => self
                .devices
                .peek_expansion(physical_addr & PHYSICAL_ADDRESS_MASK)
                .unwrap_or(OPEN_BUS_VALUE),
        }
    }

    pub fn read(&mut self, physical_addr: u32) -> u8 {
        match self.decode_physical_region(physical_addr) {
            PhysicalRegion::HuCard(offset) => self
                .devices
                .read_expansion(physical_addr & PHYSICAL_ADDRESS_MASK)
                .unwrap_or_else(|| self.hucard.read(offset)),
            PhysicalRegion::WorkRam(offset) => self.work_ram.mapped()[usize::from(offset)],
            PhysicalRegion::Vdc(offset) => self.devices.read_vdc(offset),
            PhysicalRegion::Vpc(offset) => self.devices.read_vpc(offset),
            PhysicalRegion::Vdc2(offset) => self.devices.read_vdc2(offset),
            PhysicalRegion::Vce(offset) => self.devices.read_vce(offset),
            PhysicalRegion::Psg(offset) => self.devices.read_psg(offset),
            PhysicalRegion::Timer(TimerPort::CounterReload) => self.devices.read_timer_counter(),
            PhysicalRegion::Timer(TimerPort::Control) => OPEN_BUS_VALUE,
            PhysicalRegion::Controller => self.devices.read_controller(),
            PhysicalRegion::Irq(offset) => self.devices.read_irq(offset),
            PhysicalRegion::Unmapped => self
                .devices
                .read_expansion(physical_addr & PHYSICAL_ADDRESS_MASK)
                .unwrap_or(OPEN_BUS_VALUE),
        }
    }

    pub fn write(&mut self, physical_addr: u32, value: u8) {
        match self.decode_physical_region(physical_addr) {
            PhysicalRegion::WorkRam(offset) => {
                self.work_ram.mapped_mut()[usize::from(offset)] = value;
            }
            PhysicalRegion::Vdc(offset) => self.devices.write_vdc(offset, value),
            PhysicalRegion::Vpc(offset) => self.devices.write_vpc(offset, value),
            PhysicalRegion::Vdc2(offset) => self.devices.write_vdc2(offset, value),
            PhysicalRegion::Vce(offset) => self.devices.write_vce(offset, value),
            PhysicalRegion::Psg(offset) => self.devices.write_psg(offset, value),
            PhysicalRegion::Timer(offset) => self.devices.write_timer(offset, value),
            PhysicalRegion::Controller => self.devices.write_controller(value),
            PhysicalRegion::Irq(offset) => self.devices.write_irq(offset, value),
            PhysicalRegion::HuCard(offset) => {
                if !self
                    .devices
                    .write_expansion(physical_addr & PHYSICAL_ADDRESS_MASK, value)
                {
                    self.hucard.write(offset, value);
                }
            }
            PhysicalRegion::Unmapped => {
                self.devices
                    .write_expansion(physical_addr & PHYSICAL_ADDRESS_MASK, value);
            }
        }
    }
}

impl<D: BaseBusDevices> CpuBus for BaseBus<D> {
    #[inline]
    fn read(&mut self, physical_addr: u32) -> u8 {
        BaseBus::read(self, physical_addr)
    }

    #[inline]
    fn write(&mut self, physical_addr: u32, value: u8) {
        BaseBus::write(self, physical_addr, value);
    }

    #[inline]
    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        self.devices.write_vdc_direct(port, value);
    }

    #[inline]
    fn observe_internal_read(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.devices
            .observe_internal_read(physical_addr, value, dummy);
    }

    #[inline]
    fn observe_internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.devices
            .observe_internal_write(physical_addr, value, dummy);
    }
}
