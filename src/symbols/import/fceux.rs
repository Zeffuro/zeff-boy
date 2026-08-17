use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct FceuxNameListImporter;

impl SymbolImporter for FceuxNameListImporter {
    fn name(&self) -> &'static str {
        "FCEUX NameList"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Nes || !file_name.to_ascii_lowercase().ends_with(".nl") {
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
        ImportCapabilities::SYMBOLS.union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Nes,
            "FCEUX NameLists require an NES target"
        );
        let text = std::str::from_utf8(data)?;
        let bank = ctx.source_name.as_deref().and_then(bank_from_name);
        let mut module = SymbolModule::default();
        for (index, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((address, name, comment)) = parse_line(line) else {
                module.diagnostics.push(format!(
                    "line {}: ignored malformed NameList entry",
                    index + 1
                ));
                continue;
            };
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: name.to_owned(),
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(address),
                    }),
                    storage: None,
                    bank,
                    exec_mode: ExecMode::Mos6502,
                },
                value: None,
                size: None,
                kind: SymbolKind::Label,
                scope: SymbolScope::Global,
                provenance: Provenance {
                    kind: ProvenanceKind::ReverseEngineering,
                    source: ctx.source_name.clone(),
                },
                confidence: Confidence::High,
                comment: (!comment.is_empty()).then(|| comment.to_owned()),
            });
        }
        Ok(module)
    }
}

fn parse_line(line: &str) -> Option<(u16, &str, &str)> {
    let mut fields = line.splitn(3, '#');
    let address = fields.next()?.trim().strip_prefix('$')?;
    let name = fields.next()?.trim();
    let comment = fields.next().unwrap_or_default().trim();
    if address.len() != 4 || name.is_empty() {
        return None;
    }
    Some((u16::from_str_radix(address, 16).ok()?, name, comment))
}

fn bank_from_name(path: &str) -> Option<u32> {
    let name = std::path::Path::new(path).file_name()?.to_str()?;
    let stem = name.strip_suffix(".nl")?;
    stem.rsplit_once('.')?.1.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo {
                system: System::Nes,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("contra.nes.3.nl".into()),
        }
    }

    #[test]
    fn imports_fceux_entries_and_bank_names() {
        let module = FceuxNameListImporter
            .import(b"$8001#Start#Entry point\n$0018#FrameCounter#", &context())
            .unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].location.bank, Some(3));
        assert_eq!(module.symbols[0].comment.as_deref(), Some("Entry point"));
        assert_eq!(module.symbols[1].location.cpu.unwrap().address, 0x18);
    }
}
