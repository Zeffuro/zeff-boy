use super::Mapper;
use super::header::{ChrFetchKind, Mirroring};
use super::mappers;

#[allow(clippy::large_enum_variant)]
pub enum MapperImpl {
    Nrom(mappers::Nrom),
    Mmc1(mappers::Mmc1),
    Uxrom(mappers::Uxrom),
    Cnrom(mappers::Cnrom),
    Mmc3(mappers::Mmc3),
    Mmc5(mappers::Mmc5),
    Axrom(mappers::Axrom),
    BandaiFcg16(mappers::BandaiFcg16),
    Vrc4(mappers::Vrc4),
    Vrc6(mappers::Vrc6),
    Fme7(mappers::Fme7),
    Action52(mappers::Action52),
    Namco163(mappers::Namco163),
    Vrc7(mappers::Vrc7),
    Fds(mappers::Fds),
}

macro_rules! dispatch_mapper {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            MapperImpl::Nrom(m) => m.$method($($arg),*),
            MapperImpl::Mmc1(m) => m.$method($($arg),*),
            MapperImpl::Uxrom(m) => m.$method($($arg),*),
            MapperImpl::Cnrom(m) => m.$method($($arg),*),
            MapperImpl::Mmc3(m) => m.$method($($arg),*),
            MapperImpl::Mmc5(m) => m.$method($($arg),*),
            MapperImpl::Axrom(m) => m.$method($($arg),*),
            MapperImpl::BandaiFcg16(m) => m.$method($($arg),*),
            MapperImpl::Vrc4(m) => m.$method($($arg),*),
            MapperImpl::Vrc6(m) => m.$method($($arg),*),
            MapperImpl::Fme7(m) => m.$method($($arg),*),
            MapperImpl::Action52(m) => m.$method($($arg),*),
            MapperImpl::Namco163(m) => m.$method($($arg),*),
            MapperImpl::Vrc7(m) => m.$method($($arg),*),
            MapperImpl::Fds(m) => m.$method($($arg),*),
        }
    };
}

impl MapperImpl {
    #[inline]
    pub(super) fn cpu_peek(&self, addr: u16) -> u8 {
        dispatch_mapper!(self, cpu_peek, addr)
    }

    #[inline]
    pub(super) fn cpu_read(&mut self, addr: u16) -> u8 {
        dispatch_mapper!(self, cpu_read, addr)
    }

    #[inline]
    pub(super) fn cpu_write(&mut self, addr: u16, val: u8) {
        dispatch_mapper!(self, cpu_write, addr, val)
    }

    #[inline]
    pub(super) fn chr_read(&mut self, addr: u16) -> u8 {
        dispatch_mapper!(self, chr_read, addr)
    }

    #[inline]
    pub(super) fn chr_read_kind(&mut self, addr: u16, kind: ChrFetchKind) -> u8 {
        dispatch_mapper!(self, chr_read_kind, addr, kind)
    }

    #[inline]
    pub(super) fn chr_write(&mut self, addr: u16, val: u8) {
        dispatch_mapper!(self, chr_write, addr, val)
    }

    pub(super) fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
        dispatch_mapper!(self, ppu_nametable_read, addr, ciram)
    }

    pub(super) fn ppu_nametable_write(
        &mut self,
        addr: u16,
        val: u8,
        ciram: &mut [u8; 0x800],
    ) -> bool {
        dispatch_mapper!(self, ppu_nametable_write, addr, val, ciram)
    }

    pub(super) fn mirroring(&self) -> Mirroring {
        dispatch_mapper!(self, mirroring)
    }

    pub(super) fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        dispatch_mapper!(self, write_state, w)
    }

    pub(super) fn read_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        dispatch_mapper!(self, read_state, r)
    }

    pub(super) fn irq_pending(&self) -> bool {
        dispatch_mapper!(self, irq_pending)
    }

    pub(super) fn notify_scanline(&mut self) {
        dispatch_mapper!(self, notify_scanline)
    }

    pub(super) fn clock_cpu(&mut self) {
        dispatch_mapper!(self, clock_cpu)
    }

    pub(super) fn audio_output(&self) -> f32 {
        dispatch_mapper!(self, audio_output)
    }

    pub(super) fn dump_battery_data(&self) -> Option<Vec<u8>> {
        dispatch_mapper!(self, dump_battery_data)
    }

    pub(super) fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        dispatch_mapper!(self, load_battery_data, bytes)
    }
}
