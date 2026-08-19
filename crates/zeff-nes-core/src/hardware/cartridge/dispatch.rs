use super::Mapper;
use super::header::{ChrFetchKind, Mirroring, RomHeader};
use super::mappers;

#[allow(clippy::large_enum_variant)]
pub enum MapperImpl {
    Nrom(mappers::Nrom),
    Mmc1(mappers::Mmc1),
    Uxrom(mappers::Uxrom),
    Cnrom(mappers::Cnrom),
    Cnrom185(mappers::Cnrom185),
    ColorDreams(mappers::ColorDreams),
    Contra100In1(mappers::Contra100In1),
    Cprom(mappers::Cprom),
    Mmc2(mappers::Mmc2),
    Mmc3(mappers::Mmc3),
    Mmc5(mappers::Mmc5),
    Axrom(mappers::Axrom),
    SuperMagicCard(mappers::SuperMagicCard),
    FfeMapper8(mappers::FfeMapper8),
    IremG101(mappers::IremG101),
    IremH3001(mappers::IremH3001),
    TaitoTc0190(mappers::TaitoTc0190),
    JalecoJf17(mappers::JalecoJf17),
    JalecoSs8806(mappers::JalecoSs8806),
    TaitoX1005(mappers::TaitoX1005),
    TaitoX1017(mappers::TaitoX1017),
    ConyYoko(mappers::ConyYoko),
    Bnrom(mappers::Bnrom),
    Nina001(mappers::Nina001),
    Gxrom(mappers::Gxrom),
    Rambo1(mappers::Rambo1),
    Sunsoft3(mappers::Sunsoft3),
    Sunsoft4(mappers::Sunsoft4),
    Bandai74161(mappers::Bandai74161),
    Camerica(mappers::Camerica),
    Nina03(mappers::Nina03),
    Vrc1(mappers::Vrc1),
    Mapper112(mappers::Mapper112),
    Vrc3(mappers::Vrc3),
    Mapper91(mappers::Mapper91),
    NapoleonSenki(mappers::NapoleonSenki),
    HolyDiver(mappers::HolyDiver),
    JalecoJf11(mappers::JalecoJf11),
    JalecoJf13(mappers::JalecoJf13),
    BandaiFcg16(mappers::BandaiFcg16),
    Vrc4(mappers::Vrc4),
    Vrc6(mappers::Vrc6),
    Fme7(mappers::Fme7),
    Action52(mappers::Action52),
    Quattro(mappers::Quattro),
    Namco163(mappers::Namco163),
    Vrc7(mappers::Vrc7),
    J87(mappers::J87),
    Sunsoft2Mapper89(mappers::Sunsoft2Mapper89),
    JyAsic(mappers::JyAsic),
    SenjouNoOokami(mappers::SenjouNoOokami),
    IremTamS1(mappers::IremTamS1),
    VsSystem(mappers::VsSystem),
    Namco108(mappers::Namco108),
    Sunsoft1(mappers::Sunsoft1),
    KaraokeStudio(mappers::KaraokeStudio),
    Mapper240(mappers::Mapper240),
    G0151(mappers::G0151),
    Tqrom(mappers::Tqrom),
    Ga23c(mappers::Ga23c),
    Mapper242(mappers::Mapper242),
    WaixingF003(mappers::WaixingF003),
    Mapper250(mappers::Mapper250),
    Fds(mappers::Fds),
}

