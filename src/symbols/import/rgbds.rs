use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, StorageLocation, SymbolId,
    SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct RgbdsSymImporter;

impl SymbolImporter for RgbdsSymImporter {
    fn name(&self) -> &'static str {
        "RGBDS .sym"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Gb || !file_name.to_ascii_lowercase().ends_with(".sym") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        let mut candidates = 0;
        let mut valid = 0;
        for line in text.lines().map(code_part).filter(|line| !line.is_empty()) {
            candidates += 1;
            if parse_line(line).is_some() {
                valid += 1;
            }
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
        ImportCapabilities::SYMBOLS.union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Gb,
            "RGBDS symbols require a Game Boy target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule::default();
        for (index, raw_line) in text.lines().enumerate() {
            let line = code_part(raw_line);
            if line.is_empty() {
                continue;
            }
            let Some(parsed) = parse_line(line) else {
                module
                    .diagnostics
                    .push(format!("line {}: ignored malformed symbol", index + 1));
                continue;
            };
            let (name, location, value, kind) = match parsed {
                ParsedLine::Banked {
                    bank,
                    address,
                    name,
                } => {
                    let storage = (address <= 0x7FFF).then_some(StorageLocation {
                        image: ctx.image,
                        region: ctx.rom_region,
                        offset: u64::from(bank) * 0x4000 + u64::from(address & 0x3FFF),
                    });
                    (
                        name,
                        SymbolLocation {
                            cpu: Some(CpuLocation {
                                space: ctx.cpu_space,
                                address: u64::from(address),
                            }),
                            storage,
                            bank: Some(bank),
                            exec_mode: ExecMode::Sm83,
                        },
                        None,
                        SymbolKind::Label,
                    )
                }
                ParsedLine::Constant { value, name } => (
                    name,
                    SymbolLocation {
                        cpu: None,
                        storage: None,
                        bank: None,
                        exec_mode: ExecMode::Unknown,
                    },
                    Some(value),
                    SymbolKind::Constant,
                ),
            };
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: name.into(),
                location,
                value,
                size: None,
                kind,
                scope: if name.starts_with('.') {
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
        Ok(module)
    }
}

fn code_part(line: &str) -> &str {
    line.split([';', '#']).next().unwrap_or_default().trim()
}

enum ParsedLine<'a> {
    Banked {
        bank: u32,
        address: u16,
        name: &'a str,
    },
    Constant {
        value: u64,
        name: &'a str,
    },
}

fn parse_line(line: &str) -> Option<ParsedLine<'_>> {
    let mut fields = line.split_whitespace();
    let location = fields.next()?;
    let name = fields.next()?;
    if fields.next().is_some() || name.is_empty() {
        return None;
    }
    if let Some((bank, address)) = location.split_once(':') {
        return Some(ParsedLine::Banked {
            bank: u32::from_str_radix(bank, 16).ok()?,
            address: u16::from_str_radix(address, 16).ok()?,
            name,
        });
    }
    Some(ParsedLine::Constant {
        value: u64::from_str_radix(location, 16).ok()?,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

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
    fn probe_distinguishes_rgbds_shape() {
        let importer = RgbdsSymImporter;
        let target = TargetInfo { system: System::Gb };

        assert_eq!(
            importer.probe("game.sym", b"00:0150 Entry\n23:4560 Update", &target),
            100
        );
        assert_eq!(importer.probe("game.sym", b"Update = $4560", &target), 0);
    }

    #[test]
    fn import_preserves_banked_storage_and_ram_symbols() {
        let module = RgbdsSymImporter
            .import(
                b"00:0150 Entry ; start\n23:4560 Update\n01:C120 WorkingValue\nbad line here",
                &context(),
            )
            .unwrap();

        assert_eq!(module.symbols.len(), 3);
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x150);
        assert_eq!(module.symbols[1].location.storage.unwrap().offset, 0x8C560);
        assert!(module.symbols[2].location.storage.is_none());
        assert_eq!(module.diagnostics.len(), 1);
    }

    #[test]
    fn import_keeps_unbanked_constants() {
        let module = RgbdsSymImporter.import(b"36 PICS_FIX", &context()).unwrap();
        assert_eq!(module.symbols[0].kind, SymbolKind::Constant);
        assert_eq!(module.symbols[0].value, Some(0x36));
        assert!(module.symbols[0].location.cpu.is_none());
    }

    #[test]
    #[ignore = "requires ZEFF_TEST_RGBDS_SYM with a large RGBDS symbol file"]
    fn imports_external_fixture_when_configured() {
        let path = std::env::var("ZEFF_TEST_RGBDS_SYM")
            .expect("ZEFF_TEST_RGBDS_SYM must name an RGBDS symbol file");
        let data = std::fs::read(path).unwrap();
        let module = RgbdsSymImporter.import(&data, &context()).unwrap();
        assert!(module.symbols.len() > 10_000);
        assert!(module.diagnostics.is_empty());
    }
}
