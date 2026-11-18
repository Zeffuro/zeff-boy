use super::VdpDmaError;
use super::constants::DMA_WORK_RAM_START;
use crate::constants::{CARTRIDGE_END, UNPOPULATED_ROM_BYTE, WORK_RAM_MASK};
use zeff_m68k::bus::BUS_ADDRESS_MASK;

#[derive(Clone, Copy)]
pub struct VdpDmaSource<'a> {
    rom: &'a [u8],
    ram: &'a [u8],
    cartridge: Option<&'a crate::cartridge::Cartridge>,
    tmss: Option<&'a crate::hardware::tmss::Tmss>,
    svp_dram: Option<&'a crate::svp::DramReadView>,
}

impl<'a> VdpDmaSource<'a> {
    #[cfg(test)]
    pub(crate) fn assert_word(self, address: u32, expected: u16) {
        assert_eq!(self.read(address), Ok(expected));
    }
    #[cfg(test)]
    pub(crate) fn assert_download_reads(cartridge: &'a crate::cartridge::Cartridge) {
        let source = Self::from_cartridge(cartridge, &[]);
        assert_eq!(source.read(0x1234), Ok(0x5500));
        assert_eq!(source.read(0x300000), Ok(0xff66));
        assert_eq!(source.read(0x3ffffe), Ok(0x7788));
        assert_eq!(
            source.read(0x400000),
            Err(VdpDmaError::UnsupportedSource(0x400000))
        );
    }

    #[cfg(test)]
    pub(crate) fn assert_bounded_odd_reads(cart: &'a crate::cartridge::Cartridge) {
        let source = Self::from_cartridge(cart, &[]);
        let region = crate::Region::NtscUs;
        let mut vdp = crate::Vdp::bootstrap_mode5();
        for (index, value) in [
            (1, 0x14),
            (15, 2),
            (19, 1),
            (20, 0),
            (21, 0),
            (22, 0),
            (23, 0x10),
        ] {
            vdp.advance_to(
                zeff_emu_common::time::MasterTicks::new(vdp.now().get() + 1),
                region,
            )
            .unwrap();
            let write = vdp.queue_register(index, value, region).unwrap();
            vdp.advance_to(write.publish_at, region).unwrap();
        }
        vdp.write_control(0x4000, region).unwrap();
        let before = vdp.clone();
        assert_eq!(
            vdp.write_control_from(0x0080, region, source),
            Err(crate::VdpPortError::Schedule(crate::VdpScheduleError::Dma(
                VdpDmaError::UnsupportedSource(0x200000)
            )))
        );
        assert_eq!(vdp, before);
        for address in [
            0x200000, 0x2007fe, 0x200800, 0x201000, 0x2012be, 0x2012c0, 0x3ffffe,
        ] {
            assert_eq!(
                source.read(address),
                Err(VdpDmaError::UnsupportedSource(address))
            );
        }
    }
    #[cfg(test)]
    pub(crate) fn assert_realtec_reads(cart: &'a crate::cartridge::Cartridge) {
        let source = Self::from_cartridge(cart, &[]);
        assert_eq!(source.read(0x60000), Ok(0x5aa5));
        assert_eq!(
            source.read(0x100),
            Err(VdpDmaError::UnsupportedSource(0x100))
        );
    }

    #[cfg(test)]
    pub(crate) fn assert_extended_windows(cart: &'a crate::cartridge::Cartridge) {
        let source = Self::from_cartridge(cart, &[0x56, 0x78]);
        assert_eq!(source.read(0x1000), Ok(0x2525));
        assert_eq!(source.read(0x81000), Ok(0x2626));
        assert_eq!(source.read(0x101000), Ok(0x2727));
        assert_eq!(source.read(0xe00000), Ok(0x5678));
        assert_eq!(
            source.read(0x400000),
            Err(VdpDmaError::UnsupportedSource(0x400000))
        );
        let mut partial = cart.clone();
        let mut rom = cart.bytes[..0x600004].to_vec();
        rom[0x600000..].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        partial.bytes = rom.into();
        partial.extended_ssf = Some(crate::cartridge::extended_ssf::ExtendedSsf::new(0x600004));
        partial.extended_ssf.as_mut().unwrap().control(0xa130f0, 12);
        let source = VdpDmaSource::from_cartridge(&partial, &[]);
        assert_eq!(source.read(0), Ok(0xdead));
        assert_eq!(source.read(2), Ok(0xbeef));
        assert_eq!(source.read(4), Err(VdpDmaError::UnsupportedSource(4)));
    }

