use super::UserSymbolFile;
use crate::symbols::md::{normalize_mode, validate_cpu_range};

pub(super) fn validate_file(file: &mut UserSymbolFile) -> anyhow::Result<()> {
    for symbol in &mut file.symbols {
        normalize_mode(&mut symbol.exec_mode)?;
        if let Some(address) = symbol.cpu_address {
            validate_cpu_range(address, symbol.size)?;
        }
    }
    for segment in &mut file.segments {
        normalize_mode(&mut segment.exec_mode)?;
        if let Some(address) = segment.linked_cpu_address {
            validate_cpu_range(address, Some(segment.size))?;
        }
    }
    for instance in &file.load_instances {
        if let Some(segment) = file
            .segments
            .iter()
            .find(|segment| segment.id == instance.segment_id)
        {
            validate_cpu_range(instance.cpu_address, Some(segment.size))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::symbols::ExecMode;
    use crate::symbols::identity::RomIdentity;
    use crate::symbols::user::{load_user_symbols, load_zdbg_symbols};
    use std::sync::atomic::{AtomicU64, Ordering};
    use zeff_emu_common::system::System;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "zeff-md-symbols-{}-{}.json",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn file() -> serde_json::Value {
        serde_json::json!({
            "format": "zeff-debug-symbols", "version": 2, "system": "md",
            "rom_sha256": "00".repeat(32),
            "symbols": [{"name": "Entry", "cpu_address": 0x123400, "size": 8,
                "exec_mode": "unknown", "kind": "function", "scope": "global"}],
            "segments": [{"id": 1, "name": "Copied", "rom_offset": 0x200, "size": 8,
                "linked_cpu_address": 0x123400, "exec_mode": "unknown"}],
            "load_instances": [{"id": 1, "segment_id": 1, "cpu_address": 0xFF8040,
                "generation": 1, "created_cycle": 0}]
        })
    }

    #[test]
    fn md_zdbg_preserves_explicit_storage_and_normalizes_modes() {
        let path = temp_path();
        std::fs::write(&path, serde_json::to_vec(&file()).unwrap()).unwrap();
        let loaded = load_zdbg_symbols(
            &path,
            RomIdentity {
                system: System::Md,
                sha256: [0; 32],
            },
        )
        .unwrap();
        assert_eq!(loaded.symbols[0].location.cpu.unwrap().address, 0x123400);
        assert_eq!(loaded.symbols[0].location.exec_mode, ExecMode::M68000);
        assert!(loaded.symbols[0].location.storage.is_none());
        assert_eq!(loaded.segments[0].storage.start.offset, 0x200);
        assert_eq!(loaded.segments[0].exec_mode, ExecMode::M68000);
        assert_eq!(loaded.load_instances[0].runtime_base.address, 0xFF8040);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn md_generic_files_reject_invalid_cpu_ranges_and_foreign_modes() {
        let path = temp_path();
        for field in [
            "symbol",
            "symbol_range",
            "mode",
            "segment",
            "segment_range",
            "segment_mode",
            "instance",
            "instance_range",
        ] {
            let mut data = file();
            match field {
                "symbol" => data["symbols"][0]["cpu_address"] = 0x1_000000_u64.into(),
                "symbol_range" => data["symbols"][0]["cpu_address"] = 0xFFFFFF_u64.into(),
                "mode" => data["symbols"][0]["exec_mode"] = "arm".into(),
                "segment" => data["segments"][0]["linked_cpu_address"] = 0x1_000000_u64.into(),
                "segment_range" => data["segments"][0]["linked_cpu_address"] = 0xFFFFFF_u64.into(),
                "segment_mode" => data["segments"][0]["exec_mode"] = "sm83".into(),
                "instance" => data["load_instances"][0]["cpu_address"] = 0x1_000000_u64.into(),
                "instance_range" => data["load_instances"][0]["cpu_address"] = 0xFFFFFF_u64.into(),
                _ => unreachable!(),
            }
            std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let identity = RomIdentity {
                system: System::Md,
                sha256: [0; 32],
            };
            assert!(load_zdbg_symbols(&path, identity).is_err(), "{field}");
            data["version"] = 1.into();
            std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            assert!(load_user_symbols(&path, identity).is_err(), "{field}");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn md_generic_validation_does_not_limit_gba_cpu_addresses_or_modes() {
        let path = temp_path();
        let mut data = file();
        data["system"] = "gba".into();
        data["symbols"][0]["cpu_address"] = 0x08000200_u64.into();
        data["symbols"][0]["exec_mode"] = "arm".into();
        data["segments"][0]["linked_cpu_address"] = 0x08000200_u64.into();
        data["segments"][0]["exec_mode"] = "arm".into();
        data["load_instances"][0]["cpu_address"] = 0x02000000_u64.into();
        std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let loaded = load_zdbg_symbols(
            &path,
            RomIdentity {
                system: System::Gba,
                sha256: [0; 32],
            },
        )
        .unwrap();
        assert_eq!(loaded.symbols[0].location.exec_mode, ExecMode::Arm);
        assert_eq!(loaded.symbols[0].location.cpu.unwrap().address, 0x08000200);
        std::fs::remove_file(path).unwrap();
    }
}
