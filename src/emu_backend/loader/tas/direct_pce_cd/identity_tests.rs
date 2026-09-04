use super::{
    PceCdArchiveFormat, PceCdArchiveSelection, PceCdExpansion, PceCdTasProfile,
    direct_pce_cd_arcade_eligible, direct_pce_cd_arcade_tas_sync_config_sha256,
    direct_pce_cd_archive_arcade_tas_sync_config_sha256,
    direct_pce_cd_archive_memory_base_tas_sync_config_sha256,
    direct_pce_cd_archive_ppf_source_identity, direct_pce_cd_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_archive_source_identity, direct_pce_cd_archive_tas_sync_config_sha256,
    direct_pce_cd_chd_arcade_tas_sync_config_sha256,
    direct_pce_cd_chd_memory_base_tas_sync_config_sha256, direct_pce_cd_chd_tas_sync_config_sha256,
    direct_pce_cd_iso_arcade_tas_sync_config_sha256,
    direct_pce_cd_iso_memory_base_tas_sync_config_sha256, direct_pce_cd_iso_tas_sync_config_sha256,
    direct_pce_cd_memory_base_eligible, direct_pce_cd_memory_base_tas_sync_config_sha256,
    direct_pce_cd_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_ppf_memory_base_tas_sync_config_sha256, direct_pce_cd_ppf_tas_sync_config_sha256,
    direct_pce_cd_rar_arcade_tas_sync_config_sha256,
    direct_pce_cd_rar_memory_base_tas_sync_config_sha256, direct_pce_cd_rar_ppf_source_identity,
    direct_pce_cd_rar_ppf_tas_sync_config_sha256, direct_pce_cd_rar_source_identity,
    direct_pce_cd_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_tas_sync_config_sha256, direct_pce_cd_tas_sync_config_sha256,
    direct_pce_cd_zip_arcade_tas_sync_config_sha256,
    direct_pce_cd_zip_memory_base_tas_sync_config_sha256, direct_pce_cd_zip_ppf_source_identity,
    direct_pce_cd_zip_ppf_tas_sync_config_sha256, direct_pce_cd_zip_source_identity,
    direct_pce_cd_zip_tas_sync_config_sha256, direct_pce_multitap_cd_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_tas_sync_config_sha256,
    direct_pce_multitap_cd_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256,
    direct_pce_multitap_cd_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_tas_sync_config_sha256, firmware_profile_is_supported,
    sync_config_for_runtime,
};
use zeff_pce_core::hardware::PceControllerMode;

