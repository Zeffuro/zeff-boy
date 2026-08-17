use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct Ca65DbgImporter;

impl SymbolImporter for Ca65DbgImporter {
    fn name(&self) -> &'static str {
        "cc65 .dbg"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Nes || !file_name.to_ascii_lowercase().ends_with(".dbg") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        u8::from(text.contains("version\t") && text.contains("sym\t")) * 100
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Nes,
            "cc65 debug symbols require an NES target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule::default();
        for (index, line) in text.lines().enumerate() {
            let Some(entry) = line.strip_prefix("sym\t").and_then(parse_symbol) else {
                continue;
            };
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: entry.name,
                location: SymbolLocation {
                    cpu: (entry.kind == SymbolKind::Label)
                        .then_some(entry.value)
                        .flatten()
                        .map(|address| CpuLocation {
                            space: ctx.cpu_space,
                            address,
                        }),
                    storage: None,
                    bank: None,
                    exec_mode: ExecMode::Mos6502,
                },
                value: (entry.kind == SymbolKind::Constant)
                    .then_some(entry.value)
                    .flatten(),
                size: entry.size,
                kind: entry.kind,
                scope: SymbolScope::Global,
                provenance: Provenance {
                    kind: ProvenanceKind::DebugFormat,
                    source: ctx.source_name.clone(),
                },
                confidence: Confidence::Exact,
                comment: None,
            });
            if module
                .symbols
                .last()
                .is_some_and(|symbol| symbol.name.is_empty())
            {
                module
                    .diagnostics
                    .push(format!("line {}: ignored empty symbol", index + 1));
                module.symbols.pop();
            }
        }
        Ok(module)
    }
}

struct ParsedSymbol {
    name: String,
    value: Option<u64>,
    size: Option<u64>,
    kind: SymbolKind,
}

fn parse_symbol(line: &str) -> Option<ParsedSymbol> {
    let mut name = None;
    let mut value = None;
    let mut size = None;
    let mut kind = None;
    for field in line.split(',') {
        let (key, value_text) = field.split_once('=')?;
        match key {
            "name" => name = Some(value_text.trim_matches('"').to_owned()),
            "val" => value = parse_hex(value_text),
            "size" => size = value_text.parse().ok(),
            "type" => {
                kind = match value_text {
                    "lab" => Some(SymbolKind::Label),
                    "equ" => Some(SymbolKind::Constant),
                    _ => return None,
                }
            }
            _ => {}
        }
    }
    Some(ParsedSymbol {
        name: name?,
        value,
        size,
        kind: kind?,
    })
}

fn parse_hex(value: &str) -> Option<u64> {
    value
        .strip_prefix("0x")
        .and_then(|value| u64::from_str_radix(value, 16).ok())
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
            source_name: Some("game.fds.dbg".into()),
        }
    }

    #[test]
    fn imports_labels_and_constants() {
        let module = Ca65DbgImporter.import(
            b"version\tmajor=2,minor=0\nsym\tid=0,name=\"Start\",size=3,val=0xC000,type=lab\nsym\tid=1,name=\"Count\",val=0x0010,type=equ",
            &context(),
        ).unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0xC000);
        assert_eq!(module.symbols[0].size, Some(3));
        assert_eq!(module.symbols[1].kind, SymbolKind::Constant);
        assert_eq!(module.symbols[1].value, Some(0x10));
    }

    #[test]
    fn imports_external_debug_fixture_when_configured() {
        let Ok(path) = std::env::var("ZEFF_TEST_CA65_DBG") else {
            return;
        };
        let data = std::fs::read(path).unwrap();
        let module = Ca65DbgImporter.import(&data, &context()).unwrap();
        assert!(module.symbols.len() > 1_000);
    }
}
