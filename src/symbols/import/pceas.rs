use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct PceasSymImporter;

impl SymbolImporter for PceasSymImporter {
    fn name(&self) -> &'static str {
        "PCEAS .sym"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Pce || !file_name.to_ascii_lowercase().ends_with(".sym") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        let has_header = text.lines().map(code_part).any(is_legacy_header);
        let mut candidates = 0;
        let mut legacy = 0;
        let mut modern = 0;
        for line in text
            .lines()
            .map(code_part)
            .filter(|line| !line.is_empty() && !is_legacy_header(line) && !is_separator(line))
        {
            candidates += 1;
            match parse_line(line) {
                Some(ParsedLine { modern: true, .. }) => modern += 1,
                Some(_) => legacy += 1,
                None => {}
            }
            if candidates == 16 {
                break;
            }
        }
        let valid = legacy + modern;
        if valid == candidates && (modern != 0 || has_header && legacy != 0) {
            95
        } else if legacy != 0 && valid == candidates {
            70
        } else {
            0
        }
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS.union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Pce,
            "PCEAS symbols require a PC Engine target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule {
            symbols: Vec::with_capacity(text.lines().count()),
            ..SymbolModule::default()
        };
        for (index, raw_line) in text.lines().enumerate() {
            let line = code_part(raw_line);
            if line.is_empty() || is_legacy_header(line) || is_separator(line) {
                continue;
            }
            let Some(entry) = parse_line(line) else {
                module.diagnostics.push(format!(
                    "line {}: ignored malformed PCEAS symbol",
                    index + 1
                ));
                continue;
            };
            let scope = if entry.name.starts_with('.') {
                SymbolScope::Local
            } else {
                SymbolScope::Global
            };
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: entry.name,
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(entry.address),
                    }),
                    storage: None,
                    bank: Some(u32::from(entry.bank)),
                    exec_mode: ExecMode::Unknown,
                },
                value: None,
                size: None,
                kind: SymbolKind::Label,
                scope,
                provenance: Provenance {
                    kind: ProvenanceKind::Build,
                    source: ctx.source_name.clone(),
                },
                confidence: Confidence::Exact,
                comment: None,
            });
        }
        anyhow::ensure!(!module.symbols.is_empty(), "no PCEAS symbols found");
        Ok(module)
    }
}

struct ParsedLine {
    bank: u8,
    address: u16,
    name: String,
    modern: bool,
}

fn parse_line(line: &str) -> Option<ParsedLine> {
    parse_modern(line).or_else(|| parse_legacy(line))
}

fn parse_modern(line: &str) -> Option<ParsedLine> {
    let mut fields = line.split_whitespace();
    let location = fields.next()?;
    let size = fields.next()?;
    let source = fields.next()?;
    let name = fields.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return None;
    }
    let (bank, address) = parse_location(location)?;
    if size.is_empty() || !size.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut source_fields = source.split(':');
    for _ in 0..3 {
        let field = source_fields.next()?;
        if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
    }
    if source_fields.next().is_some() {
        return None;
    }
    Some(ParsedLine {
        bank,
        address,
        name,
        modern: true,
    })
}

fn parse_legacy(line: &str) -> Option<ParsedLine> {
    let mut fields = line.split_whitespace();
    let bank = fields.next()?;
    let address = fields.next()?;
    let name = fields.next()?;
    if fields.next().is_some() || name.is_empty() {
        return None;
    }
    Some(ParsedLine {
        bank: u8::from_str_radix(bank, 16).ok()?,
        address: u16::from_str_radix(address, 16).ok()?,
        name: name.to_owned(),
        modern: false,
    })
}

fn parse_location(value: &str) -> Option<(u8, u16)> {
    let (bank, address) = value.split_once(':')?;
    Some((
        u8::from_str_radix(bank, 16).ok()?,
        u16::from_str_radix(address, 16).ok()?,
    ))
}

fn code_part(line: &str) -> &str {
    line.trim_start_matches('\u{feff}')
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
}

fn is_legacy_header(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    let matched = fields
        .next()
        .is_some_and(|field| field.eq_ignore_ascii_case("bank"))
        && fields
            .next()
            .is_some_and(|field| field.eq_ignore_ascii_case("addr"))
        && fields
            .next()
            .is_some_and(|field| field.eq_ignore_ascii_case("label"));
    matched && fields.next().is_none()
}

fn is_separator(line: &str) -> bool {
    !line.is_empty()
        && line
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo {
                system: System::Pce,
            },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.sym".into()),
        }
    }
    #[test]
    fn probe_is_pce_only_and_recognizes_both_formats() {
        let importer = PceasSymImporter;
        let pce = TargetInfo {
            system: System::Pce,
        };
        let gb = TargetInfo { system: System::Gb };

        assert_eq!(
            importer.probe(
                "game.sym",
                b"Bank Addr Label\n---- ---- -----\n00 E000 reset",
                &pce
            ),
            95
        );
        assert_eq!(
            importer.probe("game.sym", b"00:E000 0010 0:42:3 reset", &pce),
            95
        );
        assert_eq!(
            importer.probe("game.sym", b"Bank Addr Label\n00 E000 reset", &gb),
            0
        );
    }

    #[test]
    fn imports_legacy_symbols_without_guessing_storage() {
        let module = PceasSymImporter
            .import(
                b"Bank\tAddr\tLabel\r\n----\t----\t-----\r\n00\tE000\treset\r\n01\tA123\t\tlocal_name ; note",
                &context(),
            )
            .unwrap();

        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].name, "reset");
        assert_eq!(module.symbols[0].location.bank, Some(0));
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0xE000);
        assert!(module.symbols[0].location.storage.is_none());
        assert_eq!(module.symbols[1].location.bank, Some(1));
        assert_eq!(module.symbols[1].location.cpu.unwrap().address, 0xA123);
    }

    #[test]
    fn imports_modern_symbols_without_guessing_metadata() {
        let module = PceasSymImporter
            .import(
                b"00:E000 0010 0:42:3 reset handler\n02:A123 0008 1:17:1 draw_sprite\n03:A123 0004 1:18:1 draw_sprite_alt",
                &context(),
            )
            .unwrap();

        assert_eq!(module.symbols.len(), 3);
        assert_eq!(module.symbols[0].name, "reset handler");
        assert!(module.symbols[0].size.is_none());
        assert_eq!(module.symbols[1].location.bank, Some(2));
        assert_eq!(module.symbols[2].location.bank, Some(3));
        assert!(
            module
                .symbols
                .iter()
                .all(|symbol| symbol.location.storage.is_none())
        );
    }

    #[test]
    fn reports_out_of_range_records_without_truncating() {
        let module = PceasSymImporter
            .import(b"100 E000 too_wide\n00 E000 reset", &context())
            .unwrap();

        assert_eq!(module.symbols.len(), 1);
        assert_eq!(module.diagnostics.len(), 1);
        assert_eq!(module.symbols[0].name, "reset");
    }
}
