pub(crate) mod ca65;
pub(crate) mod fceux;
pub(crate) mod gnu_nm;
pub(crate) mod nocash;
pub(crate) mod rgbds;
pub(crate) mod rgbds_map;
pub(crate) mod wla;

use zeff_emu_common::system::System;

use super::{AddressSpaceId, ImageId, RegionId, SymbolRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImportCapabilities(u32);

impl ImportCapabilities {
    pub(crate) const SYMBOLS: Self = Self(1 << 0);
    pub(crate) const SECTIONS: Self = Self(1 << 1);
    pub(crate) const BANKED_ADDR: Self = Self(1 << 6);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetInfo {
    pub(crate) system: System,
}

#[derive(Clone, Debug)]
pub(crate) struct ImportContext {
    pub(crate) target: TargetInfo,
    pub(crate) image: ImageId,
    pub(crate) rom_region: RegionId,
    pub(crate) cpu_space: AddressSpaceId,
    pub(crate) source_name: Option<String>,
}

#[derive(Default)]
pub(crate) struct SymbolModule {
    pub(crate) format: String,
    pub(crate) symbols: Vec<SymbolRecord>,
    pub(crate) diagnostics: Vec<String>,
}

pub(crate) trait SymbolImporter: Send + Sync {
    fn name(&self) -> &'static str;
    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8;
    fn capabilities(&self) -> ImportCapabilities;
    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule>;
}

pub(crate) fn import_symbols(
    file_name: &str,
    data: &[u8],
    ctx: &ImportContext,
) -> anyhow::Result<SymbolModule> {
    let ca65 = ca65::Ca65DbgImporter;
    let fceux = fceux::FceuxNameListImporter;
    let gnu_nm = gnu_nm::GnuNmSymImporter;
    let rgbds = rgbds::RgbdsSymImporter;
    let nocash = nocash::NoCashSymImporter;
    let rgbds_map = rgbds_map::RgbdsMapImporter;
    let wla = wla::WlaSymImporter;
    let importers: [&dyn SymbolImporter; 7] =
        [&ca65, &fceux, &gnu_nm, &rgbds, &nocash, &rgbds_map, &wla];
    let Some((confidence, importer)) = importers
        .into_iter()
        .map(|importer| (importer.probe(file_name, data, &ctx.target), importer))
        .max_by_key(|(confidence, _)| *confidence)
    else {
        anyhow::bail!("no symbol importers are registered");
    };
    anyhow::ensure!(confidence > 0, "unsupported symbol file: {file_name}");
    let mut module = importer.import(data, ctx)?;
    module.format = importer.name().to_owned();
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo { system: System::Gb },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.sym".into()),
        }
    }

    #[test]
    fn registry_selects_rgbds_and_rejects_other_dialects() {
        let imported = import_symbols("game.sym", b"01:4560 Update", &context()).unwrap();
        assert_eq!(imported.symbols.len(), 1);
        assert!(import_symbols("game.sym", b"Update = $4560", &context()).is_err());
    }

    #[test]
    fn registry_selects_nocash_for_gba() {
        let mut ctx = context();
        ctx.target.system = System::Gba;
        let imported = import_symbols("game.sym", b"080001EC .thumb\n080001EC Init", &ctx).unwrap();
        assert_eq!(imported.format, "no$gba .sym");
        assert_eq!(imported.symbols.len(), 1);
        assert_eq!(
            imported.symbols[0].location.exec_mode,
            super::super::ExecMode::Thumb
        );
    }

    #[test]
    fn registry_selects_rgbds_map() {
        let imported = import_symbols(
            "game.map",
            b"ROMX bank #2:\n\tSECTION: $4560-$457f ($0020 bytes) [\"Player code\"]",
            &context(),
        )
        .unwrap();
        assert_eq!(imported.format, "RGBDS .map");
        assert_eq!(imported.symbols[0].kind, super::super::SymbolKind::Section);
    }

    #[test]
    fn registry_selects_fceux_and_wla() {
        let mut nes = context();
        nes.target.system = System::Nes;
        let imported = import_symbols("contra.nes.0.nl", b"$8001#Start#", &nes).unwrap();
        assert_eq!(imported.format, "FCEUX NameList");

        let imported = import_symbols(
            "game.sym",
            b"[information]\nversion 3\nwlasymbol true\n[labels]\n00:0150 Entry",
            &context(),
        )
        .unwrap();
        assert_eq!(imported.format, "WLA .sym");
    }

    #[test]
    fn registry_selects_ca65_debug_symbols() {
        let mut nes = context();
        nes.target.system = System::Nes;
        let imported = import_symbols(
            "game.fds.dbg",
            b"version\tmajor=2,minor=0\nsym\tid=0,name=\"Start\",val=0xC000,type=lab",
            &nes,
        )
        .unwrap();
        assert_eq!(imported.format, "cc65 .dbg");
    }

    #[test]
    fn registry_selects_gnu_nm_gba_symbols() {
        let mut gba = context();
        gba.target.system = System::Gba;
        let imported = import_symbols(
            "game.sym",
            b"08000204 g 00000000 Init\n02000000 l 0001c000 gHeap",
            &gba,
        )
        .unwrap();
        assert_eq!(imported.format, "GNU nm .sym");
        assert_eq!(imported.symbols.len(), 2);
    }
}
