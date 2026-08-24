use serde_json::Value;
use zeff_emu_common::system::System;

use super::{ImportCapabilities, ImportContext, SymbolImporter, SymbolModule, TargetInfo};
use crate::symbols::{
    Confidence, CpuLocation, ExecMode, Provenance, ProvenanceKind, SymbolId, SymbolKind,
    SymbolLocation, SymbolRecord, SymbolScope,
};

pub(crate) struct EtripatorLabelImporter;

impl SymbolImporter for EtripatorLabelImporter {
    fn name(&self) -> &'static str {
        "Etripator labels"
    }

    fn probe(&self, _file_name: &str, data: &[u8], target: &TargetInfo) -> u8 {
        if target.system != System::Pce {
            return 0;
        }
        let Ok(labels) = serde_json::from_slice::<Vec<Value>>(data) else {
            return 0;
        };
        u8::from(!labels.is_empty() && labels.iter().all(is_label)) * 95
    }

    fn capabilities(&self) -> ImportCapabilities {
        ImportCapabilities::SYMBOLS.union(ImportCapabilities::BANKED_ADDR)
    }

    fn import(&self, data: &[u8], ctx: &ImportContext) -> anyhow::Result<SymbolModule> {
        anyhow::ensure!(
            ctx.target.system == System::Pce,
            "Etripator labels require a PC Engine target"
        );
        let labels = serde_json::from_slice::<Vec<Value>>(data)?;
        let mut module = SymbolModule::default();
        for (index, label) in labels.iter().enumerate() {
            let Some((name, logical, page, comment)) = parse_label(label) else {
                module.diagnostics.push(format!(
                    "entry {}: ignored malformed Etripator label",
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
                        address: u64::from(logical),
                    }),
                    storage: None,
                    bank: Some(u32::from(page)),
                    exec_mode: ExecMode::Unknown,
                },
                value: None,
                size: None,
                kind: SymbolKind::Label,
                scope: if name.starts_with('.') {
                    SymbolScope::Local
                } else {
                    SymbolScope::Global
                },
                provenance: Provenance {
                    kind: ProvenanceKind::ReverseEngineering,
                    source: ctx.source_name.clone(),
                },
                confidence: Confidence::High,
                comment,
            });
        }
        Ok(module)
    }
}

fn is_label(value: &Value) -> bool {
    value.as_object().is_some_and(|label| {
        ["name", "logical", "page"]
            .into_iter()
            .all(|field| label.get(field).and_then(Value::as_str).is_some())
    })
}

fn parse_label(value: &Value) -> Option<(&str, u16, u8, Option<String>)> {
    let label = value.as_object()?;
    let name = label.get("name")?.as_str()?;
    let logical = u16::from_str_radix(label.get("logical")?.as_str()?, 16).ok()?;
    let page = u8::from_str_radix(label.get("page")?.as_str()?, 16).ok()?;
    let comment = match label.get("description") {
        None => None,
        Some(Value::String(text)) => Some(text.to_owned()),
        Some(Value::Array(lines)) => lines
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()
            .map(|lines| lines.join("\n")),
        _ => return None,
    };
    Some((name, logical, page, comment))
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
            source_name: Some("game.lbl".into()),
        }
    }

    #[test]
    fn imports_page_qualified_labels_without_storage_offsets() {
        let module = EtripatorLabelImporter
            .import(
                br#"[
                    {"name":"entry","logical":"e22a","page":"00","description":["reset","input: none"]},
                    {"name":".loop","logical":"f8a4","page":"f8","description":"repeat"}
                ]"#,
                &context(),
            )
            .unwrap();
        assert_eq!(module.symbols.len(), 2);
        assert_eq!(module.symbols[0].location.cpu.unwrap().address, 0xE22A);
        assert_eq!(module.symbols[0].location.bank, Some(0));
        assert!(module.symbols[0].location.storage.is_none());
        assert_eq!(
            module.symbols[0].comment.as_deref(),
            Some("reset\ninput: none")
        );
        assert_eq!(module.symbols[1].scope, SymbolScope::Local);
    }

    #[test]
    fn probe_is_pce_specific_and_requires_label_objects() {
        let importer = EtripatorLabelImporter;
        let labels = br#"[{"name":"entry","logical":"e000","page":"00"}]"#;
        assert_eq!(
            importer.probe(
                "labels.json",
                labels,
                &TargetInfo {
                    system: System::Pce
                }
            ),
            95
        );
        assert_eq!(
            importer.probe("labels.json", labels, &TargetInfo { system: System::Gb }),
            0
        );
        assert_eq!(
            importer.probe(
                "labels.json",
                br#"[{"name":"entry"}]"#,
                &TargetInfo {
                    system: System::Pce
                }
            ),
            0
        );
    }
}
