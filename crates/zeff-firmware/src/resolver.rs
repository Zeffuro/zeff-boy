use crate::catalog::{
    FallbackKind, FirmwareDependency, FirmwareId, FirmwareRequest, FirmwareSpec, FirmwareVariantId,
    RequirementLevel,
};
use crate::manifest::{FirmwareSelectionManifest, ResolvedFirmware};
use crate::store::{FirmwareInventory, FirmwareInventoryEntry, ValidationStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareCandidate {
    pub firmware_id: FirmwareId,
    pub variant_id: FirmwareVariantId,
    pub original_filename: Option<String>,
    pub sha256: [u8; 32],
    pub known_good: bool,
}

#[derive(Clone, Debug)]
pub enum FirmwareSelection {
    External(ResolvedFirmware),
    Fallback(FirmwareSelectionManifest),
    Missing(ResolveFailure),
}

impl FirmwareSelection {
    pub fn manifest(&self) -> Option<FirmwareSelectionManifest> {
        match self {
            Self::External(firmware) => Some(firmware.selection_manifest()),
            Self::Fallback(manifest) => Some(manifest.clone()),
            Self::Missing(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveFailure {
    UnknownFirmwareId(FirmwareId),
    MissingRequired {
        firmware_id: FirmwareId,
        requirement: RequirementLevel,
    },
}

#[derive(Clone, Debug)]
pub struct FirmwareResolutionEntry {
    pub request: FirmwareRequest,
    pub selection: FirmwareSelection,
    pub candidates: Vec<FirmwareCandidate>,
}

#[derive(Clone, Debug, Default)]
pub struct FirmwareResolution {
    pub entries: Vec<FirmwareResolutionEntry>,
}

impl FirmwareResolution {
    pub fn has_blocking_failure(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.selection, FirmwareSelection::Missing(_)))
    }

    pub fn manifests(&self) -> Vec<FirmwareSelectionManifest> {
        self.entries
            .iter()
            .filter_map(|entry| entry.selection.manifest())
            .collect()
    }
}

pub struct FirmwareResolver<'a> {
    catalog: &'a [FirmwareSpec],
    inventory: &'a FirmwareInventory,
}

impl<'a> FirmwareResolver<'a> {
    pub fn new(catalog: &'a [FirmwareSpec], inventory: &'a FirmwareInventory) -> Self {
        Self { catalog, inventory }
    }

    pub fn resolve(&self, plan: &[FirmwareRequest]) -> FirmwareResolution {
        FirmwareResolution {
            entries: plan
                .iter()
                .cloned()
                .map(|request| self.resolve_request(request))
                .collect(),
        }
    }

    fn resolve_request(&self, request: FirmwareRequest) -> FirmwareResolutionEntry {
        let Some(spec) = self
            .catalog
            .iter()
            .find(|spec| spec.id == request.id.as_ref())
        else {
            return FirmwareResolutionEntry {
                request: request.clone(),
                selection: FirmwareSelection::Missing(ResolveFailure::UnknownFirmwareId(
                    request.id.clone(),
                )),
                candidates: Vec::new(),
            };
        };

        let mut candidates = self.candidates_for_request(spec, &request);
        candidates.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));

        let diagnostic_candidates = candidates
            .iter()
            .map(|candidate| candidate.public_candidate(spec))
            .collect();

        let selection = candidates
            .into_iter()
            .find(|candidate| candidate.known_good)
            .map(|candidate| candidate.resolved_firmware(spec))
            .map(FirmwareSelection::External)
            .or_else(|| fallback_selection(&request))
            .unwrap_or_else(|| {
                FirmwareSelection::Missing(ResolveFailure::MissingRequired {
                    firmware_id: request.id.clone(),
                    requirement: request.requirement,
                })
            });

        FirmwareResolutionEntry {
            request,
            selection,
            candidates: diagnostic_candidates,
        }
    }

    fn candidates_for_request(
        &'a self,
        spec: &'a FirmwareSpec,
        request: &FirmwareRequest,
    ) -> Vec<RankedCandidate<'a>> {
        let mut out = Vec::new();

        for (variant_index, variant) in spec.variants.iter().enumerate() {
            if !variant_matches_request(variant, request) {
                continue;
            }

            for entry in self.inventory.entries() {
                if !variant.size.matches(entry.bytes.len() as u64) {
                    continue;
                }

                let known_good = matches!(
                    &entry.validation,
                    ValidationStatus::KnownGood {
                        spec_id,
                        variant_id
                    } if spec_id == spec.id && variant_id == variant.id
                );
                let plausible = known_good
                    || matches!(
                        &entry.validation,
                        ValidationStatus::UnknownHash {
                            spec_id,
                            plausible_variant_ids
                        } if spec_id == spec.id
                            && plausible_variant_ids.iter().any(|id| id == variant.id)
                    );

                if !plausible {
                    continue;
                }

                out.push(RankedCandidate {
                    entry,
                    variant_id: variant.id,
                    known_good,
                    sort_key: CandidateSortKey {
                        unknown_penalty: u8::from(!known_good),
                        preferred_penalty: u8::from(
                            request
                                .preferred_variant
                                .as_ref()
                                .is_some_and(|preferred| preferred.as_ref() != variant.id),
                        ),
                        region_penalty: region_penalty(request.region.as_deref(), variant.region),
                        model_penalty: model_penalty(request.model.as_deref(), variant.model),
                        variant_index,
                    },
                });
            }
        }

        out
    }
}

