use super::*;
use crate::hardware::bus::{PhysicalRegion, PlainMemoryTarget, decode_physical_region};

pub(super) struct PlainOnChipBus<'a, B> {
    inner: OnChipBus<'a, B>,
}

impl<'a, B> PlainOnChipBus<'a, B> {
    #[inline]
    pub(super) fn new(on_chip_io: &'a mut OnChipIo, inner: &'a mut B) -> Self {
        Self {
            inner: OnChipBus::new(on_chip_io, inner),
        }
    }
}

impl<B: PlainMemoryCpuBus> PlainOnChipBus<'_, B> {
    #[inline]
    fn route(&self, physical_addr: u32) -> (PhysicalRegion, Option<PlainMemoryTarget>) {
        let region = decode_physical_region(physical_addr);
        let target = match region {
            PhysicalRegion::Timer(_) | PhysicalRegion::Irq(_) => None,
            _ => self.inner.inner.plain_memory_target_for_region(region),
        };
        (region, target)
    }

    fn read_access(&mut self, physical_addr: u32, dummy: bool) -> u8 {
        let (region, target) = self.route(physical_addr);
        match region {
            PhysicalRegion::Timer(_) | PhysicalRegion::Irq(_) => {
                self.inner
                    .internal_read_region(physical_addr, region, dummy)
            }
            _ => match target {
                Some(target) => {
                    let value = self
                        .inner
                        .inner
                        .read_plain_memory(physical_addr, target, dummy);
                    self.inner.advance_elapsed_time();
                    value
                }
                None => {
                    self.inner.inner.record_plain_memory_fallback();
                    self.inner
                        .read_non_internal_region(physical_addr, region, dummy)
                }
            },
        }
    }

    fn write_access(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        let (region, target) = self.route(physical_addr);
        match region {
            PhysicalRegion::Timer(_) | PhysicalRegion::Irq(_) => {
                self.inner
                    .internal_write_region(physical_addr, region, value, dummy);
            }
            _ => match target {
                Some(target @ PlainMemoryTarget::WorkRam(_)) => {
                    self.inner
                        .inner
                        .write_plain_memory(physical_addr, target, value, dummy);
                    self.inner.advance_elapsed_time();
                }
                Some(PlainMemoryTarget::HuCard(_)) | None => {
                    self.inner.inner.record_plain_memory_fallback();
                    self.inner
                        .write_non_internal_region(physical_addr, region, value, dummy);
                }
            },
        }
    }
}

impl<B: PlainMemoryCpuBus> CpuBus for PlainOnChipBus<'_, B> {
    #[inline]
    fn read(&mut self, physical_addr: u32) -> u8 {
        self.read_access(physical_addr, false)
    }

    #[inline]
    fn write(&mut self, physical_addr: u32, value: u8) {
        self.write_access(physical_addr, value, false);
    }

    #[inline]
    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        self.read_access(physical_addr, true)
    }

    #[inline]
    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        self.write_access(physical_addr, value, true);
    }

    #[inline]
    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        self.inner.write_vdc(port, value);
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
    }
}