#[test]
fn archive_sync_vectors_are_stable() {
    let vectors = [
        (
            direct_pce_multitap_cd_arcade_tas_sync_config_sha256(),
            "aa28022010045c7813f7f81ba9e8a189ca86cfa1870d86d1e0b6844de26416fc",
        ),
        (
            direct_pce_cd_archive_tas_sync_config_sha256(),
            "0f902a9940f5b1aec2b274abdc0bf97cc15a3795c05366915c55babdc952ac08",
        ),
        (
            direct_pce_cd_archive_arcade_tas_sync_config_sha256(),
            "320a51f6991c061b16bf50507529da7998b3717170190eb69e6268991d97f0fa",
        ),
        (
            direct_pce_cd_archive_memory_base_tas_sync_config_sha256(),
            "62404875ce6e3467c79ce2383b61020a92df7ac319ce9e8becff58b340bf861b",
        ),
        (
            direct_pce_cd_selected_archive_tas_sync_config_sha256(),
            "2bcf0d7630fdb6a4ffa6908e7613b096236ce5bf2b7b99df3442d496013917a2",
        ),
        (
            direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256(),
            "9e8bfc3c5e25aabc3b74dce358476819c196e3edeab924e5fc9b7ff0d6c5959d",
        ),
        (
            direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256(),
            "feb1ad7d629d967d537ca4edf08146508355506d15d5035505dac67edc910d5b",
        ),
        (
            direct_pce_cd_rar_tas_sync_config_sha256(),
            "62870bd496dd818693834dc1c643afa10bdbb34d038993c4d537321eb2bcc5cf",
        ),
        (
            direct_pce_cd_rar_arcade_tas_sync_config_sha256(),
            "965c01991c0227b9c49ed7c6ee78f38a4b6aa0aff748574f593c975741915244",
        ),
        (
            direct_pce_cd_rar_memory_base_tas_sync_config_sha256(),
            "f1ba56b8f1d47b753bc99c606c4915d6f9d1ff31c04baa442fe04b8f55e8f9f4",
        ),
        (
            direct_pce_cd_selected_rar_tas_sync_config_sha256(),
            "4dde8b66bd07c7dfe735dadf8be105da7b50c3e4168f90a10f910f498853935e",
        ),
        (
            direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256(),
            "86eb85221f8fcd01c1c94fec15d698200b191da0f17a6e3b9922cfd5df66f6fa",
        ),
        (
            direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256(),
            "bbde8191cdc66cd72bb5a72ec7632c4c21bfcd7243a78c0ca70ce0780c6336b4",
        ),
        (
            direct_pce_cd_zip_tas_sync_config_sha256(),
            "a29e2c401762d298d3b73fc8a5c1d97092c52b49e6e102d21ceafec182de6f12",
        ),
        (
            direct_pce_cd_zip_arcade_tas_sync_config_sha256(),
            "63144ae542b9bbbfe2eed2db3988d9b8d5f74f614c75934c3e724e2afab0ed60",
        ),
        (
            direct_pce_cd_zip_memory_base_tas_sync_config_sha256(),
            "3c4e9ecb545f11340ad5421d067aebaf0e6d24aa5a2b4d0d1efab2a575f89a35",
        ),
        (
            direct_pce_cd_selected_zip_tas_sync_config_sha256(),
            "bfb20622169023cf2518258a5da9063d2afedb8f7aaae00253220bf88d42c4ac",
        ),
        (
            direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256(),
            "aafb4ab28739f4dfea40c240cbc94a5bdb5750dce7d9b4b2aa1506427528e1d8",
        ),
        (
            direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256(),
            "c35dc15aed8c109567d1250257a69960d0282ad1370a431729aab4ba6abcb4fb",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn archive_source_vectors_are_stable() {
    let raw_sha256 = std::array::from_fn(|idx| idx as u8);
    let member_sha256 = std::array::from_fn(|idx| 31 - idx as u8);
    let raw_len = 0x0102_0304_usize;
    let vectors = [
        (
            direct_pce_cd_archive_source_identity(raw_sha256, raw_len, member_sha256),
            "7700877007cadd88c1395f856a7452bf3816f0d63b87d55f009538b9e5607288",
        ),
        (
            direct_pce_cd_rar_source_identity(raw_sha256, raw_len, member_sha256),
            "47f210dd3e0a09a6a3e6baf19b627bbe06772c94c68ab9b24aa7dce4c93865ca",
        ),
        (
            direct_pce_cd_zip_source_identity(raw_sha256, raw_len, member_sha256),
            "385e1180c30310efadc37e1e4b049567557ae83ff3c2094bfca361a0a490df47",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn archive_ppf_sync_and_source_vectors_are_stable() {
    let syncs = [
        (
            direct_pce_cd_archive_ppf_tas_sync_config_sha256(),
            "ad4698bd280e24fe1e05fb0bb5588a9208de1544e10184450433f5c5b1264264",
        ),
        (
            direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256(),
            "47efa7d1c38c56dbaba9cf4dc4cab40b1b625c3610bff4704f7310b148591076",
        ),
        (
            direct_pce_cd_rar_ppf_tas_sync_config_sha256(),
            "afef4b4b0b955e0e8fc56ac6d0872e3f628f9d82c554e3b37cf45e1c404d6bb9",
        ),
        (
            direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256(),
            "7f9c0f4f6e822b12903d70c43d5fde99bc4c3de526261ef5941af49e6f0c2ee9",
        ),
        (
            direct_pce_cd_zip_ppf_tas_sync_config_sha256(),
            "c3b999d5a5d750cec70df81ba682eb6f288560e0c1724a1a866e480684ca3a8e",
        ),
        (
            direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256(),
            "930150573c80748aedbda06b4ba4c48298672a340b31eb1af85639efd3ab623f",
        ),
    ];
    for (actual, expected) in syncs {
        assert_eq!(actual.to_hex(), expected);
    }
    let raw = std::array::from_fn(|idx| idx as u8);
    let cue = std::array::from_fn(|idx| 31 - idx as u8);
    let patches = [
        ("dir/disc.ppf/0001.ppf", 0x102, [0xA5; 32]),
        (
            "dir/disc.ppf/0002.ppf",
            0x010203,
            std::array::from_fn(|idx| 255 - idx as u8),
        ),
    ];
    let sources = [
        (
            direct_pce_cd_archive_ppf_source_identity(raw, 0x01020304, cue, &patches),
            "44e99306792ffe1040f360a1faa3a39d567e4a9ddc03db86c46cbb28915d2571",
        ),
        (
            direct_pce_cd_rar_ppf_source_identity(raw, 0x01020304, cue, &patches),
            "983cbf0a523cca7673da619c86b03f816033508ffeec13444cbe42a137cf607c",
        ),
        (
            direct_pce_cd_zip_ppf_source_identity(raw, 0x01020304, cue, &patches),
            "1006c26dabb156b5233ebb349db2ea50fec72fd6c910776947d64d1d6cfa72fc",
        ),
    ];
    for (actual, expected) in sources {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn archive_ppf_profiles_are_exact_two_button_no_card_routes() {
    let syncs = [
        (
            direct_pce_cd_archive_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Unique,
        ),
        (
            direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Selected,
        ),
        (
            direct_pce_cd_rar_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::Rar,
            PceCdArchiveSelection::Unique,
        ),
        (
            direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::Rar,
            PceCdArchiveSelection::Selected,
        ),
        (
            direct_pce_cd_zip_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::Zip,
            PceCdArchiveSelection::Unique,
        ),
        (
            direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256(),
            PceCdArchiveFormat::Zip,
            PceCdArchiveSelection::Selected,
        ),
    ];
    for (sync, format, selection) in syncs {
        let profile = PceCdTasProfile::from_sync(sync).expect("archive PPF profile");
        assert!(profile.archive_ppf());
        assert_eq!(profile.archive(), Some((format, selection)));
        assert_eq!(profile.expansion(), PceCdExpansion::None);
        assert_eq!(profile.controller(), PceControllerMode::TwoButton);
        assert_eq!(profile.sync_config(), sync);
    }
    let archive = (false, false, false, true, false, false);
    assert!(
        PceCdTasProfile::from_runtime_flags(
            archive,
            true,
            (false, false, false),
            (false, false),
            PceControllerMode::TwoButton,
        )
        .is_some_and(PceCdTasProfile::archive_ppf)
    );
    for (cards, controller) in [
        ((true, false), PceControllerMode::TwoButton),
        ((false, true), PceControllerMode::TwoButton),
        ((false, false), PceControllerMode::Multitap),
    ] {
        assert!(
            PceCdTasProfile::from_runtime_flags(
                archive,
                true,
                (false, false, false),
                cards,
                controller,
            )
            .is_none()
        );
    }
    for (media, selected) in [
        (
            (false, false, false, false, false, false),
            (false, false, false),
        ),
        (
            (true, false, false, true, false, false),
            (false, false, false),
        ),
        (
            (false, false, false, true, true, false),
            (false, false, false),
        ),
        (
            (false, false, false, true, false, false),
            (false, true, false),
        ),
        (
            (false, false, false, true, false, false),
            (true, true, false),
        ),
    ] {
        assert!(
            PceCdTasProfile::from_runtime_flags(
                media,
                true,
                selected,
                (false, false),
                PceControllerMode::TwoButton,
            )
            .is_none()
        );
    }
}

#[test]
fn archive_ppf_source_identity_binds_every_ordered_input() {
    let raw = [1; 32];
    let cue = [2; 32];
    let patches = [
        ("dir/disc.ppf/0001.ppf", 11, [3; 32]),
        ("dir/disc.ppf/0002.ppf", 22, [4; 32]),
    ];
    let base = direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &patches);
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity([9; 32], 33, cue, &patches)
    );
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 34, cue, &patches)
    );
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, [9; 32], &patches)
    );
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &patches[..1])
    );
    let renamed = [("dir/disc.ppf/0008.ppf", 11, [3; 32]), patches[1]];
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &renamed)
    );
    let resized = [(patches[0].0, 12, patches[0].2), patches[1]];
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &resized)
    );
    let rehashed = [(patches[0].0, patches[0].1, [8; 32]), patches[1]];
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &rehashed)
    );
    let reversed = [patches[1], patches[0]];
    assert_ne!(
        base,
        direct_pce_cd_archive_ppf_source_identity(raw, 33, cue, &reversed)
    );
    assert_ne!(
        base,
        direct_pce_cd_rar_ppf_source_identity(raw, 33, cue, &patches)
    );
    assert_ne!(
        base,
        direct_pce_cd_zip_ppf_source_identity(raw, 33, cue, &patches)
    );
}