    #[cfg(test)]
    pub(crate) fn assert_sf004_boundaries(cartridge: &'a crate::cartridge::Cartridge) {
        let source = Self::from_cartridge(cartridge, &[0x56, 0x78]);
        assert_eq!(source.read(0xe00000), Ok(0x5678));
        assert_eq!(source.read(0xff0000), Ok(0x5678));
        assert_eq!(source.read(0x1234), Ok(0x1111));
        for address in [0x200000, 0x206000, 0x300000, 0x7ffffe, 0xa13000] {
            assert_eq!(
                source.read(address),
                Err(VdpDmaError::UnsupportedSource(address))
            );
        }
    }

    pub fn new(rom: &'a [u8], ram: &'a [u8]) -> Self {
        Self {
            rom,
            ram,
            cartridge: None,
            tmss: None,
            svp_dram: None,
        }
    }

    pub(crate) fn from_cartridge(
        cartridge: &'a crate::cartridge::Cartridge,
        ram: &'a [u8],
    ) -> Self {
        Self {
            rom: &cartridge.bytes,
            ram,
            cartridge: Some(cartridge),
            tmss: None,
            svp_dram: None,
        }
    }

    pub(crate) fn with_tmss(mut self, tmss: Option<&'a crate::hardware::tmss::Tmss>) -> Self {
        self.tmss = tmss;
        self
    }

    pub(crate) fn with_svp_dram(mut self, dram: Option<&'a crate::svp::DramReadView>) -> Self {
        self.svp_dram = dram;
        self
    }

    pub(super) fn read(self, address: u32) -> Result<u16, VdpDmaError> {
        if let Some(svp) = self.cartridge.and_then(|cartridge| cartridge.svp.as_ref()) {
            return match address {
                0x300000..=0x31fffe if address & 1 == 0 => {
                    let address = ((address - 0x300000) >> 1) as u16;
                    Ok(self
                        .svp_dram
                        .map_or_else(|| svp.peek_dram(address), |dram| dram.read(address)))
                }
                0x200000..=CARTRIDGE_END => Err(VdpDmaError::UnsupportedSource(address)),
                _ => self.read_mapped(address),
            };
        }
        self.read_mapped(address)
    }

    fn read_mapped(self, address: u32) -> Result<u16, VdpDmaError> {
        let byte = |address| {
            if let Some(value) = self.tmss.and_then(|tmss| tmss.read_boot(address)) {
                return Ok(value);
            }
            if self
                .cartridge
                .is_some_and(|cart| cart.bounded_switched_guarded(address))
            {
                return Err(VdpDmaError::UnsupportedSource(address));
            }
            if self.cartridge.is_some_and(|cart| cart.realtec.is_some()) && address & !1 == 0x100 {
                return Err(VdpDmaError::UnsupportedSource(address));
            }
            if let Some(cart) = self.cartridge
                && let Some(board) = &cart.sf004
                && crate::cartridge::sf004::Sf004::domain(address)
            {
                return board
                    .source_offset(address)
                    .and_then(|offset| cart.source_byte(offset))
                    .ok_or(VdpDmaError::UnsupportedSource(address));
            }
            if let Some(cartridge) = self.cartridge
                && cartridge.sf001.as_ref().is_some_and(|board| {
                    matches!(
                        board.decode(address),
                        crate::cartridge::sf001::Location::Sram(_)
                    )
                })
            {
                return cartridge
                    .read(address)
                    .ok_or(VdpDmaError::UnsupportedSource(address));
            }
            if address <= CARTRIDGE_END {
                if let Some(cartridge) = self.cartridge {
                    return cartridge
                        .read(address)
                        .ok_or(VdpDmaError::UnsupportedSource(address));
                }
                Ok(self
                    .rom
                    .get(address as usize)
                    .copied()
                    .unwrap_or(UNPOPULATED_ROM_BYTE))
            } else if (DMA_WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&address) {
                self.ram
                    .get(address as usize & WORK_RAM_MASK)
                    .copied()
                    .ok_or(VdpDmaError::UnsupportedSource(address))
            } else {
                Err(VdpDmaError::UnsupportedSource(address))
            }
        };
        Ok(u16::from_be_bytes([byte(address)?, byte(address + 1)?]))
    }
}

#[cfg(test)]
mod register_tests;
#[cfg(test)]
mod sf001_tests;

#[cfg(test)]
mod serial16_dma_tests;
