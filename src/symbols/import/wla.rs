use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SourceFile, SourceLine,
    StorageLocation, SymbolId, SymbolKind, SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct WlaSymImporter;

impl SymbolImporter for WlaSymImporter {
    fn name(&self) -> &'static str {
        "WLA .sym"
    }

    fn probe(&self, file_name: &str, data: &[u8], _target: &TargetInfo) -> u8 {
        if !file_name.to_ascii_lowercase().ends_with(".sym") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        let has_information = text.lines().any(|line| line.trim() == "[information]");
        let has_content = text.lines().any(|line| {
            matches!(
                line.trim(),
                "[labels]" | "[source files v2]" | "[addr-to-line mapping v2]"
            )
        });
        u8::from(has_information && has_content) * 90
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS
            .union(ImportCapabilities::SECTIONS)
            .union(ImportCapabilities::SOURCE_LINES)
            .union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        let text = std::str::from_utf8(data)?;
        let mut entries: Vec<Entry<'_>> = Vec::new();
        let mut sections = Vec::new();
        let mut source_files = Vec::new();
        let mut source_lines = Vec::new();
        let mut module = SymbolModule::default();
        let mut section = "";
        for (index, raw_line) in text.lines().enumerate() {
            let line = raw_line.split(';').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = &line[1..line.len() - 1];
                continue;
            }
            match section {
                "labels" | "symbols" => match parse_banked(line) {
                    Some((bank, address, name)) => {
                        entries.push((bank, address, name, SymbolKind::Label).into())
                    }
                    None => module
                        .diagnostics
                        .push(format!("line {}: ignored malformed WLA label", index + 1)),
                },
                "definitions" => match parse_definition(line) {
                    Some((value, name)) => entries
                        .push(Entry::from((0, 0, name, SymbolKind::Constant)).with_value(value)),
                    None => module.diagnostics.push(format!(
                        "line {}: ignored malformed WLA definition",
                        index + 1
                    )),
                },
                "sections" => match parse_section(line) {
                    Some(parsed) => sections.push(parsed),
                    None => module
                        .diagnostics
                        .push(format!("line {}: ignored malformed WLA section", index + 1)),
                },
                "source files v2" => match parse_source_file(line) {
                    Some(source) => source_files.push(source),
                    None => module.diagnostics.push(format!(
                        "line {}: ignored malformed WLA source file",
                        index + 1
                    )),
                },
                "addr-to-line mapping v2" => match parse_source_line(line) {
                    Some(source) => source_lines.push(source),
                    None => module.diagnostics.push(format!(
                        "line {}: ignored malformed WLA source line",
                        index + 1
                    )),
                },
                _ => {}
            }
        }

        for section in &sections {
            module.symbols.push(record(
                ctx,
                section.name,
                SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(section.cpu),
                    }),
                    storage: Some(StorageLocation {
                        image: ctx.image,
                        region: ctx.rom_region,
                        offset: u64::from(section.rom),
                    }),
                    bank: Some(section.bank),
                    exec_mode: exec_mode(ctx.target.system),
                },
                None,
                Some(u64::from(section.size)),
                SymbolKind::Section,
            ));
        }
        for entry in entries {
            let (bank, address, name, kind, value) = entry.split();
            if kind == SymbolKind::Constant {
                module.symbols.push(record(
                    ctx,
                    name,
                    unknown_location(),
                    Some(value),
                    None,
                    kind,
                ));
                continue;
            }
            let storage = resolve_storage(ctx, &sections, bank, address);
            module.symbols.push(record(
                ctx,
                name,
                SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(address),
                    }),
                    storage,
                    bank: Some(bank),
                    exec_mode: exec_mode(ctx.target.system),
                },
                None,
                None,
                kind,
            ));
        }
        source_lines.sort_by_key(|source| (source.bank, source.rom, source.cpu));
        for (index, source) in source_lines.iter().enumerate() {
            let Some(source_file) = source_files
                .iter()
                .position(|file| file.object == source.object && file.file == source.file)
            else {
                module.diagnostics.push(format!(
                    "ignored WLA source line {} with unknown source file {:04X}:{:04X}",
                    source.line, source.object, source.file
                ));
                continue;
            };
            let next_offset = source_lines
                .get(index + 1)
                .filter(|next| next.bank == source.bank && next.rom > source.rom)
                .map(|next| u64::from(next.rom - source.rom));
            let section_remaining = sections
                .iter()
                .find(|section| {
                    source.rom >= section.rom
                        && source.rom < section.rom.saturating_add(section.size)
                })
                .map(|section| {
                    u64::from(
                        section
                            .rom
                            .saturating_add(section.size)
                            .saturating_sub(source.rom),
                    )
                });
            let size = match (next_offset, section_remaining) {
                (Some(next), Some(remaining)) => next.min(remaining),
                (Some(next), None) => next,
                (None, Some(remaining)) => remaining,
                (None, None) => 1,
            }
            .max(1);
            module.source_lines.push(SourceLine {
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address: u64::from(source.cpu),
                    }),
                    storage: Some(StorageLocation {
                        image: ctx.image,
                        region: ctx.rom_region,
                        offset: u64::from(source.rom),
                    }),
                    bank: Some(source.bank),
                    exec_mode: exec_mode(ctx.target.system),
                },
                size,
                source_file,
                line: source.line,
            });
        }
        module.source_files = source_files
            .into_iter()
            .map(|source| SourceFile {
                path: source.path.to_owned(),
                crc32: Some(source.crc32),
            })
            .collect();
        Ok(module)
    }
}