#[test]
fn direct_multitap_sync_vectors_are_stable() {
    let vectors = [
        (
            direct_pce_multitap_cd_tas_sync_config_sha256(),
            "6dbe2698026ae2278b851a8b35543a11826409ab5856e30f05b41cb834f4fa83",
        ),
        (
            direct_pce_multitap_cd_chd_tas_sync_config_sha256(),
            "a7587d0581c14ec3c3716474922aff9d020cbdcfba475903e77d3f70f127e975",
        ),
        (
            direct_pce_multitap_cd_iso_tas_sync_config_sha256(),
            "e4fb2a65d6545872a510580ac9fa2b67b0aa13bba773a830e04acbf59b1b4133",
        ),
        (
            direct_pce_multitap_cd_ppf_tas_sync_config_sha256(),
            "97e4ce197ea0d1bba808913634cf2ad5a202ec13ee5ea82f0dc4694e7d4f68b1",
        ),
        (
            direct_pce_multitap_cd_archive_tas_sync_config_sha256(),
            "53557f951574288745fe97e43810ed70c4cb1181ac233ad8d9faee487cdc1a9f",
        ),
        (
            direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256(),
            "295a29bd238cc15d9589a379740a1c93e020d09ed7568c99116b262caade8f71",
        ),
        (
            direct_pce_multitap_cd_rar_tas_sync_config_sha256(),
            "3b1d6260a83bedc074429255744d521e5567c6e05ba324b7a012620e3a2e23f2",
        ),
        (
            direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256(),
            "ee58cda48b80de51b22f56dfbbd56b8d9ee8641271f6c773e5c6bb730dd4d589",
        ),
        (
            direct_pce_multitap_cd_zip_tas_sync_config_sha256(),
            "2b9808e83322b017f19963c22ee0d0cda066e03213408702c113463fe66f299c",
        ),
        (
            direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256(),
            "3e20a90524c2b33b1945cf3bd56894821348f289e289e219145844dca24abea6",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn direct_cue_memory_base_multitap_sync_vector_is_additive() {
    assert_eq!(
        direct_pce_multitap_cd_memory_base_tas_sync_config_sha256().to_hex(),
        "116a7b6aab96319c30d2a8295039d73b3389c78b3b19e82d4153f1e2f5fd559b"
    );
}

#[test]
fn legacy_direct_cue_sync_and_device_vectors_are_stable() {
    assert_eq!(
        direct_pce_cd_tas_sync_config_sha256().to_hex(),
        "26963f220c242207cb5c2ccbd4d4df4af2b0169d77752c8371c2a1d19ad3e161"
    );
    let devices = super::devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].port, "p1");
    assert_eq!(devices[0].device, "pce-two-button-controller");
    assert_eq!(
        devices[0].configuration_sha256.to_hex(),
        "df82045f6c24f06980624e090817b05a55f4e0c51abff0193402a73d494bf361"
    );
}

#[test]
fn legacy_direct_two_button_cd_sync_vectors_are_stable() {
    let vectors = [
        (
            direct_pce_cd_chd_tas_sync_config_sha256(),
            "896a429eabaa7da572b366c4ba4efc26ec03016f445c25423770c29fc9b3805b",
        ),
        (
            direct_pce_cd_iso_tas_sync_config_sha256(),
            "629a81eeff3fc0fd4b9107a0f853f57717328c7024375c40785f02242a721238",
        ),
        (
            direct_pce_cd_ppf_tas_sync_config_sha256(),
            "e3ae9559a0f4436166ab9f85a63c42bd1f32cbaf4ff9eb220f3d510d823d572b",
        ),
        (
            direct_pce_cd_arcade_tas_sync_config_sha256(),
            "2b7782b967b67f7aa8e7c89f3d5272114c69b8fa34129682d3f57915ed15000d",
        ),
        (
            direct_pce_cd_chd_arcade_tas_sync_config_sha256(),
            "ae9fb07b232ec0429ad35d7e50ec9e95a5b818dc9c75dd0cc6ee51f73724c4e2",
        ),
        (
            direct_pce_cd_iso_arcade_tas_sync_config_sha256(),
            "28c041c22782d26ead61fbe69655b9dd0c4a0d1317d51f0efb19adaf3cf2d57c",
        ),
        (
            direct_pce_cd_ppf_arcade_tas_sync_config_sha256(),
            "8155d943ca8081cabe03c0c852de8b5f5e696d6c89e01008754120d190d01495",
        ),
        (
            direct_pce_cd_memory_base_tas_sync_config_sha256(),
            "be705fccff1edfa0c7ae34a9a864488c6f9f1963d975343d42248ee8285dedcc",
        ),
        (
            direct_pce_cd_chd_memory_base_tas_sync_config_sha256(),
            "4b5baba581f10a0444453cd979ce1fe791345c37fee3f2dd6f803bc3edb11e42",
        ),
        (
            direct_pce_cd_iso_memory_base_tas_sync_config_sha256(),
            "166a1cf5e4ac5e78058d3080346d9c0e7f7b03d88a2eae85e96a1807aff7d8ac",
        ),
        (
            direct_pce_cd_ppf_memory_base_tas_sync_config_sha256(),
            "103d16cc913e758f83dac96d2fcabe8578cd4832c6fa5525c4065fdb04111164",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn firmware_profile_rejects_wrong_region_tier_and_unknown_hash() {
    assert!(firmware_profile_is_supported(
        zeff_firmware::PCE_SYSTEM_CARD_V3_JAPAN_SHA256
    ));
    assert!(!firmware_profile_is_supported(
        zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256
    ));
    assert!(!firmware_profile_is_supported(
        zeff_firmware::PCE_SYSTEM_CARD_V2_JAPAN_SHA256
    ));
    assert!(!firmware_profile_is_supported(
        zeff_firmware::PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256
    ));
    assert!(!firmware_profile_is_supported([0; 32]));
}

#[test]
fn archive_sync_configuration_selects_each_no_card_route() {
    let routes = [
        (
            (false, false, false, true, false, false),
            (false, false, false),
            direct_pce_cd_archive_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (true, false, false),
            direct_pce_cd_selected_archive_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, false, false),
            direct_pce_cd_rar_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, true, false),
            direct_pce_cd_selected_rar_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, false),
            direct_pce_cd_zip_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, true),
            direct_pce_cd_selected_zip_tas_sync_config_sha256(),
        ),
    ];
    for (media, selection, expected) in routes {
        assert_eq!(
            sync_config_for_runtime(media, selection, (false, false)),
            expected
        );
    }
}

