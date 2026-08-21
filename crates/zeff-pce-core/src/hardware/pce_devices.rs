use super::bus::{BaseBusDevices, PceHardwareTopology, PsgPort, VcePort};
use super::cartridge::PceConsoleWiring;
use super::cd_media::CdDisc;
use super::cdrom2::CdRom2;
use super::controller::{ControllerDevice, ControllerPort};
use super::cpu::{LineLevel, VdcPort};
use super::psg::{HuC6280Psg, PsgRevision};
use super::vce::HuC6260;
use super::vdc::{HuC6270, VdcDmaError};
use super::vdc_horizontal::VdcHorizontalAdvance;
use super::vdc_scanline::{VdcExternalVceScanline, VdcScanlineAdvanceError, VdcScanlineBoundary};
use super::vpc::{HuC6202, VpcPort, VpcVdc};

pub const BASE_PCE_NO_CD_CONTROLLER_UPPER_BITS: u8 =
    PceConsoleWiring::PcEngine.controller_upper_bits();
pub const BASE_TURBOGRAFX16_NO_CD_CONTROLLER_UPPER_BITS: u8 =
    PceConsoleWiring::TurboGrafx16.controller_upper_bits();
pub const BASE_PCE_CDROM2_CONTROLLER_UPPER_BITS: u8 = 0x70;
pub const BASE_TURBOGRAFX16_CDROM2_CONTROLLER_UPPER_BITS: u8 = 0x30;

#[derive(Debug)]
pub struct PceDevices {
    vdc: HuC6270,
    supergrafx: Option<SuperGrafxVideo>,
    vce: HuC6260,
    psg: HuC6280Psg,
    controller: ControllerPort,
    console_wiring: PceConsoleWiring,
    cdrom2: Option<CdRom2>,
}

#[derive(Debug)]
pub struct SuperGrafxVideo {
    vpc: HuC6202,
    vdc2: HuC6270,
}

impl SuperGrafxVideo {
    #[inline]
    pub const fn vpc(&self) -> &HuC6202 {
        &self.vpc
    }

    #[inline]
    pub fn vpc_mut(&mut self) -> &mut HuC6202 {
        &mut self.vpc
    }

    #[inline]
    pub const fn vdc2(&self) -> &HuC6270 {
        &self.vdc2
    }

    #[inline]
    pub fn vdc2_mut(&mut self) -> &mut HuC6270 {
        &mut self.vdc2
    }
}

impl Default for PceDevices {
    fn default() -> Self {
        Self::new(ControllerPort::default())
    }
}

impl PceDevices {
    pub fn new(controller: ControllerPort) -> Self {
        Self::with_console_wiring(controller, PceConsoleWiring::PcEngine)
    }

    pub fn with_console_wiring(
        controller: ControllerPort,
        console_wiring: PceConsoleWiring,
    ) -> Self {
        Self::with_console_wiring_and_psg_revision(controller, console_wiring, PsgRevision::HuC6280)
    }

    pub fn with_console_wiring_and_psg_revision(
        controller: ControllerPort,
        console_wiring: PceConsoleWiring,
        psg_revision: PsgRevision,
    ) -> Self {
        Self::with_topology_console_wiring_and_psg_revision(
            PceHardwareTopology::Base,
            controller,
            console_wiring,
            psg_revision,
        )
    }

    pub fn with_topology_console_wiring_and_psg_revision(
        topology: PceHardwareTopology,
        controller: ControllerPort,
        console_wiring: PceConsoleWiring,
        psg_revision: PsgRevision,
    ) -> Self {
        Self {
            vdc: HuC6270::new(),
            supergrafx: match topology {
                PceHardwareTopology::Base => None,
                PceHardwareTopology::SuperGrafx => Some(SuperGrafxVideo {
                    vpc: HuC6202::new(),
                    vdc2: HuC6270::new(),
                }),
            },
            vce: HuC6260::new(),
            psg: HuC6280Psg::with_revision(psg_revision),
            controller,
            console_wiring,
            cdrom2: None,
        }
    }

    pub fn with_cdrom2(
        controller: ControllerPort,
        console_wiring: PceConsoleWiring,
        disc: CdDisc,
    ) -> Self {
        Self::with_cdrom2_system_card(controller, console_wiring, disc, false)
    }

    pub(crate) fn with_cdrom2_system_card(
        controller: ControllerPort,
        console_wiring: PceConsoleWiring,
        disc: CdDisc,
        super_system_card: bool,
    ) -> Self {
        let mut devices = Self::with_console_wiring(controller, console_wiring);
        devices.cdrom2 = Some(CdRom2::with_super_system_card(disc, super_system_card));
        devices
    }

    pub fn reset(&mut self) {
        self.vdc.reset();
        if let Some(supergrafx) = &mut self.supergrafx {
            supergrafx.vpc.reset();
            supergrafx.vdc2.reset();
        }
        self.vce.reset();
        self.psg.reset();
        self.controller.reset();
        if let Some(cdrom2) = &mut self.cdrom2 {
            cdrom2.reset();
        }
    }

