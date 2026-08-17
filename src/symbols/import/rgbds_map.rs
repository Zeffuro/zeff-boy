use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, StorageLocation, SymbolId,
    SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct RgbdsMapImporter;

#[derive(Clone, Copy)]
struct BankContext<'a> {
    kind: &'a str,
    bank: u32,
}

impl SymbolImporter for RgbdsMapImporter {
    fn name(&self) -> &'static str {
        "RGBDS .map"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Gb || !file_name.to_ascii_lowercase().ends_with(".map") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        if text.lines().any(|line| parse_bank(line.trim()).is_some())
            && text
                .lines()
                .any(|line| parse_section(line.trim()).is_some())
        {
            100
        } else {
            0
        }
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS
            .union(ImportCapabilities::SECTIONS)
            .union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Gb,
            "RGBDS maps require a Game Boy target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule::default();
        let mut bank = None;
        for (index, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if let Some(parsed) = parse_bank(line) {
                bank = Some(parsed);
                continue;
            }
            let Some(bank) = bank else {
                continue;
            };
            if line.starts_with("SECTION:") {
                let Some((address, size, name)) = parse_section(line) else {
                    module
                        .diagnostics
                        .push(format!("line {}: ignored malformed section", index + 1));
                    continue;
                };
                module.symbols.push(record(
                    name,
                    address,
                    Some(size),
                    SymbolKind::Section,
                    bank,
                    ctx,
                ));
            } else if let Some((address, name)) = parse_label(line) {
                module.symbols.push(record(
                    name.to_owned(),
                    address,
                    None,
                    SymbolKind::Label,
                    bank,
                    ctx,
                ));
            }
        }
        anyhow::ensure!(
            !module.symbols.is_empty(),
            "map contains no sections or labels"
        );
        Ok(module)
    }
}

fn record(
    name: String,
    address: u16,
    size: Option<u64>,
    kind: SymbolKind,
    bank: BankContext<'_>,
    ctx: &ImportContext,
) -> SymbolRecord {
    let is_rom = matches!(bank.kind, "ROM0" | "ROMX");
    let storage = is_rom.then_some(StorageLocation {
        image: ctx.image,
        region: ctx.rom_region,
        offset: u64::from(bank.bank) * 0x4000 + u64::from(address & 0x3FFF),
    });
    SymbolRecord {
        id: SymbolId(0),
        name,
        location: SymbolLocation {
            cpu: Some(CpuLocation {
                space: ctx.cpu_space,
                address: u64::from(address),
            }),
            storage,
            bank: Some(bank.bank),
            exec_mode: if is_rom {
                ExecMode::Sm83
            } else {
                ExecMode::Unknown
            },
        },
        value: None,
        size,
        kind,
        scope: if kind == SymbolKind::Section {
            SymbolScope::Module
        } else {
            SymbolScope::Global
        },
        provenance: Provenance {
            kind: ProvenanceKind::LinkMap,
            source: ctx.source_name.clone(),
        },
        confidence: Confidence::Exact,
        comment: None,
    }
}

fn parse_bank(line: &str) -> Option<BankContext<'_>> {
    let line = line.strip_suffix(':')?;
    let (kind, bank) = line.split_once(" bank #")?;
    if !matches!(
        kind,
        "ROM0" | "ROMX" | "VRAM" | "SRAM" | "WRAM0" | "WRAMX" | "OAM" | "HRAM"
    ) {
        return None;
    }
    Some(BankContext {
        kind,
        bank: bank.parse().ok()?,
    })
}

fn parse_section(line: &str) -> Option<(u16, u64, String)> {
    let rest = line.strip_prefix("SECTION: $")?;
    let address_end = rest.find(['-', ' '])?;
    let address = u16::from_str_radix(&rest[..address_end], 16).ok()?;
    let size_start = line.find(" ($")? + 3;
    let size_end = line[size_start..].find(' ')? + size_start;
    let size = u64::from_str_radix(&line[size_start..size_end], 16).ok()?;
    let name_start = line.find("[\"")? + 2;
    let name_end = line.rfind("\"]")?;
    Some((address, size, unescape_name(&line[name_start..name_end])?))
}

fn parse_label(line: &str) -> Option<(u16, &str)> {
    let (address, name) = line.strip_prefix('$')?.split_once(" = ")?;
    Some((u16::from_str_radix(address, 16).ok()?, name))
}

fn unescape_name(name: &str) -> Option<String> {
    let mut output = String::new();
    let mut chars = name.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        output.push(match chars.next()? {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '\\' => '\\',
            '"' => '"',
            _ => return None,
        });
    }
    Some(output)
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
            source_name: Some("game.map".to_owned()),
        }
    }

    #[test]
    fn imports_rom_sections_labels_and_ranges() {
        let data = br#"SUMMARY:
ROMX bank #2:
	SECTION: $4560-$457f ($0020 bytes) ["Player code"]
	         $4560 = UpdatePlayer
	EMPTY: $4580-$7fff ($3a80 bytes)
"#;
        let module = RgbdsMapImporter.import(data, &context()).unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].kind, SymbolKind::Section);
        assert_eq!(module.symbols[0].size, Some(0x20));
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x8560);
        assert_eq!(module.symbols[1].name, "UpdatePlayer");
    }

    #[test]
    fn keeps_ram_sections_out_of_rom_storage() {
        let data = b"WRAMX bank #1:\n\tSECTION: $d000-$d005 ($0006 bytes) [\"ram\"]";
        let module = RgbdsMapImporter.import(data, &context()).unwrap();
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0xD000);
        assert!(module.symbols[0].location.storage.is_none());
    }
}