#[test]
fn arcade_catalog_eligibility_selects_each_exact_direct_source_route() {
    let disc = [
        0xa3, 0x88, 0x7d, 0xa6, 0x25, 0xbb, 0x8d, 0xee, 0x4f, 0xe3, 0x44, 0x76, 0x51, 0x52, 0xab,
        0x43, 0x73, 0xe8, 0xc5, 0x3d, 0x80, 0xda, 0x78, 0x1b, 0x1a, 0xc9, 0x3e, 0x7d, 0x0e, 0x6d,
        0xb8, 0xb2,
    ];
    assert!(direct_pce_cd_arcade_eligible(false, disc));
    assert!(direct_pce_cd_arcade_eligible(true, disc));
    assert!(!direct_pce_cd_arcade_eligible(false, [0; 32]));
    let routes = [
        (
            (true, false, false, false, false, false),
            (false, false, false),
            direct_pce_cd_chd_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, true, false, false, false, false),
            (false, false, false),
            direct_pce_cd_iso_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, true, false, false, false),
            (false, false, false),
            direct_pce_cd_ppf_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (false, false, false),
            direct_pce_cd_archive_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (true, false, false),
            direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, false, false),
            direct_pce_cd_rar_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, true, false),
            direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, false),
            direct_pce_cd_zip_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, true),
            direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256(),
        ),
    ];
    for (media, selection, expected) in routes {
        assert_eq!(
            sync_config_for_runtime(media, selection, (true, false)),
            expected
        );
    }
}