struct Entry<'a> {
    bank: u32,
    address: u16,
    name: &'a str,
    kind: SymbolKind,
    value: Option<u64>,
}

impl<'a> Entry<'a> {
    fn with_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    fn split(self) -> (u32, u16, &'a str, SymbolKind, u64) {
        (
            self.bank,
            self.address,
            self.name,
            self.kind,
            self.value.unwrap_or_default(),
        )
    }
}

impl<'a> From<(u32, u16, &'a str, SymbolKind)> for Entry<'a> {
    fn from((bank, address, name, kind): (u32, u16, &'a str, SymbolKind)) -> Self {
        Self {
            bank,
            address,
            name,
            kind,
            value: None,
        }
    }
}

struct Section<'a> {
    rom: u32,
    bank: u32,
    cpu: u16,
    size: u32,
    name: &'a str,
}

struct SourceFileEntry<'a> {
    object: u16,
    file: u16,
    crc32: u32,
    path: &'a str,
}

struct SourceLineEntry {
    rom: u32,
    bank: u32,
    cpu: u16,
    object: u16,
    file: u16,
    line: u32,
}

fn parse_banked(line: &str) -> Option<(u32, u16, &str)> {
    let (location, name) = line.split_once(char::is_whitespace)?;
    let (bank, address) = location.split_once(':')?;
    Some((
        u32::from_str_radix(bank, 16).ok()?,
        u16::from_str_radix(address, 16).ok()?,
        name.trim(),
    ))
}

fn parse_definition(line: &str) -> Option<(u64, &str)> {
    let (value, name) = line.split_once(char::is_whitespace)?;
    Some((u64::from_str_radix(value, 16).ok()?, name.trim()))
}

fn parse_section(line: &str) -> Option<Section<'_>> {
    let mut fields = line.split_whitespace();
    let rom = u32::from_str_radix(fields.next()?, 16).ok()?;
    let (bank, cpu) = parse_banked_pair(fields.next()?)?;
    fields.next()?;
    let size = u32::from_str_radix(fields.next()?, 16).ok()?;
    let name = fields.next()?;
    Some(Section {
        rom,
        bank,
        cpu,
        size,
        name,
    })
}

fn parse_banked_pair(value: &str) -> Option<(u32, u16)> {
    let (bank, address) = value.split_once(':')?;
    Some((
        u32::from_str_radix(bank, 16).ok()?,
        u16::from_str_radix(address, 16).ok()?,
    ))
}

fn parse_source_file(line: &str) -> Option<SourceFileEntry<'_>> {
    let mut fields = line
        .splitn(3, char::is_whitespace)
        .filter(|field| !field.is_empty());
    let (object, file) = parse_u16_pair(fields.next()?)?;
    let crc32 = u32::from_str_radix(fields.next()?, 16).ok()?;
    let path = fields.next()?.trim();
    (!path.is_empty()).then_some(SourceFileEntry {
        object,
        file,
        crc32,
        path,
    })
}

fn parse_source_line(line: &str) -> Option<SourceLineEntry> {
    let mut fields = line.split_whitespace();
    let rom = u32::from_str_radix(fields.next()?, 16).ok()?;
    let (bank, _) = parse_banked_pair(fields.next()?)?;
    let cpu = u16::from_str_radix(fields.next()?, 16).ok()?;
    let (object, file, line) = parse_source_triplet(fields.next()?)?;
    fields.next().is_none().then_some(SourceLineEntry {
        rom,
        bank,
        cpu,
        object,
        file,
        line,
    })
}

fn parse_u16_pair(value: &str) -> Option<(u16, u16)> {
    let (first, second) = value.split_once(':')?;
    Some((
        u16::from_str_radix(first, 16).ok()?,
        u16::from_str_radix(second, 16).ok()?,
    ))
}

fn parse_source_triplet(value: &str) -> Option<(u16, u16, u32)> {
    let mut fields = value.split(':');
    let object = u16::from_str_radix(fields.next()?, 16).ok()?;
    let file = u16::from_str_radix(fields.next()?, 16).ok()?;
    let line = u32::from_str_radix(fields.next()?, 16).ok()?;
    fields.next().is_none().then_some((object, file, line))
}