macro_rules! dispatch_mapper {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            MapperImpl::Nrom(m) => m.$method($($arg),*),
            MapperImpl::Mmc1(m) => m.$method($($arg),*),
            MapperImpl::Uxrom(m) => m.$method($($arg),*),
            MapperImpl::Cnrom(m) => m.$method($($arg),*),
            MapperImpl::Cnrom185(m) => m.$method($($arg),*),
            MapperImpl::ColorDreams(m) => m.$method($($arg),*),
            MapperImpl::Contra100In1(m) => m.$method($($arg),*),
            MapperImpl::Cprom(m) => m.$method($($arg),*),
            MapperImpl::Mmc2(m) => m.$method($($arg),*),
            MapperImpl::Mmc3(m) => m.$method($($arg),*),
            MapperImpl::Mmc5(m) => m.$method($($arg),*),
            MapperImpl::Axrom(m) => m.$method($($arg),*),
            MapperImpl::SuperMagicCard(m) => m.$method($($arg),*),
            MapperImpl::FfeMapper8(m) => m.$method($($arg),*),
            MapperImpl::IremG101(m) => m.$method($($arg),*),
            MapperImpl::IremH3001(m) => m.$method($($arg),*),
            MapperImpl::TaitoTc0190(m) => m.$method($($arg),*),
            MapperImpl::JalecoJf17(m) => m.$method($($arg),*),
            MapperImpl::JalecoSs8806(m) => m.$method($($arg),*),
            MapperImpl::TaitoX1005(m) => m.$method($($arg),*),
            MapperImpl::TaitoX1017(m) => m.$method($($arg),*),
            MapperImpl::ConyYoko(m) => m.$method($($arg),*),
            MapperImpl::Bnrom(m) => m.$method($($arg),*),
            MapperImpl::Nina001(m) => m.$method($($arg),*),
            MapperImpl::Gxrom(m) => m.$method($($arg),*),
            MapperImpl::Rambo1(m) => m.$method($($arg),*),
            MapperImpl::Sunsoft3(m) => m.$method($($arg),*),
            MapperImpl::Sunsoft4(m) => m.$method($($arg),*),
            MapperImpl::Bandai74161(m) => m.$method($($arg),*),
            MapperImpl::Camerica(m) => m.$method($($arg),*),
            MapperImpl::Nina03(m) => m.$method($($arg),*),
            MapperImpl::Vrc1(m) => m.$method($($arg),*),
            MapperImpl::Mapper112(m) => m.$method($($arg),*),
            MapperImpl::Vrc3(m) => m.$method($($arg),*),
            MapperImpl::Mapper91(m) => m.$method($($arg),*),
            MapperImpl::NapoleonSenki(m) => m.$method($($arg),*),
            MapperImpl::HolyDiver(m) => m.$method($($arg),*),
            MapperImpl::JalecoJf11(m) => m.$method($($arg),*),
            MapperImpl::JalecoJf13(m) => m.$method($($arg),*),
            MapperImpl::BandaiFcg16(m) => m.$method($($arg),*),
            MapperImpl::Vrc4(m) => m.$method($($arg),*),
            MapperImpl::Vrc6(m) => m.$method($($arg),*),
            MapperImpl::Fme7(m) => m.$method($($arg),*),
            MapperImpl::Action52(m) => m.$method($($arg),*),
            MapperImpl::Quattro(m) => m.$method($($arg),*),
            MapperImpl::Namco163(m) => m.$method($($arg),*),
            MapperImpl::Vrc7(m) => m.$method($($arg),*),
            MapperImpl::J87(m) => m.$method($($arg),*),
            MapperImpl::Sunsoft2Mapper89(m) => m.$method($($arg),*),
            MapperImpl::JyAsic(m) => m.$method($($arg),*),
            MapperImpl::SenjouNoOokami(m) => m.$method($($arg),*),
            MapperImpl::IremTamS1(m) => m.$method($($arg),*),
            MapperImpl::VsSystem(m) => m.$method($($arg),*),
            MapperImpl::Namco108(m) => m.$method($($arg),*),
            MapperImpl::Sunsoft1(m) => m.$method($($arg),*),
            MapperImpl::KaraokeStudio(m) => m.$method($($arg),*),
            MapperImpl::Mapper240(m) => m.$method($($arg),*),
            MapperImpl::G0151(m) => m.$method($($arg),*),
            MapperImpl::Tqrom(m) => m.$method($($arg),*),
            MapperImpl::Ga23c(m) => m.$method($($arg),*),
            MapperImpl::Mapper242(m) => m.$method($($arg),*),
            MapperImpl::WaixingF003(m) => m.$method($($arg),*),
            MapperImpl::Mapper250(m) => m.$method($($arg),*),
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
    pub(super) fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        dispatch_mapper!(self, cpu_rom_offset, addr)
    }

    #[inline]
    pub(super) fn rom_mapping_token(&self) -> u64 {
        dispatch_mapper!(self, rom_mapping_token)
    }

    #[inline]
    pub(super) fn cpu_read(&mut self, addr: u16) -> u8 {
        dispatch_mapper!(self, cpu_read, addr)
    }

    #[inline]
    pub(super) fn cpu_read_open_bus(&mut self, addr: u16, open_bus: u8) -> u8 {
        dispatch_mapper!(self, cpu_read_open_bus, addr, open_bus)
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

    pub(super) fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8]) -> Option<u8> {
        dispatch_mapper!(self, ppu_nametable_read, addr, ciram)
    }

    pub(super) fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8]) -> bool {
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

    #[inline]
    pub(super) fn irq_pending(&self) -> bool {
        dispatch_mapper!(self, irq_pending)
    }

    #[inline]
    pub(super) fn notify_scanline(&mut self) {
        dispatch_mapper!(self, notify_scanline)
    }

    #[inline]
    pub(super) fn uses_qualified_ppu_a12(&self) -> bool {
        dispatch_mapper!(self, uses_qualified_ppu_a12)
    }

    pub(super) fn notify_ppu_a12(&mut self, high: bool, ppu_cycle: u64) {
        dispatch_mapper!(self, notify_ppu_a12, high, ppu_cycle)
    }

    pub(super) fn write_ppu_runtime_state(&self, w: &mut crate::save_state::StateWriter) {
        dispatch_mapper!(self, write_ppu_runtime_state, w)
    }

    pub(super) fn read_ppu_runtime_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        dispatch_mapper!(self, read_ppu_runtime_state, r)
    }

    #[inline]
    pub(super) fn clock_cpu(&mut self) {
        dispatch_mapper!(self, clock_cpu)
    }

    #[inline]
    pub(super) fn audio_output(&self) -> f32 {
        dispatch_mapper!(self, audio_output)
    }

    pub(super) fn load_trainer(&mut self, bytes: &[u8], header: &RomHeader) -> anyhow::Result<()> {
        dispatch_mapper!(self, load_trainer, bytes, header)
    }

    pub(super) fn dump_battery_data(&self) -> Option<Vec<u8>> {
        dispatch_mapper!(self, dump_battery_data)
    }

    pub(super) fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        dispatch_mapper!(self, load_battery_data, bytes)
    }

    pub(super) fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        match self {
            Self::Fds(mapper) => mapper.select_side(side),
            _ => anyhow::bail!("current NES cartridge is not Famicom Disk System content"),
        }
    }

    pub(super) fn fds_disk_side(&self) -> Option<u8> {
        match self {
            Self::Fds(mapper) => mapper.selected_side(),
            _ => None,
        }
    }
}
