use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope,
};

pub(super) struct MdNmImporter;

impl SymbolImporter for MdNmImporter {
    fn name(&self) -> &'static str {
        "GNU nm M68000 .sym"
    }

    fn probe(&self, file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Md || !file_name.to_ascii_lowercase().ends_with(".sym") {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(data) else {
            return 0;
        };
        u8::from(text.lines().any(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            matches!(fields.as_slice(), ["U" | "w" | "v", _])
                || matches!(fields.as_slice(), [_, kind, _] | [_, _, kind, _] if symbol_kind(kind).is_some())
        })) * 100
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Md,
            "M68000 nm symbols require a Mega Drive target"
        );
        let text = std::str::from_utf8(data)?;
        let mut module = SymbolModule::default();
        for (index, line) in text.lines().enumerate() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.is_empty() || matches!(fields.as_slice(), ["U" | "w" | "v", _]) {
                continue;
            }
            let (address, size, binding, name) = match fields.as_slice() {
                [address, binding, name] => (*address, None, *binding, *name),
                [address, size, binding, name] => (*address, Some(*size), *binding, *name),
                _ => anyhow::bail!(
                    "line {}: expected GNU nm address [size] type name",
                    index + 1
                ),
            };
            let kind = symbol_kind(binding).ok_or_else(|| {
                anyhow::anyhow!("line {}: unsupported GNU nm symbol type", index + 1)
            })?;
            let address = parse_hex(address)?;
            let size = size.map(parse_hex).transpose()?.filter(|size| *size != 0);
            anyhow::ensure!(
                address <= 0xFF_FFFF && size.is_none_or(|size| size - 1 <= 0xFF_FFFF - address),
                "line {}: symbol exceeds the 24-bit Mega Drive CPU address space",
                index + 1
            );
            anyhow::ensure!(
                name.len() <= 256 && !name.chars().any(char::is_control),
                "line {}: invalid symbol name",
                index + 1
            );
            module.symbols.push(SymbolRecord {
                id: SymbolId(0),
                name: name.to_owned(),
                location: SymbolLocation {
                    cpu: Some(CpuLocation {
                        space: ctx.cpu_space,
                        address,
                    }),
                    storage: None,
                    bank: None,
                    exec_mode: ExecMode::M68000,
                },
                value: (kind == SymbolKind::Constant).then_some(address),
                size,
                kind,
                scope: if binding.as_bytes()[0].is_ascii_lowercase() {
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
        anyhow::ensure!(
            !module.symbols.is_empty(),
            "no defined GNU nm M68000 symbols found"
        );
        Ok(module)
    }
}

fn parse_hex(value: &str) -> anyhow::Result<u64> {
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid GNU nm hexadecimal address or size"
    );
    Ok(u64::from_str_radix(value, 16)?)
}

fn symbol_kind(value: &str) -> Option<SymbolKind> {
    match value {
        "T" | "t" | "W" | "w" => Some(SymbolKind::Function),
        "D" | "d" | "B" | "b" | "R" | "r" | "S" | "s" | "G" | "g" | "V" | "v" => {
            Some(SymbolKind::Data)
        }
        "A" | "a" => Some(SymbolKind::Constant),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::import::import_symbols;
    use crate::symbols::{AddressSpaceId, ImageId, RegionId};

    fn context() -> ImportContext {
        ImportContext {
            target: TargetInfo { system: System::Md },
            image: ImageId(0),
            rom_region: RegionId(0),
            cpu_space: AddressSpaceId(0),
            source_name: Some("game.sym".into()),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_nm_imports_full_cpu_addresses_without_rom_mapping() {
        let text = format!(
            "{}{}",
            " \t\n".repeat(16),
            "00010200 00000008 T Entry\n00FF8040 00000002 b Counter\n00C00004 A VideoControl\n         U external\n         w optional"
        );
        let module = import_symbols("game.SYM", text.as_bytes(), &context()).unwrap();
        assert_eq!(module.format, "GNU nm M68000 .sym");
        assert_eq!(module.symbols.len(), 3);
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0x10200);
        assert_eq!(module.symbols[0].kind, SymbolKind::Function);
        assert_eq!(module.symbols[0].size, Some(8));
        assert_eq!(module.symbols[1].location.cpu.unwrap().address, 0xFF8040);
        assert_eq!(module.symbols[1].scope, SymbolScope::Local);
        assert_eq!(module.symbols[2].value, Some(0xC00004));
        for symbol in module.symbols {
            assert_eq!(symbol.location.exec_mode, ExecMode::M68000);
            assert!(symbol.location.storage.is_none());
            assert!(symbol.location.bank.is_none());
        }
        let mut many_undefined = "         U external\n".repeat(20);
        many_undefined.push_str("00123400 T AfterUndefined\n");
        let module = import_symbols("game.sym", many_undefined.as_bytes(), &context()).unwrap();
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0x123400);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_nm_rejects_invalid_ranges_and_mixed_malformed_records_atomically() {
        for invalid in [
            "01000000 T Wrapped",
            "FFFF8040 D SignExtended",
            "00FFFFFF 2 D Overflow",
            "FFFFFFFFFFFFFFFF T Wide",
            "00000201 -1 T BadSize",
            "000200 Q Unknown",
            "00:0200 Entry",
            "000200 Entry",
            "000200 T Name With Spaces",
        ] {
            let data = format!("00000200 T Valid\n{invalid}");
            assert!(
                import_symbols("game.sym", data.as_bytes(), &context()).is_err(),
                "{invalid}"
            );
        }
        let boundary = import_symbols("game.sym", b"00FFFFFF 1 D LastByte", &context()).unwrap();
        assert_eq!(boundary.symbols[0].location.cpu.unwrap().address, 0xFFFFFF);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_nm_registry_rejects_other_dialects_and_targets() {
        for (name, bytes) in [
            ("game.sym", b"08000200 g 00000008 ArmEntry".as_slice()),
            (
                "game.sym",
                b"[information]\n[labels]\n00:0200 Entry".as_slice(),
            ),
            ("game.elf", b"\x7FELF".as_slice()),
            ("game.map", b"00000200 T Entry".as_slice()),
            ("game.sym", b"         U unresolved".as_slice()),
        ] {
            assert!(import_symbols(name, bytes, &context()).is_err(), "{name}");
        }
        let mut foreign = context();
        foreign.target.system = System::Gba;
        assert_eq!(
            MdNmImporter.probe("game.sym", b"00000200 T Entry", &foreign.target),
            0
        );
        assert!(MdNmImporter.import(b"00000200 T Entry", &foreign).is_err());
    }
}
