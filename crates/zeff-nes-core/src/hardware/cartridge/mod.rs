mod dispatch;
mod header;
pub mod mappers;

use anyhow::{Result, bail};

use dispatch::MapperImpl;
pub use header::{
    ChrFetchKind, ConsoleType, Mirroring, NesMapper, RomFormat, RomHeader, TimingMode,
};

const HEADER_SIZE: usize = 16;
const TRAINER_SIZE: usize = 512;
const SAMURAI_SPIRITS_2_BAD_MAPPER90_CRC32: u32 = 0xBB64_D4A1;
const MORTAL_KOMBAT_3_SPECIAL_BAD_HEADER_CRC32: u32 = 0x4088_6623;
const SWEET_HOME_TRANSLATION_BAD_MAPPER33_CRC32: u32 = 0x74CE_0ADA;
const SMB_EXTREME_BAD_MAPPER64_CRC32: u32 = 0xD76A_E771;
const BAD_HEADER_MAPPER0_TO_16_PRG_CRC32: u32 = 0xAB30_62CF;
const BAD_HEADER_MAPPER0_TO_32_PRG_CRC32: u32 = 0xC0FE_D437;
const BAD_HEADER_MAPPER1_TO_MMC5_PRG_CRC32: u32 = 0xCD9A_CF43;
const BAD_HEADER_MMC5_NO_PRG_RAM_PRG_CRC32: u32 = 0xCD9A_CF43;
const BAD_HEADER_MAPPER2_TO_MMC1_PRG_CRC32: u32 = 0x57DD_23D1;
const BAD_HEADER_MAPPER3_TO_GXROM_PRG_CRC32: u32 = 0xDABA_9E8E;
const BAD_HEADER_MAPPER7_TO_34_PRG_CRC32: u32 = 0x5030_BCA8;
const BAD_HEADER_MAPPER7_TO_71_PRG_CRC32: u32 = 0xE62E_3382;
const BAD_HEADER_FALSE_FOUR_SCREEN_PRG_CRC32: u32 = 0x5913_64C9;
const MAPPER3_NO_BUS_CONFLICT_PRG_CRC32S: &[u32] = &[0xF2A9_F64D, 0xE366_4231];

