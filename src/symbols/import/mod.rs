pub(crate) mod rgbds;

use zeff_emu_common::system::System;

use super::{AddressSpaceId, ImageId, RegionId, SymbolRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImportCapabilities(u32);

impl ImportCapabilities {
    pub(crate) const SYMBOLS: Self = Self(1 << 0);
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
    let rgbds = rgbds::RgbdsSymImporter;
    let importers: [&dyn SymbolImporter; 1] = [&rgbds];
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
}