#[derive(Clone, Debug)]
struct RankedCandidate<'a> {
    entry: &'a FirmwareInventoryEntry,
    variant_id: &'static str,
    known_good: bool,
    sort_key: CandidateSortKey,
}

impl RankedCandidate<'_> {
    fn public_candidate(&self, spec: &FirmwareSpec) -> FirmwareCandidate {
        FirmwareCandidate {
            firmware_id: FirmwareId::from(spec.id),
            variant_id: FirmwareVariantId::from(self.variant_id),
            original_filename: self.entry.original_filename.clone(),
            sha256: self.entry.digests.sha256,
            known_good: self.known_good,
        }
    }

    fn resolved_firmware(self, spec: &FirmwareSpec) -> ResolvedFirmware {
        ResolvedFirmware {
            id: FirmwareId::from(spec.id),
            variant: Some(FirmwareVariantId::from(self.variant_id)),
            bytes: self.entry.bytes.clone(),
            sha256: self.entry.digests.sha256,
            original_filename: self.entry.original_filename.clone(),
            validation: self.entry.validation.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateSortKey {
    unknown_penalty: u8,
    preferred_penalty: u8,
    region_penalty: u8,
    model_penalty: u8,
    variant_index: usize,
}

fn variant_matches_request(
    variant: &crate::catalog::FirmwareVariantSpec,
    request: &FirmwareRequest,
) -> bool {
    if region_penalty(request.region.as_deref(), variant.region) >= 2 {
        return false;
    }
    if model_penalty(request.model.as_deref(), variant.model) >= 2 {
        return false;
    }
    true
}

fn region_penalty(requested: Option<&str>, variant_region: &str) -> u8 {
    match requested {
        None => 1,
        Some(requested)
            if requested.eq_ignore_ascii_case(variant_region) || variant_region == "any" =>
        {
            0
        }
        Some(_) => 2,
    }
}

fn model_penalty(requested: Option<&str>, variant_model: Option<&str>) -> u8 {
    match (requested, variant_model) {
        (None, _) => 1,
        (Some(requested), Some(model)) if requested.eq_ignore_ascii_case(model) => 0,
        (Some(_), None) => 1,
        (Some(_), Some(_)) => 2,
    }
}

fn fallback_selection(request: &FirmwareRequest) -> Option<FirmwareSelection> {
    match &request.fallback {
        FallbackKind::None => None,
        FallbackKind::SkipBoot {
            compatibility_version,
        } => Some(FirmwareSelection::Fallback(
            FirmwareSelectionManifest::Skipped {
                firmware_id: request.id.clone(),
                compatibility_version: *compatibility_version,
            },
        )),
        FallbackKind::Hle {
            implementation,
            compatibility_version,
        } => Some(FirmwareSelection::Fallback(
            FirmwareSelectionManifest::Hle {
                firmware_id: request.id.clone(),
                implementation: implementation.clone(),
                compatibility_version: *compatibility_version,
            },
        )),
        FallbackKind::BuiltinOpenSource {
            implementation,
            compatibility_version,
            sha256,
        } => Some(FirmwareSelection::Fallback(
            FirmwareSelectionManifest::BuiltinOpenSource {
                firmware_id: request.id.clone(),
                implementation: implementation.clone(),
                compatibility_version: *compatibility_version,
                sha256: *sha256,
            },
        )),
    }
}

#[allow(dead_code)]
fn _assert_dependency_is_public(_: FirmwareDependency) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FirmwareVariantSpec, KnownHashes, SizeRule};

    const TEST_VARIANTS: &[FirmwareVariantSpec] = &[
        FirmwareVariantSpec {
            id: "test.firmware.usa",
            display_name: "Test Firmware USA",
            region: "usa",
            model: None,
            filenames: &["usa.bin"],
            size: SizeRule::Exact(4),
            hashes: KnownHashes {
                md5: Some("md5-usa"),
                sha1: None,
                sha256: None,
            },
        },
        FirmwareVariantSpec {
            id: "test.firmware.japan",
            display_name: "Test Firmware Japan",
            region: "japan",
            model: None,
            filenames: &["japan.bin"],
            size: SizeRule::Exact(4),
            hashes: KnownHashes {
                md5: Some("md5-japan"),
                sha1: None,
                sha256: None,
            },
        },
    ];

    const TEST_CATALOG: &[FirmwareSpec] = &[FirmwareSpec {
        id: "test.firmware",
        display_name: "Test Firmware",
        system: "Test",
        purpose: "Testing",
        variants: TEST_VARIANTS,
    }];

    fn entry(filename: &str, md5: &str, bytes: &'static [u8]) -> FirmwareInventoryEntry {
        FirmwareInventoryEntry::from_bytes_with_legacy_digests(
            bytes,
            Some(filename.to_owned()),
            Some(md5.to_owned()),
            None,
            TEST_CATALOG,
        )
    }

    #[test]
    fn resolver_selects_known_good_candidate_by_region() {
        let mut inventory = FirmwareInventory::new();
        inventory.add(entry("japan.bin", "md5-japan", b"jpaa"));
        inventory.add(entry("usa.bin", "md5-usa", b"usaa"));

        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_region("usa")];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        let FirmwareSelection::External(selected) = &resolution.entries[0].selection else {
            panic!("expected external firmware");
        };
        assert_eq!(
            selected.variant.as_ref().unwrap().as_ref(),
            "test.firmware.usa"
        );
    }

    #[test]
    fn equal_candidates_preserve_inventory_root_order() {
        let mut inventory = FirmwareInventory::new();
        inventory.add(entry("usa.bin", "md5-usa", b"one1"));
        inventory.add(entry("usa.bin", "md5-usa", b"two2"));
        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_region("usa")];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        let FirmwareSelection::External(selected) = &resolution.entries[0].selection else {
            panic!("expected external firmware");
        };
        assert_eq!(&*selected.bytes, b"one1");
    }

    #[test]
    fn preferred_variant_ranks_without_excluding_known_fallback() {
        let mut inventory = FirmwareInventory::new();
        inventory.add(entry("japan.bin", "md5-japan", b"jpaa"));
        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )
        .with_preferred_variant("test.firmware.usa")];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        let FirmwareSelection::External(selected) = &resolution.entries[0].selection else {
            panic!("expected fallback external firmware");
        };
        assert_eq!(
            selected.variant.as_ref().unwrap().as_ref(),
            "test.firmware.japan"
        );
    }

    #[test]
    fn pce_plan_prefers_regional_v3_and_accepts_exact_v2_fallback() {
        let catalog = crate::catalog::catalog_specs();
        let v2 = FirmwareInventoryEntry::from_bytes_with_legacy_digests(
            vec![2; 262_144],
            Some("syscard2.pce".to_owned()),
            Some("3cdd6614a918616bfc41c862e889dd79".to_owned()),
            None,
            catalog,
        );
        let v3 = FirmwareInventoryEntry::from_bytes_with_legacy_digests(
            vec![3; 262_144],
            Some("syscard3.pce".to_owned()),
            Some("38179df8f4ac870017db21ebcbf53114".to_owned()),
            None,
            catalog,
        );
        let plan = crate::catalog::firmware_plan_for_pce_cdrom2_region("japan");

        let mut inventory = FirmwareInventory::new();
        inventory.add(v2.clone());
        inventory.add(v3);
        let resolution = FirmwareResolver::new(catalog, &inventory).resolve(&plan);
        let FirmwareSelection::External(selected) = &resolution.entries[0].selection else {
            panic!("expected preferred v3 firmware");
        };
        assert_eq!(
            selected.variant.as_ref().unwrap().as_ref(),
            "nec.pce.cd.system_card.v3"
        );

        let mut inventory = FirmwareInventory::new();
        inventory.add(v2);
        let resolution = FirmwareResolver::new(catalog, &inventory).resolve(&plan);
        let FirmwareSelection::External(selected) = &resolution.entries[0].selection else {
            panic!("expected exact v2 fallback firmware");
        };
        assert_eq!(
            selected.variant.as_ref().unwrap().as_ref(),
            "nec.pce.cd.system_card.v2"
        );
    }

    #[test]
    fn resolver_uses_hle_fallback_for_missing_recommended_firmware() {
        let inventory = FirmwareInventory::new();
        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Recommended,
            FallbackKind::Hle {
                implementation: "test-hle".to_owned(),
                compatibility_version: 7,
            },
            FirmwareDependency::RuntimeCallable,
        )];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        assert!(!resolution.has_blocking_failure());
        assert!(matches!(
            &resolution.entries[0].selection,
            FirmwareSelection::Fallback(FirmwareSelectionManifest::Hle {
                implementation,
                compatibility_version: 7,
                ..
            }) if implementation == "test-hle"
        ));
    }

    #[test]
    fn resolver_blocks_missing_required_firmware_without_fallback() {
        let inventory = FirmwareInventory::new();
        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Required,
            FallbackKind::None,
            FirmwareDependency::RuntimeMapped,
        )];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        assert!(resolution.has_blocking_failure());
        assert!(matches!(
            &resolution.entries[0].selection,
            FirmwareSelection::Missing(ResolveFailure::MissingRequired { .. })
        ));
    }

    #[test]
    fn resolver_does_not_select_unknown_hash_without_override_policy() {
        let mut inventory = FirmwareInventory::new();
        inventory.add(FirmwareInventoryEntry::from_bytes(
            b"abcd".as_slice(),
            Some("usa.bin".to_owned()),
            TEST_CATALOG,
        ));
        let plan = [FirmwareRequest::new(
            "test.firmware",
            RequirementLevel::Optional,
            FallbackKind::SkipBoot {
                compatibility_version: 1,
            },
            FirmwareDependency::BootOnly,
        )
        .with_region("usa")];

        let resolution = FirmwareResolver::new(TEST_CATALOG, &inventory).resolve(&plan);
        assert_eq!(resolution.entries[0].candidates.len(), 1);
        assert!(!resolution.entries[0].candidates[0].known_good);
        assert!(matches!(
            &resolution.entries[0].selection,
            FirmwareSelection::Fallback(FirmwareSelectionManifest::Skipped { .. })
        ));
    }
}
