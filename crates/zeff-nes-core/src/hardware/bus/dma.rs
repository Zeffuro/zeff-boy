use crate::hardware::constants::PRIMARY_OAM_BYTES;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DmaController {
    oam: Option<OamTransfer>,
    dmc: Option<DmcTransfer>,
    dmc_load_delay: u8,
}

#[derive(Clone, Copy, Debug)]
struct OamTransfer {
    page: u8,
    index: u16,
    read_latch: Option<u8>,
    need_halt: bool,
}

#[derive(Clone, Copy, Debug)]
struct DmcTransfer {
    need_halt: bool,
    need_dummy: bool,
}

impl DmaController {
    pub(super) fn request_oam(&mut self, page: u8) {
        self.oam = Some(OamTransfer {
            page,
            index: 0,
            read_latch: None,
            need_halt: true,
        });
    }

    pub(super) fn request_dmc(&mut self) {
        if self.dmc.is_none() {
            self.dmc = Some(DmcTransfer {
                need_halt: true,
                need_dummy: true,
            });
        }
    }

    pub(super) fn schedule_dmc_load(&mut self, cycles: u8) {
        self.dmc_load_delay = cycles;
    }

    pub(super) fn clock_dmc_load_delay(&mut self) {
        self.dmc_load_delay = self.dmc_load_delay.saturating_sub(1);
    }

    pub(super) fn dmc_load_is_pending(&self) -> bool {
        self.dmc_load_delay > 0
    }

    pub(super) fn cancel_dmc(&mut self) {
        self.dmc = None;
        self.dmc_load_delay = 0;
    }

    pub(super) fn is_active(&self) -> bool {
        self.oam.is_some() || self.dmc.is_some()
    }

    pub(super) fn needs_halt(&self) -> bool {
        self.oam.is_some_and(|transfer| transfer.need_halt)
            || self.dmc.is_some_and(|transfer| transfer.need_halt)
    }

    pub(super) fn consume_cycle_setup(&mut self) {
        if let Some(transfer) = &mut self.oam
            && transfer.need_halt
        {
            transfer.need_halt = false;
        }
        if let Some(transfer) = &mut self.dmc {
            if transfer.need_halt {
                transfer.need_halt = false;
            } else if transfer.need_dummy {
                transfer.need_dummy = false;
            }
        }
    }

    pub(super) fn dmc_is_ready(&self) -> bool {
        self.dmc
            .is_some_and(|transfer| !transfer.need_halt && !transfer.need_dummy)
    }

    pub(super) fn take_dmc(&mut self) -> bool {
        self.dmc.take().is_some()
    }

    pub(super) fn oam_read_address(&self) -> Option<u16> {
        self.oam.and_then(|transfer| {
            transfer
                .read_latch
                .is_none()
                .then_some((u16::from(transfer.page) << 8) | transfer.index)
        })
    }

    pub(super) fn store_oam_read(&mut self, value: u8) {
        self.oam
            .as_mut()
            .expect("OAM read requires an active transfer")
            .read_latch = Some(value);
    }

    pub(super) fn take_oam_write(&mut self) -> Option<u8> {
        let transfer = self.oam.as_mut()?;
        let value = transfer.read_latch.take()?;
        transfer.index += 1;
        if transfer.index == PRIMARY_OAM_BYTES as u16 {
            self.oam = None;
        }
        Some(value)
    }