#[test]
fn memory_base_catalog_eligibility_selects_each_exact_direct_source_route() {
    let disc = [
        0x6d, 0x9c, 0x62, 0x34, 0x57, 0x8f, 0x65, 0x3d, 0x4c, 0x81, 0x37, 0x9e, 0x0b, 0xef, 0xfb,
        0x4b, 0x80, 0xbe, 0x18, 0x16, 0xf6, 0x61, 0x42, 0xfd, 0x08, 0x63, 0xa7, 0x79, 0xe6, 0x8f,
        0xab, 0x8f,
    ];
    for media in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert!(direct_pce_cd_memory_base_eligible(
            media.0, media.1, media.2, disc
        ));
    }
    assert!(!direct_pce_cd_memory_base_eligible(true, false, true, disc));
    assert!(!direct_pce_cd_memory_base_eligible(false, true, true, disc));
    assert!(!direct_pce_cd_memory_base_eligible(
        false, false, false, [0; 32]
    ));
    let routes = [
        (
            (false, false, false, false, false, false),
            (false, false, false),
            direct_pce_cd_memory_base_tas_sync_config_sha256(),
        ),
        (
            (true, false, false, false, false, false),
            (false, false, false),
            direct_pce_cd_chd_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, true, false, false, false, false),
            (false, false, false),
            direct_pce_cd_iso_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, true, false, false, false),
            (false, false, false),
            direct_pce_cd_ppf_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (false, false, false),
            direct_pce_cd_archive_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (true, false, false),
            direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, false, false),
            direct_pce_cd_rar_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, true, false),
            direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, false),
            direct_pce_cd_zip_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, true),
            direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256(),
        ),
    ];
    for (media, selection, expected) in routes {
        assert_eq!(
            sync_config_for_runtime(media, selection, (false, true)),
            expected
        );
    }
}
