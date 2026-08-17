use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, StorageLocation, SymbolId,
    SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct NoCashSymImporter;

impl SymbolImporter for NoCashSymImporter {
    fn name(&self) -> &'static str {
        "no$gba .sym"
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
        ImportCapabilities::SYMBOLS
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Gba,
            "no$gba symbols require a GBA target"
        );
        let text = std::str::from_utf8(data)?;
        let mut entries = Vec::new();
        let mut module = SymbolModule::default();
        for (index, raw_line) in text.lines().enumerate() {
            let line = code_part(raw_line);
            if line.is_empty() {
                continue;
            }
            if let Some(entry) = parse_line(line) {
                entries.push(entry);
            } else {
                module
                    .diagnostics
                    .push(format!("line {}: ignored malformed symbol", index + 1));
            }
        }

        let modes = entries
            .iter()
            .filter_map(|entry| match entry.kind {
                EntryKind::Arm => Some((entry.address, ExecMode::Arm)),
                EntryKind::Thumb => Some((entry.address, ExecMode::Thumb)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let data_ranges = entries
            .iter()
            .filter_map(|entry| match entry.kind {
                EntryKind::Data(size) => Some((entry.address, size)),
                _ => None,
            })
            .collect::<Vec<_>>();

        for entry in entries {
            let EntryKind::Label(name) = entry.kind else {
                continue;
            };
            let data_range = data_ranges.iter().find(|(start, size)| {
                entry.address >= *start && entry.address < start.saturating_add(*size)
            });
            let exec_mode = if data_range.is_some() {
                ExecMode::Unknown
            } else {
                modes
                    .iter()
                    .filter(|(address, _)| *address <= entry.address)
                    .max_by_key(|(address, _)| *address)
                    .map_or(ExecMode::Unknown, |(_, mode)| *mode)
            };
            let storage = gba_rom_offset(entry.address).map(|offset| StorageLocation {
                image: ctx.image,
                region: ctx.rom_region,
                offset,
            });
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: name.to_owned(),
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(entry.address),
                    }),
                    storage,
                    bank: None,
                    exec_mode,
                },
                value: None,
                size: data_range
                    .filter(|(start, _)| *start == entry.address)
                    .map(|(_, size)| u64::from(*size)),
                kind: if data_range.is_some() {
                    SymbolKind::Data
                } else {
                    SymbolKind::Label
                },
                scope: SymbolScope::Global,
                provenance: Provenance {
                    kind: ProvenanceKind::DebugFormat,
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
    line.split(';').next().unwrap_or_default().trim()
}

struct ParsedEntry<'a> {
    address: u32,
    kind: EntryKind<'a>,
}

enum EntryKind<'a> {
    Label(&'a str),
    Arm,
    Thumb,
    Data(u32),
    Pool,
}

fn parse_line(line: &str) -> Option<ParsedEntry<'_>> {
    let mut fields = line.split_whitespace();
    let address = fields.next()?;
    let name = fields.next()?;
    if fields.next().is_some() || address.len() != 8 || name.is_empty() {
        return None;
    }
    let address = u32::from_str_radix(address, 16).ok()?;
    let lower = name.to_ascii_lowercase();
    let kind = match lower.as_str() {
        ".arm" => EntryKind::Arm,
        ".thumb" => EntryKind::Thumb,
        ".pool" => EntryKind::Pool,
        _ => match lower.split_once(':') {
            Some((".byt" | ".wrd" | ".dbl" | ".asc", size)) => {
                EntryKind::Data(u32::from_str_radix(size, 16).ok()?)
            }
            _ => EntryKind::Label(name),
        },
    };
    Some(ParsedEntry { address, kind })
}

fn gba_rom_offset(address: u32) -> Option<u64> {
    (0x0800_0000..=0x0DFF_FFFF)
        .contains(&address)
        .then(|| u64::from((address - 0x0800_0000) & 0x01FF_FFFF))
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
    fn probe_requires_nocash_gba_shape() {
        let importer = NoCashSymImporter;
        let target = TargetInfo {
            system: System::Gba,
        };
        assert_eq!(
            importer.probe("game.sym", b"080000C0 start\n080001EC .thumb", &target),
            100
        );
        assert_eq!(importer.probe("game.sym", b"00:0150 Entry", &target), 0);
    }

    #[test]
    fn imports_modes_rom_offsets_and_data_ranges() {
        let module = NoCashSymImporter
            .import(
                b"; example\n080001EC .thumb\n080001EC init_video\n08000210 .arm\n08000210 irq_handler\n08000228 jumplist\n08000228 .dbl:0010\n06000000 vram_base",
                &context(),
            )
            .unwrap();
        assert_eq!(module.symbols.len(), 4);
        assert_eq!(module.symbols[0].location.exec_mode, ExecMode::Thumb);
        assert_eq!(module.symbols[0].location.storage.unwrap().offset, 0x1EC);
        assert_eq!(module.symbols[1].location.exec_mode, ExecMode::Arm);
        assert_eq!(module.symbols[2].kind, SymbolKind::Data);
        assert_eq!(module.symbols[2].size, Some(0x10));
        assert!(module.symbols[3].location.storage.is_none());
    }

    #[test]
    fn mode_markers_do_not_need_sorted_input() {
        let module = NoCashSymImporter
            .import(
                b"08000210 irq_handler\n080001EC init_video\n08000210 .arm\n080001EC .thumb",
                &context(),
            )
            .unwrap();
        assert_eq!(module.symbols[0].location.exec_mode, ExecMode::Arm);
        assert_eq!(module.symbols[1].location.exec_mode, ExecMode::Thumb);
    }
}