    pub(super) fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.oam.is_some());
        if let Some(transfer) = self.oam {
            w.write_u8(transfer.page);
            w.write_u16(transfer.index);
            w.write_bool(transfer.read_latch.is_some());
            w.write_u8(transfer.read_latch.unwrap_or(0));
            w.write_bool(transfer.need_halt);
        }
        w.write_bool(self.dmc.is_some());
        if let Some(transfer) = self.dmc {
            w.write_bool(transfer.need_halt);
            w.write_bool(transfer.need_dummy);
        }
        w.write_u8(self.dmc_load_delay);
    }

    pub(super) fn read_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        let oam = if r.read_bool()? {
            let page = r.read_u8()?;
            let index = r.read_u16()?;
            if index >= PRIMARY_OAM_BYTES as u16 {
                anyhow::bail!("invalid OAM DMA byte index {index}");
            }
            let has_latch = r.read_bool()?;
            let latch = r.read_u8()?;
            let need_halt = r.read_bool()?;
            if need_halt && (index != 0 || has_latch) {
                anyhow::bail!("invalid pending OAM DMA state");
            }
            Some(OamTransfer {
                page,
                index,
                read_latch: has_latch.then_some(latch),
                need_halt,
            })
        } else {
            None
        };
        let dmc = if r.read_bool()? {
            let need_halt = r.read_bool()?;
            let need_dummy = r.read_bool()?;
            if need_halt && !need_dummy {
                anyhow::bail!("invalid pending DMC DMA state");
            }
            Some(DmcTransfer {
                need_halt,
                need_dummy,
            })
        } else {
            None
        };
        let dmc_load_delay = r.read_u8()?;
        if dmc_load_delay > 4 {
            anyhow::bail!("invalid DMC DMA load delay {dmc_load_delay}");
        }
        if dmc.is_some() && dmc_load_delay != 0 {
            anyhow::bail!("invalid overlapping DMC DMA request and load delay");
        }

        self.oam = oam;
        self.dmc = dmc;
        self.dmc_load_delay = dmc_load_delay;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oam_page_write_replaces_a_pending_transfer() {
        let mut dma = DmaController::default();
        dma.request_oam(0x02);
        dma.request_oam(0x03);

        assert_eq!(dma.oam_read_address(), Some(0x0300));
    }

    #[test]
    fn dmc_requires_halt_and_dummy_before_its_get_cycle() {
        let mut dma = DmaController::default();
        dma.request_dmc();
        assert!(dma.needs_halt());
        assert!(!dma.dmc_is_ready());

        dma.consume_cycle_setup();
        assert!(!dma.needs_halt());
        assert!(!dma.dmc_is_ready());

        dma.consume_cycle_setup();
        assert!(dma.dmc_is_ready());
        assert!(dma.take_dmc());
        assert!(!dma.is_active());
    }

    #[test]
    fn dmc_load_delay_counts_down_before_a_request_can_start() {
        let mut dma = DmaController::default();
        dma.schedule_dmc_load(2);
        assert!(dma.dmc_load_is_pending());
        dma.clock_dmc_load_delay();
        assert!(dma.dmc_load_is_pending());
        dma.clock_dmc_load_delay();
        assert!(!dma.dmc_load_is_pending());
    }

    #[test]
    fn invalid_serialized_dma_states_are_rejected() {
        fn read(bytes: Vec<u8>) -> anyhow::Result<()> {
            let mut dma = DmaController::default();
            dma.read_state(&mut crate::save_state::StateReader::new(&bytes))
        }

        let mut invalid_oam = crate::save_state::StateWriter::new();
        invalid_oam.write_bool(true);
        invalid_oam.write_u8(0);
        invalid_oam.write_u16(1);
        invalid_oam.write_bool(false);
        invalid_oam.write_u8(0);
        invalid_oam.write_bool(true);
        invalid_oam.write_bool(false);
        invalid_oam.write_u8(0);
        assert!(read(invalid_oam.into_bytes()).is_err());

        let mut invalid_dmc = crate::save_state::StateWriter::new();
        invalid_dmc.write_bool(false);
        invalid_dmc.write_bool(true);
        invalid_dmc.write_bool(true);
        invalid_dmc.write_bool(false);
        invalid_dmc.write_u8(0);
        assert!(read(invalid_dmc.into_bytes()).is_err());

        let mut invalid_delay = crate::save_state::StateWriter::new();
        invalid_delay.write_bool(false);
        invalid_delay.write_bool(false);
        invalid_delay.write_u8(5);
        assert!(read(invalid_delay.into_bytes()).is_err());
    }
}
