use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, StorageLocation, SymbolId,
    SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct GnuNmSymImporter;

impl SymbolImporter for GnuNmSymImporter {
    fn name(&self) -> &'static str {
        "GNU nm .sym"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Gba || !file_name.to_ascii_lowercase().ends_with(".sym") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        let mut candidates = 0;
        let mut valid = 0;
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            candidates += 1;
            valid += usize::from(parse_line(line).is_some());
            if candidates == 16 {
                break;
            }
        }
        if valid == 0 {
            0
        } else if valid == candidates {
            100
        } else if valid * 4 >= candidates * 3 {
            80
        } else {
            0
        }
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Gba,
            "GNU nm symbols require a GBA target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule {
            symbols: Vec::with_capacity(text.lines().count()),
            ..SymbolModule::default()
        };
        for raw_line in text.lines() {
            let Some(entry) = parse_line(raw_line.trim()) else {
                continue;
            };
            let storage = gba_rom_offset(entry.address).map(|offset| StorageLocation {
                image: ctx.image,
                region: ctx.rom_region,
                offset,
            });
            let is_rom = storage.is_some();
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: entry.name.to_owned(),
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(entry.address),
                    }),
                    storage,
                    bank: None,
                    exec_mode: ExecMode::Unknown,
                },
                value: None,
                size: (entry.size != 0).then_some(u64::from(entry.size)),
                kind: if is_rom {
                    SymbolKind::Label
                } else {
                    SymbolKind::Data
                },
                scope: if entry.binding == "l" {
                    SymbolScope::Local
                } else {
                    SymbolScope::Global
                },
                provenance: Provenance {
                    kind: ProvenanceKind::Build,
                    source: ctx.source_name.clone(),
                },
                confidence: Confidence::Exact,
                comment: None,
            });
        }
        anyhow::ensure!(!module.symbols.is_empty(), "no GNU nm symbols found");
        Ok(module)
    }
}

struct ParsedLine<'a> {
    address: u32,
    binding: &'a str,
    size: u32,
    name: &'a str,
}

fn parse_line(line: &str) -> Option<ParsedLine<'_>> {
    let mut fields = line.split_whitespace();
    let address = fields.next()?;
    let binding = fields.next()?;
    let size = fields.next()?;
    let name = fields.next()?;
    if fields.next().is_some() || address.len() != 8 || !matches!(binding, "g" | "l") {
        return None;
    }
    Some(ParsedLine {
        address: u32::from_str_radix(address, 16).ok()?,
        binding,
        size: u32::from_str_radix(size, 16).ok()?,
        name,
    })
}

fn gba_rom_offset(address: u32) -> Option<u64> {
    (0x0800_0000..=0x0DFF_FFFF)
        .contains(&address)
        .then(|| u64::from((address - 0x0800_0000) & 0x01FF_FFFF))
}

#[cfg(test)]
pub(crate) fn generated_large_fixture(symbol_count: usize) -> Vec<u8> {
    use std::fmt::Write as _;

    let mut text = String::with_capacity(symbol_count * 36);
    for index in 0..symbol_count {
        let address = 0x0800_0000 + u32::try_from(index).unwrap() * 4;
        let binding = if index & 1 == 0 { 'g' } else { 'l' };
        writeln!(text, "{address:08X} {binding} 00000004 symbol_{index:05X}").unwrap();
    }
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo {
                system: System::Gba,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.sym".into()),
        }
    }

    #[test]
    fn imports_rom_and_ram_symbols() {
        let module = GnuNmSymImporter
            .import(
                b"08000204 g 00000020 Init\n02000000 l 0001c000 gHeap",
                &context(),
            )
            .unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x204);
        assert_eq!(module.symbols[0].kind, SymbolKind::Label);
        assert_eq!(module.symbols[1].kind, SymbolKind::Data);
        assert_eq!(module.symbols[1].scope, SymbolScope::Local);
    }

    #[test]
    fn imports_large_generated_fixture() {
        let data = generated_large_fixture(70_001);
        let module = GnuNmSymImporter.import(&data, &context()).unwrap();
        assert!(module.symbols.len() > 70_000);
        assert!(module.diagnostics.is_empty());
    }
}