fn resolve_storage(
    ctx: &ImportContext,
    sections: &[Section<'_>],
    bank: u32,
    address: u16,
) -> Option<StorageLocation> {
    let section = sections.iter().find(|section| {
        section.bank == bank
            && address >= section.cpu
            && u32::from(address - section.cpu) < section.size
    });
    section.map(|section| StorageLocation {
        image: ctx.image,
        region: ctx.rom_region,
        offset: u64::from(section.rom) + u64::from(address - section.cpu),
    })
}

fn record(
    ctx: &ImportContext,
    name: &str,
    location: SymbolLocation,
    value: Option<u64>,
    size: Option<u64>,
    kind: SymbolKind,
) -> SymbolRecord {
    SymbolRecord {
        id: SymbolId(0),
        name: name.to_owned(),
        location,
        value,
        size,
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
    }
}

fn unknown_location() -> SymbolLocation {
    SymbolLocation {
        cpu: None,
        storage: None,
        bank: None,
        exec_mode: ExecMode::Unknown,
    }
}

fn exec_mode(system: System) -> ExecMode {
    match system {
        System::Gb => ExecMode::Sm83,
        System::Gba => ExecMode::Arm,
        System::Nes => ExecMode::Mos6502,
        System::Pce => ExecMode::Unknown,
        System::Ws => ExecMode::V30,
        System::Sms | System::Gg | System::Sg => ExecMode::Z80,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context(system: System) -> ImportContext {
        ImportContext {
            target: TargetInfo { system },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.sym".into()),
        }
    }

    #[test]
    fn imports_labels_definitions_and_sections() {
        let module = WlaSymImporter.import(
            b"[information]\nversion 3\nwlasymbol true\n[labels]\n02:4560 Update\n[definitions]\n00000020 SIZE\n[sections]\n00008560 02:4560 0000 00000010 PlayerCode",
            &context(System::Gb),
        ).unwrap();
        assert_eq!(module.symbols.len(), 3);
        assert!(
            module
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Constant)
        );
        assert!(module.symbols.iter().any(|symbol| {
            symbol.name == "PlayerCode"
                && symbol
                    .location
                    .storage
                    .is_some_and(|location| location.offset == 0x8560)
        }));
        assert!(module.symbols.iter().any(|symbol| {
            symbol.name == "Update"
                && symbol
                    .location
                    .storage
                    .is_some_and(|location| location.offset == 0x8560)
        }));
    }

    #[test]
    fn preserves_non_gb_cpu_labels_without_guessing_storage() {
        let module = WlaSymImporter
            .import(
                b"[information]\nversion 3\n[labels]\n03:8000 Start",
                &context(System::Nes),
            )
            .unwrap();
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0x8000);
        assert!(module.symbols[0].location.storage.is_none());
        assert_eq!(module.symbols[0].location.exec_mode, ExecMode::Mos6502);
    }

    #[test]
    fn imports_wla_v2_source_lines() {
        let module = WlaSymImporter
            .import(
                b"[information]\nversion 3\n[source files v2]\n0001:0002 12345678 src/player.asm\n[addr-to-line mapping v2]\n00008560 02:4560 4560 0001:0002:0000002A",
                &context(System::Gb),
            )
            .unwrap();
        assert_eq!(module.source_files.len(), 1);
        assert_eq!(module.source_files[0].path, "src/player.asm");
        assert_eq!(module.source_files[0].crc32, Some(0x1234_5678));
        assert_eq!(module.source_lines.len(), 1);
        assert_eq!(module.source_lines[0].line, 42);
        assert_eq!(module.source_lines[0].size, 1);
        assert_eq!(
            module.source_lines[0].location.storage.unwrap().offset,
            0x8560
        );
    }

    #[test]
    fn source_lines_cover_until_the_next_mapping() {
        let module = WlaSymImporter
            .import(
                b"[information]\nversion 3\n[source files v2]\n0001:0002 12345678 src/player.asm\n[addr-to-line mapping v2]\n00008560 02:4560 4560 0001:0002:0000002A\n00008564 02:4564 4564 0001:0002:0000002B",
                &context(System::Gb),
            )
            .unwrap();

        assert_eq!(module.source_lines[0].size, 4);
        assert_eq!(module.source_lines[1].size, 1);
    }

    #[test]
    fn imports_external_wla_fixture_when_configured() {
        let Ok(path) = std::env::var("ZEFF_TEST_WLA_SYM") else {
            return;
        };
        let data = std::fs::read(path).unwrap();
        let module = WlaSymImporter.import(&data, &context(System::Sms)).unwrap();
        assert!(module.symbols.len() > 1_000);
        assert!(module.diagnostics.is_empty());
    }
}