    pub fn advance_master_ticks(&mut self, master_ticks: u64) {
        self.psg.advance_master_ticks(master_ticks);
        self.controller.advance_master_ticks(master_ticks);
        if let Some(cdrom2) = &mut self.cdrom2 {
            cdrom2.advance_master_ticks(master_ticks);
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.psg.set_sample_rate(sample_rate);
        if let Some(cdrom2) = &mut self.cdrom2 {
            cdrom2.set_sample_rate(sample_rate);
        }
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.psg.set_sample_generation_enabled(enabled);
        if let Some(cdrom2) = &mut self.cdrom2 {
            cdrom2.set_sample_generation_enabled(enabled);
        }
    }

    pub fn set_channel_mutes(&mut self, mutes: &[bool]) {
        self.psg.set_channel_mutes(mutes);
    }

    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        let start = output.len();
        self.psg.drain_audio_samples_into(output);
        if let Some(cdrom2) = &mut self.cdrom2 {
            cdrom2.mix_audio_samples_into(&mut output[start..]);
        }
    }

    #[inline]
    pub const fn vdc(&self) -> &HuC6270 {
        &self.vdc
    }

    #[inline]
    pub fn vdc_mut(&mut self) -> &mut HuC6270 {
        &mut self.vdc
    }

    #[inline]
    pub const fn topology(&self) -> PceHardwareTopology {
        if self.supergrafx.is_some() {
            PceHardwareTopology::SuperGrafx
        } else {
            PceHardwareTopology::Base
        }
    }

    #[inline]
    pub const fn supergrafx_video(&self) -> Option<&SuperGrafxVideo> {
        self.supergrafx.as_ref()
    }

    #[inline]
    pub fn supergrafx_video_mut(&mut self) -> Option<&mut SuperGrafxVideo> {
        self.supergrafx.as_mut()
    }

    #[inline]
    pub const fn direct_vdc_target(&self) -> VpcVdc {
        match &self.supergrafx {
            Some(supergrafx) => supergrafx.vpc.direct_vdc_target(),
            None => VpcVdc::One,
        }
    }

    #[inline]
    pub const fn vdc_for(&self, target: VpcVdc) -> Option<&HuC6270> {
        match target {
            VpcVdc::One => Some(&self.vdc),
            VpcVdc::Two => match &self.supergrafx {
                Some(supergrafx) => Some(&supergrafx.vdc2),
                None => None,
            },
        }
    }

    #[inline]
    pub fn vdc_for_mut(&mut self, target: VpcVdc) -> Option<&mut HuC6270> {
        match target {
            VpcVdc::One => Some(&mut self.vdc),
            VpcVdc::Two => match &mut self.supergrafx {
                Some(supergrafx) => Some(&mut supergrafx.vdc2),
                None => None,
            },
        }
    }

    pub fn vdc_irq_level(&self) -> LineLevel {
        if self.vdc.irq_level() == LineLevel::Low
            || self
                .supergrafx
                .as_ref()
                .is_some_and(|video| video.vdc2.irq_level() == LineLevel::Low)
        {
            LineLevel::Low
        } else {
            LineLevel::High
        }
    }

    pub(crate) fn advance_horizontal_pixels(
        &mut self,
        pixel_clocks: u64,
    ) -> Result<(VdcHorizontalAdvance, Option<VdcHorizontalAdvance>), VdcDmaError> {
        let first = self.vdc.advance_horizontal_pixels(pixel_clocks)?;
        let second = self
            .supergrafx
            .as_mut()
            .map(|video| video.vdc2.advance_horizontal_pixels(pixel_clocks))
            .transpose()?;
        Ok((first, second))
    }

    pub(crate) fn begin_external_horizontal_line(&mut self) {
        self.vdc.begin_external_horizontal_line();
        if let Some(supergrafx) = &mut self.supergrafx {
            supergrafx.vdc2.begin_external_horizontal_line();
        }
    }

    pub(crate) fn advance_machine_vce_scanline(
        &mut self,
        input: VdcExternalVceScanline,
    ) -> Result<(VdcScanlineBoundary, Option<VdcScanlineBoundary>), VdcScanlineAdvanceError> {
        self.vdc.validate_machine_vce_scanline(input)?;
        if let Some(supergrafx) = &self.supergrafx {
            supergrafx.vdc2.validate_machine_vce_scanline(input)?;
        }
        let first = self.vdc.advance_machine_vce_scanline(input)?;
        let second = self
            .supergrafx
            .as_mut()
            .map(|video| video.vdc2.advance_machine_vce_scanline(input))
            .transpose()?;
        Ok((first, second))
    }

    #[inline]
    pub const fn vce(&self) -> &HuC6260 {
        &self.vce
    }

    #[inline]
    pub fn vce_mut(&mut self) -> &mut HuC6260 {
        &mut self.vce
    }

    #[inline]
    pub const fn psg(&self) -> &HuC6280Psg {
        &self.psg
    }

