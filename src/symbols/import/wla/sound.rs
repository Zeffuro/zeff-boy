use super::*;

pub(crate) fn import_md_sound(
    file_name: &str,
    data: &[u8],
    ctx: &ImportContext,
) -> anyhow::Result<SymbolModule> {
    let text = std::str::from_utf8(data)?;
    anyhow::ensure!(
        file_name.to_ascii_lowercase().ends_with(".sym")
            && text.lines().any(|line| matches!(
                line.trim(),
                "[labels]" | "[symbols]" | "[source files v2]" | "[addr-to-line mapping v2]"
            )),
        "MD SOUND imports require an explicit WLA .sym file"
    );
    let mut module = WlaSymImporter.import(data, ctx)?;
    module.format = "WLA .sym (MD SOUND, read-only)".into();
    Ok(module)
}

pub(super) fn qualify(module: &mut SymbolModule) {
    module.symbols.retain(|symbol| {
        symbol.kind != SymbolKind::Section && symbol.location.bank.is_none_or(|bank| bank == 0)
    });
    module
        .source_lines
        .retain(|source| source.location.bank == Some(0));
    for symbol in &mut module.symbols {
        if symbol.location.cpu.is_some() {
            symbol.location.storage = None;
            symbol.location.bank = None;
            symbol.location.exec_mode = ExecMode::Z80;
        }
    }
    module
        .source_lines
        .sort_by_key(|source| source.location.cpu);
    for index in 0..module.source_lines.len() {
        let address = module.source_lines[index].location.cpu.unwrap().address;
        let end = module
            .source_lines
            .get(index + 1)
            .map_or(address + 1, |source| source.location.cpu.unwrap().address);
        let source = &mut module.source_lines[index];
        source.size = end.saturating_sub(address).clamp(1, 0x1_0000 - address);
        // Linker ROM offsets and banks do not describe the running SOUND mapping.
        source.location.storage = None;
        source.location.bank = None;
        source.location.exec_mode = ExecMode::Z80;
    }
    module.diagnostics.push("SOUND labels use logical space 1; nonzero banks and sections are ignored; storage mapping is unknown".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn md_sound_import_keeps_identity_space_and_unknown_storage() {
        let ctx = ImportContext {
            target: TargetInfo { system: System::Md },
            image: crate::symbols::ImageId(0),
            rom_region: crate::symbols::RegionId(0),
            cpu_space: crate::symbols::AddressSpaceId(1),
            source_name: None,
        };
        let data = b"[labels]\n00:0200 SoundEntry\n01:0200 AmbiguousBank\n00:10000 TooWide\n[sections]\n00010000 00:0200 0000 00000010 SoundCode\n[source files v2]\n0001:0002 12345678 sound.asm\n[addr-to-line mapping v2]\n00010000 00:0200 0200 0001:0002:0000002A\n00010002 00:0202 0202 0001:0002:0000002B";
        let module =
            crate::symbols::import::import_md_sound_symbols("sound.sym", data, &ctx).unwrap();
        assert_eq!(module.symbols.len(), 1);
        let symbol = &module.symbols[0];
        assert_eq!(symbol.name, "SoundEntry");
        assert_eq!(
            symbol.location.cpu.unwrap(),
            CpuLocation {
                space: ctx.cpu_space,
                address: 0x200
            }
        );
        assert_eq!(symbol.location.exec_mode, ExecMode::Z80);
        assert!(symbol.location.storage.is_none() && symbol.location.bank.is_none());
        assert_eq!(module.source_lines[0].size, 2);
        assert!(
            module
                .source_lines
                .iter()
                .all(|line| line.location.storage.is_none())
        );
        assert!(
            module
                .diagnostics
                .iter()
                .any(|line| line.contains("malformed WLA label"))
        );
        assert!(
            crate::symbols::import::import_md_sound_symbols("sound.sym", b"000200 T Main", &ctx)
                .is_err()
        );
        let primary = ImportContext {
            cpu_space: crate::symbols::AddressSpaceId(0),
            ..ctx
        };
        assert!(crate::symbols::import::import_symbols("main.sym", data, &primary).is_err());
        let main =
            crate::symbols::import::import_symbols("main.sym", b"000200 T MainEntry", &primary)
                .unwrap();
        assert_eq!(main.symbols[0].location.exec_mode, ExecMode::M68000);
        assert!(
            crate::symbols::import::import_md_sound_symbols("sound.sym", data, &primary).is_err()
        );
        let secondary_primary = ImportContext {
            cpu_space: crate::symbols::AddressSpaceId(1),
            ..primary
        };
        let main = crate::symbols::import::import_symbols(
            "main.sym",
            b"000200 T PrimarySpaceOne",
            &secondary_primary,
        )
        .unwrap();
        assert_eq!(main.symbols[0].location.exec_mode, ExecMode::M68000);
        assert_eq!(
            main.symbols[0].location.cpu.unwrap().space,
            secondary_primary.cpu_space
        );
        assert!(
            crate::symbols::import::import_symbols("main.sym", data, &secondary_primary).is_err()
        );
        let foreign = ImportContext {
            target: TargetInfo {
                system: System::Coleco,
            },
            ..secondary_primary
        };
        assert!(
            crate::symbols::import::import_md_sound_symbols("sound.sym", data, &foreign).is_err()
        );
    }
}
