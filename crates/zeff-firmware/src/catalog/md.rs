use super::*;

pub(super) const TMSS_SPEC: FirmwareSpec = FirmwareSpec {
    id: "sega.md.tmss",
    display_name: "Mega Drive / Genesis TMSS boot ROM",
    system: "Mega Drive / Genesis",
    purpose: "TMSS startup boot ROM",
    variants: &[FirmwareVariantSpec {
        id: "sega.md.tmss.retail",
        display_name: "Mega Drive / Genesis retail TMSS boot ROM",
        region: "any",
        model: None,
        filenames: &["bios_MD.bin"],
        size: SizeRule::Exact(2048),
        hashes: KnownHashes {
            md5: Some("45e298905a08f9cfb38fd504cd6dbc84"),
            sha1: None,
            sha256: Some("5a5497f47bf3892d60476151494b8e5b7949e6db728ed513d162095d54de1b9a"),
        },
    }],
};

pub fn firmware_plan_for_mega_drive_tmss(enabled: bool) -> Vec<FirmwareRequest> {
    if !enabled {
        return Vec::new();
    }
    vec![FirmwareRequest::new(
        "sega.md.tmss",
        RequirementLevel::Required,
        FallbackKind::None,
        FirmwareDependency::RuntimeMapped,
    )]
}

pub(super) const SVP_SPEC: FirmwareSpec = FirmwareSpec {
    id: "sega.md.svp.irom",
    display_name: "SVP internal program ROM",
    system: "Mega Drive / Genesis",
    purpose: "SVP startup and math routines",
    variants: &[FirmwareVariantSpec {
        id: "sega.md.svp.irom.ssp1601",
        display_name: "SVP SSP1601 internal ROM",
        region: "any",
        model: Some("svp"),
        filenames: &["svp.bin"],
        size: SizeRule::Exact(2048),
        hashes: KnownHashes {
            md5: Some("0850e5d87f1e730ac641b5c91bc345dd"),
            sha1: Some("0b951ea9c6094b3c34e4f0b64d031c75c237564f"),
            sha256: Some("772dfb1896b490a406215aba4f83659f96281af95324db99130a05caebba2128"),
        },
    }],
};

#[cfg(test)]
pub(super) fn assert_svp_spec() {
    let spec = catalog_specs()
        .iter()
        .find(|spec| spec.id == "sega.md.svp.irom")
        .unwrap();
    let variant = &spec.variants[0];
    assert_eq!(variant.filenames, &["svp.bin"]);
    assert_eq!(variant.size, SizeRule::Exact(2048));
    assert_eq!(variant.model, Some("svp"));
    assert_eq!(
        variant.hashes.sha1,
        Some("0b951ea9c6094b3c34e4f0b64d031c75c237564f")
    );
}