    #[inline]
    pub fn psg_mut(&mut self) -> &mut HuC6280Psg {
        &mut self.psg
    }

    #[inline]
    pub const fn controller(&self) -> &ControllerPort {
        &self.controller
    }

    #[inline]
    pub fn controller_mut(&mut self) -> &mut ControllerPort {
        &mut self.controller
    }

    #[inline]
    pub fn set_controller_device(&mut self, device: ControllerDevice) {
        self.controller.set_device(device);
    }

    #[inline]
    pub const fn console_wiring(&self) -> PceConsoleWiring {
        self.console_wiring
    }

    #[inline]
    pub const fn cdrom2(&self) -> Option<&CdRom2> {
        self.cdrom2.as_ref()
    }

    #[inline]
    pub fn cdrom2_mut(&mut self) -> Option<&mut CdRom2> {
        self.cdrom2.as_mut()
    }

    #[inline]
    pub fn cdrom2_irq_level(&self) -> LineLevel {
        self.cdrom2
            .as_ref()
            .map_or(LineLevel::High, CdRom2::irq2_level)
    }

    #[inline]
    pub(crate) fn video_devices_mut(&mut self) -> (&mut HuC6270, &HuC6260) {
        (&mut self.vdc, &self.vce)
    }

    pub(crate) fn supergrafx_video_devices_mut(
        &mut self,
    ) -> Option<(&mut HuC6270, &mut HuC6270, &HuC6202, &HuC6260)> {
        let supergrafx = self.supergrafx.as_mut()?;
        Some((
            &mut self.vdc,
            &mut supergrafx.vdc2,
            &supergrafx.vpc,
            &self.vce,
        ))
    }
}

impl BaseBusDevices for PceDevices {
    #[inline]
    fn hardware_topology(&self) -> PceHardwareTopology {
        self.topology()
    }

    #[inline]
    fn read_vdc(&mut self, port: VdcPort) -> u8 {
        self.vdc.read_port(port)
    }

    #[inline]
    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        let _ = self.vdc.write_port(port, value);
    }

    #[inline]
    fn write_vdc_direct(&mut self, port: VdcPort, value: u8) {
        let target = self.direct_vdc_target();
        if let Some(vdc) = self.vdc_for_mut(target) {
            let _ = vdc.write_port(port, value);
        }
    }

    #[inline]
    fn read_vpc(&mut self, port: VpcPort) -> u8 {
        self.supergrafx
            .as_ref()
            .map_or(super::bus::OPEN_BUS_VALUE, |video| {
                video.vpc.read_port(port)
            })
    }

    #[inline]
    fn write_vpc(&mut self, port: VpcPort, value: u8) {
        if let Some(supergrafx) = &mut self.supergrafx {
            supergrafx.vpc.write_port(port, value);
        }
    }

    #[inline]
    fn read_vdc2(&mut self, port: VdcPort) -> u8 {
        self.supergrafx
            .as_mut()
            .map_or(super::bus::OPEN_BUS_VALUE, |video| {
                video.vdc2.read_port(port)
            })
    }

    #[inline]
    fn write_vdc2(&mut self, port: VdcPort, value: u8) {
        if let Some(supergrafx) = &mut self.supergrafx {
            let _ = supergrafx.vdc2.write_port(port, value);
        }
    }

    #[inline]
    fn read_vce(&mut self, port: VcePort) -> u8 {
        self.vce.read_port(port)
    }

    #[inline]
    fn write_vce(&mut self, port: VcePort, value: u8) {
        self.vce.write_port(port, value);
    }

    #[inline]
    fn read_psg(&mut self, port: PsgPort) -> u8 {
        self.psg.read_port(port)
    }

    #[inline]
    fn write_psg(&mut self, port: PsgPort, value: u8) {
        self.psg.write_port(port, value);
    }

    #[inline]
    fn read_controller(&mut self) -> u8 {
        let upper = if self.cdrom2.is_some() {
            match self.console_wiring {
                PceConsoleWiring::PcEngine => BASE_PCE_CDROM2_CONTROLLER_UPPER_BITS,
                PceConsoleWiring::TurboGrafx16 => BASE_TURBOGRAFX16_CDROM2_CONTROLLER_UPPER_BITS,
            }
        } else {
            self.console_wiring.controller_upper_bits()
        };
        upper | self.controller.read_nibble()
    }

    #[inline]
    fn write_controller(&mut self, value: u8) {
        self.controller.write_lines(value & 1 != 0, value & 2 != 0);
    }

    fn peek_expansion(&self, physical_addr: u32) -> Option<u8> {
        self.cdrom2.as_ref()?.peek_physical(physical_addr)
    }

    fn read_expansion(&mut self, physical_addr: u32) -> Option<u8> {
        self.cdrom2.as_mut()?.read_physical(physical_addr)
    }

    fn write_expansion(&mut self, physical_addr: u32, value: u8) -> bool {
        self.cdrom2
            .as_mut()
            .is_some_and(|cdrom2| cdrom2.write_physical(physical_addr, value))
    }
}