pub(crate) trait Mapper: Send {
    fn cpu_peek(&self, addr: u16) -> u8;
    fn cpu_rom_offset(&self, _addr: u16) -> Option<usize> {
        None
    }
    fn rom_mapping_token(&self) -> u64 {
        0
    }
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.cpu_peek(addr)
    }
    fn cpu_read_open_bus(&mut self, addr: u16, _open_bus: u8) -> u8 {
        self.cpu_read(addr)
    }
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&mut self, addr: u16) -> u8;
    fn chr_read_kind(&mut self, addr: u16, _kind: ChrFetchKind) -> u8 {
        self.chr_read(addr)
    }
    fn chr_write(&mut self, addr: u16, val: u8);
    fn ppu_nametable_read(&mut self, _addr: u16, _ciram: &[u8]) -> Option<u8> {
        None
    }
    fn ppu_nametable_write(&mut self, _addr: u16, _val: u8, _ciram: &mut [u8]) -> bool {
        false
    }
    fn mirroring(&self) -> Mirroring;
    fn write_state(&self, w: &mut crate::save_state::StateWriter);
    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()>;

    fn irq_pending(&self) -> bool {
        false
    }

    fn notify_scanline(&mut self) {}

    fn uses_qualified_ppu_a12(&self) -> bool {
        false
    }

    fn notify_ppu_a12(&mut self, _high: bool, _ppu_cycle: u64) {}

    fn write_ppu_runtime_state(&self, _w: &mut crate::save_state::StateWriter) {}

    fn read_ppu_runtime_state(
        &mut self,
        _r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn clock_cpu(&mut self) {}

    fn audio_output(&self) -> f32 {
        0.0
    }

    fn load_trainer(&mut self, _bytes: &[u8], _header: &RomHeader) -> anyhow::Result<()> {
        Ok(())
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        None
    }

    fn load_battery_data(&mut self, _bytes: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct Cartridge {
    header: RomHeader,
    mapper: MapperImpl,
    rom_crc32: u32,
    prg_crc32: u32,
    effective_mapper_label: Option<&'static str>,
}

impl Cartridge {
    pub fn load(rom_data: &[u8]) -> Result<Self> {
        let mut header = RomHeader::parse(rom_data)?;

        if header.prg_rom_size == 0 {
            bail!(
                "ROM declares 0 bytes of PRG ROM, which is invalid:every NES ROM needs at least one PRG bank"
            );
        }

        let trainer_offset = if header.has_trainer { TRAINER_SIZE } else { 0 };
        let prg_start = HEADER_SIZE + trainer_offset;
        let rom_crc32 = crc32fast::hash(rom_data);
        let header_prg_size = header.prg_rom_size;
        let header_prg_crc32 = rom_data
            .get(prg_start..prg_start.saturating_add(header_prg_size))
            .map(crc32fast::hash);
        let mut mapper_kind = header.mapper_kind();
        let mut effective_mapper_label = None;
        apply_bad_header_mapper_overrides(
            rom_crc32,
            header_prg_crc32,
            &mut mapper_kind,
            &mut effective_mapper_label,
        );
        apply_bad_header_mirroring_overrides(header_prg_crc32, &mut header.mirroring);
        header.display_info();
        let prg_size = header.prg_rom_size;
        let mut chr_size = header.chr_rom_size;
        if matches!(mapper_kind, NesMapper::Mapper251) && chr_size == 0 {
            let trailing_size = rom_data.len().saturating_sub(prg_start);
            if trailing_size > prg_size {
                // Mapper 251 is a bad-header alias for GA23C. Known dumps can declare
                // 1 MiB PRG and 0 CHR while storing the remaining tile data after PRG.
                chr_size = trailing_size - prg_size;
            }
        }
        let chr_start = prg_start + prg_size;

        let expected_min = chr_start + chr_size;
        if rom_data.len() < expected_min {
            bail!(
                "ROM file truncated: header declares {} bytes of PRG+CHR data but file only has {} bytes total",
                expected_min,
                rom_data.len()
            );
        }

        let prg_rom = rom_data[prg_start..prg_start + prg_size].to_vec();
        let prg_crc32 = crc32fast::hash(&prg_rom);
        let chr_rom = if chr_size > 0 {
            rom_data[chr_start..chr_start + chr_size].to_vec()
        } else {
            vec![0; header::CHR_ROM_BANK_SIZE]
        };
        let trainer = header
            .has_trainer
            .then(|| rom_data[HEADER_SIZE..HEADER_SIZE + TRAINER_SIZE].to_vec());

        let mut mapper = match mapper_kind {
            NesMapper::Nrom => {
                MapperImpl::Nrom(mappers::Nrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::SxRom => {
                MapperImpl::Mmc1(mappers::Mmc1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::UxRom => {
                MapperImpl::Uxrom(mappers::Uxrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::CnRom => {
                let bus_conflicts =
                    mapper3_has_bus_conflicts(header.submapper_id, header_prg_crc32);
                if !bus_conflicts && header.submapper_id == 0 {
                    effective_mapper_label = Some("CNROM (no bus conflicts)");
                }
                MapperImpl::Cnrom(mappers::Cnrom::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    bus_conflicts,
                ))
            }
            NesMapper::CnRomWithProtectionDiodes => MapperImpl::Cnrom185(mappers::Cnrom185::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.submapper_id,
            )),
            NesMapper::ColorDreams => MapperImpl::ColorDreams(mappers::ColorDreams::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::FfeMapper12 => {
                if chr_size == 0 && prg_size == 0x8000 {
                    MapperImpl::Cprom(mappers::Cprom::new(prg_rom))
                } else {
                    bail!(
                        "Unsupported mapper: {}. Mapper 12 is only handled for CPROM-shaped bad headers",
                        header.mapper_label()
                    )
                }
            }
            NesMapper::CpRom => MapperImpl::Cprom(mappers::Cprom::new(prg_rom)),
            NesMapper::Contra100In1Function16 => MapperImpl::Contra100In1(
                mappers::Contra100In1::new(prg_rom, chr_rom, header.mirroring),
            ),
            NesMapper::PxRom => {
                MapperImpl::Mmc2(mappers::Mmc2::new_mmc2(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::FxRom => {
                MapperImpl::Mmc2(mappers::Mmc2::new_mmc4(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::TxRom => {
                MapperImpl::Mmc3(mappers::Mmc3::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::ExRom => {
                let prg_ram_size = header.prg_ram_size + header.prg_nvram_size;
                if prg_crc32 == BAD_HEADER_MMC5_NO_PRG_RAM_PRG_CRC32 {
                    MapperImpl::Mmc5(mappers::Mmc5::new_exact_prg_ram(
                        prg_rom,
                        chr_rom,
                        header.mirroring,
                        0,
                        false,
                    ))
                } else {
                    MapperImpl::Mmc5(mappers::Mmc5::new(
                        prg_rom,
                        chr_rom,
                        header.mirroring,
                        prg_ram_size,
                        header.has_battery || header.prg_nvram_size > 0,
                    ))
                }
            }
            NesMapper::FfeMapper6 => {
                MapperImpl::SuperMagicCard(mappers::SuperMagicCard::new_mapper6(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    header.submapper_id,
                    chr_size > 0,
                ))
            }
            NesMapper::AxRom => {
                MapperImpl::Axrom(mappers::Axrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::FfeMapper8 => {
                MapperImpl::FfeMapper8(mappers::FfeMapper8::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::IremG101 => {
                MapperImpl::IremG101(mappers::IremG101::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::IremH3001 => {
                MapperImpl::IremH3001(mappers::IremH3001::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::TaitoTc0190 => MapperImpl::TaitoTc0190(mappers::TaitoTc0190::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::JalecoJf17 => {
                MapperImpl::JalecoJf17(mappers::JalecoJf17::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Vrc3 => {
                MapperImpl::Vrc3(mappers::Vrc3::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::TaitoX1005 => MapperImpl::TaitoX1005(mappers::TaitoX1005::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.has_battery,
            )),
            NesMapper::TaitoX1017 => MapperImpl::TaitoX1017(mappers::TaitoX1017::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.has_battery || header.prg_nvram_size > 0,
            )),
            NesMapper::ConyYoko => {
                MapperImpl::ConyYoko(mappers::ConyYoko::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::JalecoEarlyMapper1 => MapperImpl::JalecoJf17(
                mappers::JalecoJf17::new_fixed_low_prg(prg_rom, chr_rom, header.mirroring),
            ),
            NesMapper::BnRom => {
                let is_nina = header.submapper_id == 1 || chr_size > 0;
                if is_nina {
                    MapperImpl::Nina001(mappers::Nina001::new(
                        prg_rom,
                        chr_rom,
                        header.prg_ram_size + header.prg_nvram_size,
                        header.has_battery || header.prg_nvram_size > 0,
                    ))
                } else {
                    MapperImpl::Bnrom(mappers::Bnrom::new(prg_rom, chr_rom, header.mirroring))
                }
            }
            NesMapper::GxRom => {
                MapperImpl::Gxrom(mappers::Gxrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Rambo1 => {
                MapperImpl::Rambo1(mappers::Rambo1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Sunsoft3 if prg_size == 0x8000 && chr_size == 0x4000 => {
                // Known bad-header dump shape, e.g. "Ninja Jajamaru Kun (Mapper 3
                // Wrong Size)": the header says mapper 67, but the program writes
                // $8000 as a CNROM CHR bank register.
                effective_mapper_label = Some("CNROM (bad mapper 67 header)");
                MapperImpl::Cnrom(mappers::Cnrom::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    true,
                ))
            }
            NesMapper::Sunsoft3 => {
                MapperImpl::Sunsoft3(mappers::Sunsoft3::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::AfterBurner => {
                MapperImpl::Sunsoft4(mappers::Sunsoft4::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Bandai74161 => MapperImpl::Bandai74161(mappers::Bandai74161::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::CamericaCodemasters => {
                MapperImpl::Camerica(mappers::Camerica::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Nina03Or06 => {
                MapperImpl::Nina03(mappers::Nina03::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Vrc1 => {
                MapperImpl::Vrc1(mappers::Vrc1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Mapper112 => {
                MapperImpl::Mapper112(mappers::Mapper112::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::JalecoJf11 => {
                MapperImpl::JalecoJf11(mappers::JalecoJf11::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Vrc1VsSystem => {
                MapperImpl::Vrc1(mappers::Vrc1::new(prg_rom, chr_rom, Mirroring::FourScreen))
            }
            NesMapper::NapoleonSenki => {
                MapperImpl::NapoleonSenki(mappers::NapoleonSenki::new(prg_rom, chr_rom))
            }
            NesMapper::HolyDiver => MapperImpl::HolyDiver(mappers::HolyDiver::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.submapper_id,
            )),
            NesMapper::JalecoJf13 => {
                MapperImpl::JalecoJf13(mappers::JalecoJf13::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::J87 => {
                MapperImpl::J87(mappers::J87::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::SunsoftMapper89 => MapperImpl::Sunsoft2Mapper89(
                mappers::Sunsoft2Mapper89::new(prg_rom, chr_rom, header.mirroring),
            ),
            NesMapper::JyAsic if rom_crc32 == SAMURAI_SPIRITS_2_BAD_MAPPER90_CRC32 => {
                effective_mapper_label = Some("J.Y. ASIC mapper 209 (bad mapper 90 header)");
                MapperImpl::JyAsic(mappers::JyAsic::new_mapper209(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                ))
            }
            NesMapper::JyAsic if rom_crc32 == MORTAL_KOMBAT_3_SPECIAL_BAD_HEADER_CRC32 => {
                MapperImpl::JyAsic(mappers::JyAsic::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::JyAsic => {
                MapperImpl::JyAsic(mappers::JyAsic::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::JyAsicMapper35 | NesMapper::JyAsicMapper209 => MapperImpl::JyAsic(
                mappers::JyAsic::new_mapper209(prg_rom, chr_rom, header.mirroring),
            ),
            NesMapper::JyAsicMapper211 => MapperImpl::JyAsic(mappers::JyAsic::new_mapper211(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::Mapper91 => MapperImpl::Mapper91(mappers::Mapper91::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.submapper_id,
            )),
            NesMapper::SenjouNoOokami => MapperImpl::SenjouNoOokami(mappers::SenjouNoOokami::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::IremTamS1 => {
                MapperImpl::IremTamS1(mappers::IremTamS1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::LegacyVsVrc1 => {
                if prg_size == 0x10000 && chr_size == 0x10000 {
                    MapperImpl::Vrc1(mappers::Vrc1::new(prg_rom, chr_rom, Mirroring::FourScreen))
                } else {
                    bail!(
                        "Unsupported mapper: {}. Mapper 98 is only handled for legacy Vs. VRC1 headers",
                        header.mapper_label()
                    )
                }
            }
            NesMapper::VsSystem => MapperImpl::VsSystem(mappers::VsSystem::new(prg_rom, chr_rom)),
            NesMapper::Mapper88 => MapperImpl::Namco108(mappers::Namco108::new_mapper88(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::BandaiEprom24C02 => MapperImpl::BandaiFcg16(mappers::BandaiFcg16::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.submapper_id,
                header.has_battery || header.prg_nvram_size >= 256,
            )),
            NesMapper::SuperMagicCard => {
                MapperImpl::SuperMagicCard(mappers::SuperMagicCard::new_mapper17(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    chr_size > 0,
                ))
            }
            NesMapper::JalecoSs8806 => MapperImpl::JalecoSs8806(mappers::JalecoSs8806::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::Vrc4A => {
                let (a0, a1) = match header.submapper_id {
                    1 => (0x02, 0x04),
                    2 => (0x40, 0x80),
                    _ => (0x02 | 0x40, 0x04 | 0x80),
                };
                MapperImpl::Vrc4(mappers::Vrc4::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    a0,
                    a1,
                ))
            }
            NesMapper::Vrc2A => MapperImpl::Vrc4(mappers::Vrc4::new_with_chr_bank_shift(
                prg_rom,
                chr_rom,
                header.mirroring,
                0x02,
                0x01,
                1,
            )),
            NesMapper::Vrc2B => {
                let (a0, a1) = match header.submapper_id {
                    1 | 3 => (0x01, 0x02),
                    2 => (0x04, 0x08),
                    _ => (0x01 | 0x04, 0x02 | 0x08),
                };
                MapperImpl::Vrc4(mappers::Vrc4::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    a0,
                    a1,
                ))
            }
            NesMapper::Vrc4B => {
                let (a0, a1) = match header.submapper_id {
                    1 | 3 => (0x02, 0x01),
                    2 => (0x08, 0x04),
                    _ => (0x02 | 0x08, 0x01 | 0x04),
                };
                MapperImpl::Vrc4(mappers::Vrc4::new(
                    prg_rom,
                    chr_rom,
                    header.mirroring,
                    a0,
                    a1,
                ))
            }
            NesMapper::Vrc6A => MapperImpl::Vrc6(mappers::Vrc6::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                false,
            )),
            NesMapper::Vrc6B => {
                MapperImpl::Vrc6(mappers::Vrc6::new(prg_rom, chr_rom, header.mirroring, true))
            }
            NesMapper::Fme7 => MapperImpl::Fme7(mappers::Fme7::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery,
            )),
            NesMapper::Ga23c | NesMapper::Mapper251 => MapperImpl::Ga23c(
                mappers::Ga23c::with_chr_ram(prg_rom, chr_rom, header.mirroring, chr_size == 0),
            ),
            NesMapper::Namco163 => MapperImpl::Namco163(mappers::Namco163::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery || header.prg_nvram_size > 0,
            )),
            NesMapper::Nina03Or06Multicart => MapperImpl::Nina03(mappers::Nina03::new_multicart(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::TqRom => {
                MapperImpl::Tqrom(mappers::Tqrom::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Vrc7 => MapperImpl::Vrc7(mappers::Vrc7::new(
                prg_rom,
                chr_rom,
                header.mirroring,
                header.prg_ram_size + header.prg_nvram_size,
                header.has_battery || header.prg_nvram_size > 0,
            )),
            NesMapper::Fds => MapperImpl::Fds(mappers::Fds::new(prg_rom, header.mirroring)),
            NesMapper::Action52 => {
                MapperImpl::Action52(mappers::Action52::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::CamericaCodemastersQuattro => {
                MapperImpl::Quattro(mappers::Quattro::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Sunsoft1 => {
                MapperImpl::Sunsoft1(mappers::Sunsoft1::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::KaraokeStudio => MapperImpl::KaraokeStudio(mappers::KaraokeStudio::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::Mapper240 => {
                MapperImpl::Mapper240(mappers::Mapper240::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Mapper242 => {
                MapperImpl::Mapper242(mappers::Mapper242::new(prg_rom, header.mirroring))
            }
            NesMapper::WaixingF003 => MapperImpl::WaixingF003(mappers::WaixingF003::new(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            NesMapper::G0151 => {
                MapperImpl::G0151(mappers::G0151::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::Mapper250 => {
                MapperImpl::Mapper250(mappers::Mapper250::new(prg_rom, chr_rom, header.mirroring))
            }
            NesMapper::DxRom => MapperImpl::Namco108(mappers::Namco108::new_dxrom(
                prg_rom,
                chr_rom,
                header.mirroring,
            )),
            _ => bail!(
                "Unsupported mapper: {}. This mapper is not yet implemented",
                header.mapper_label()
            ),
        };
        if let Some(trainer) = trainer.as_deref() {
            mapper.load_trainer(trainer, &header)?;
        }

        Ok(Self {
            header,
            mapper,
            rom_crc32,
            prg_crc32,
            effective_mapper_label,
        })
    }

    pub fn load_fds(image: mappers::FdsImage, bios_data: Vec<u8>) -> Result<Self> {
        if bios_data.len() != mappers::FDS_BIOS_SIZE {
            bail!(
                "FDS BIOS size mismatch: expected {} bytes, got {}",
                mappers::FDS_BIOS_SIZE,
                bios_data.len()
            );
        }

        let rom_crc32 = fds_image_crc32(&image);
        let prg_crc32 = crc32fast::hash(&bios_data);
        let header = fds_synthetic_header(bios_data.len());
        let mapper = MapperImpl::Fds(mappers::Fds::with_disk_image(
            bios_data,
            image,
            header.mirroring,
        ));

        Ok(Self {
            header,
            mapper,
            rom_crc32,
            prg_crc32,
            effective_mapper_label: Some("Famicom Disk System"),
        })
    }

    pub fn header(&self) -> &RomHeader {
        &self.header
    }

    pub fn rom_crc32(&self) -> u32 {
        self.rom_crc32
    }

    pub fn prg_crc32(&self) -> u32 {
        self.prg_crc32
    }

    pub fn effective_mapper_label(&self) -> String {
        self.effective_mapper_label
            .map(str::to_owned)
            .unwrap_or_else(|| self.header.mapper_label())
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    #[inline]
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_read_open_bus(&mut self, addr: u16, open_bus: u8) -> u8 {
        self.mapper.cpu_read_open_bus(addr, open_bus)
    }

    #[inline]
    pub fn cpu_peek(&self, addr: u16) -> u8 {
        self.mapper.cpu_peek(addr)
    }

    pub fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        self.mapper.cpu_rom_offset(addr)
    }

    pub fn rom_mapping_token(&self) -> u64 {
        self.mapper.rom_mapping_token()
    }

    #[inline]
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    #[inline]
    pub fn chr_read(&mut self, addr: u16) -> u8 {
        self.mapper.chr_read(addr)
    }

    #[inline]
    pub fn chr_read_with_kind(&mut self, addr: u16, kind: ChrFetchKind) -> u8 {
        self.mapper.chr_read_kind(addr, kind)
    }

    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }

    pub fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8]) -> Option<u8> {
        self.mapper.ppu_nametable_read(addr, ciram)
    }

    pub fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8]) -> bool {
        self.mapper.ppu_nametable_write(addr, val, ciram)
    }

    #[inline]
    pub fn irq_pending(&self) -> bool {
        self.mapper.irq_pending()
    }

    #[inline]
    pub fn notify_scanline(&mut self) {
        self.mapper.notify_scanline();
    }

    #[inline]
    pub fn uses_qualified_ppu_a12(&self) -> bool {
        self.mapper.uses_qualified_ppu_a12()
    }

    #[inline]
    pub fn notify_ppu_a12(&mut self, high: bool, ppu_cycle: u64) {
        self.mapper.notify_ppu_a12(high, ppu_cycle);
    }

    pub(crate) fn write_ppu_runtime_state(&self, w: &mut crate::save_state::StateWriter) {
        self.mapper.write_ppu_runtime_state(w);
    }

    pub(crate) fn read_ppu_runtime_state(
        &mut self,
        r: &mut crate::save_state::StateReader,
    ) -> anyhow::Result<()> {
        self.mapper.read_ppu_runtime_state(r)
    }

    #[inline]
    pub fn clock_cpu(&mut self) {
        self.mapper.clock_cpu();
    }

    #[inline]
    pub fn audio_output(&self) -> f32 {
        self.mapper.audio_output()
    }

    pub fn dump_battery_data(&self) -> Option<Vec<u8>> {
        self.mapper.dump_battery_data()
    }

    pub fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.mapper.load_battery_data(bytes)
    }

    pub fn set_fds_disk_side(&mut self, side: u8) -> anyhow::Result<()> {
        self.mapper.set_fds_disk_side(side)
    }

    pub fn fds_disk_side(&self) -> Option<u8> {
        self.mapper.fds_disk_side()
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        self.mapper.write_state(w);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mapper.read_state(r)
    }
}

fn fds_image_crc32(image: &mappers::FdsImage) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    for side in image.sides() {
        hasher.update(side);
    }
    hasher.finalize()
}

fn fds_synthetic_header(bios_size: usize) -> RomHeader {
    RomHeader {
        format: RomFormat::Nes2,
        prg_rom_size: bios_size,
        chr_rom_size: 0,
        mapper_id: NesMapper::Fds.id(),
        submapper_id: 0,
        mirroring: Mirroring::Horizontal,
        has_battery: true,
        has_trainer: false,
        console_type: ConsoleType::Nes,
        prg_ram_size: 0x8000,
        prg_nvram_size: 0,
        chr_ram_size: 0x2000,
        chr_nvram_size: 0,
        timing: TimingMode::Ntsc,
        misc_roms: 0,
        default_expansion_device: 0,
    }
}

fn mapper3_has_bus_conflicts(submapper_id: u8, prg_crc32: Option<u32>) -> bool {
    match submapper_id {
        1 => false,
        2 => true,
        _ => !prg_crc32.is_some_and(|crc| MAPPER3_NO_BUS_CONFLICT_PRG_CRC32S.contains(&crc)),
    }
}

fn apply_bad_header_mirroring_overrides(prg_crc32: Option<u32>, mirroring: &mut Mirroring) {
    if prg_crc32 == Some(BAD_HEADER_FALSE_FOUR_SCREEN_PRG_CRC32) {
        *mirroring = Mirroring::Horizontal;
    }
}

fn apply_bad_header_mapper_overrides(
    rom_crc32: u32,
    prg_crc32: Option<u32>,
    mapper_kind: &mut NesMapper,
    effective_mapper_label: &mut Option<&'static str>,
) {
    match rom_crc32 {
        MORTAL_KOMBAT_3_SPECIAL_BAD_HEADER_CRC32 => {
            *mapper_kind = NesMapper::JyAsic;
            *effective_mapper_label = Some("J.Y. ASIC mapper 90 (bad mapper 10 header)");
        }
        SWEET_HOME_TRANSLATION_BAD_MAPPER33_CRC32 => {
            *mapper_kind = NesMapper::SxRom;
            *effective_mapper_label = Some("SxROM / MMC1 (bad mapper 33 header)");
        }
        SMB_EXTREME_BAD_MAPPER64_CRC32 => {
            *mapper_kind = NesMapper::Nrom;
            *effective_mapper_label = Some("NROM (bad mapper 64 header)");
        }
        _ => {}
    }

    if matches!(*mapper_kind, NesMapper::AxRom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER7_TO_34_PRG_CRC32)
    {
        *mapper_kind = NesMapper::BnRom;
        *effective_mapper_label = Some("BNROM / mapper 34 (bad mapper 7 header)");
    }

    if matches!(*mapper_kind, NesMapper::AxRom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER7_TO_71_PRG_CRC32)
    {
        *mapper_kind = NesMapper::CamericaCodemasters;
        *effective_mapper_label = Some("Camerica / Codemasters mapper 71 (bad mapper 7 header)");
    }

    if matches!(*mapper_kind, NesMapper::CnRom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER3_TO_GXROM_PRG_CRC32)
    {
        *mapper_kind = NesMapper::GxRom;
        *effective_mapper_label = Some("GxROM (bad mapper 3 header)");
    }

    if matches!(*mapper_kind, NesMapper::Nrom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER0_TO_16_PRG_CRC32)
    {
        *mapper_kind = NesMapper::BandaiEprom24C02;
        *effective_mapper_label = Some("Bandai mapper 16 (bad mapper 0 header)");
    }

    if matches!(*mapper_kind, NesMapper::Nrom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER0_TO_32_PRG_CRC32)
    {
        *mapper_kind = NesMapper::IremG101;
        *effective_mapper_label = Some("Irem G-101 / mapper 32 (bad mapper 0 header)");
    }

    if matches!(*mapper_kind, NesMapper::SxRom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER1_TO_MMC5_PRG_CRC32)
    {
        *mapper_kind = NesMapper::ExRom;
        *effective_mapper_label = Some("ExROM / MMC5 (bad mapper 1 header)");
    }

    if matches!(*mapper_kind, NesMapper::UxRom)
        && prg_crc32 == Some(BAD_HEADER_MAPPER2_TO_MMC1_PRG_CRC32)
    {
        *mapper_kind = NesMapper::SxRom;
        *effective_mapper_label = Some("SxROM / MMC1 (bad mapper 2 header)");
    }
}

#[cfg(test)]
mod test_utils;
#[cfg(test)]
mod tests;
