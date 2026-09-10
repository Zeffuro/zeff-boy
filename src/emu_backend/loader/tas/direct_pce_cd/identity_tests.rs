use super::archive_ppf_combinations_identity::*;
use super::{
    PceCdExpansion, PceCdTasMediaRoute, PceCdTasProfile, direct_pce_cd_arcade_eligible,
    direct_pce_cd_arcade_tas_sync_config_sha256,
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
    direct_pce_multitap_cd_archive_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_tas_sync_config_sha256,
    direct_pce_multitap_cd_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_tas_sync_configs_for_test,
    direct_pce_multitap_cd_rar_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256,
    direct_pce_multitap_cd_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_tas_sync_config_sha256, firmware_profile_is_supported,
    is_direct_pce_multitap_cd_ppf_tas_sync_config_sha256, sync_config_for_runtime,
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
        (
            direct_pce_cd_archive_ppf_arcade_tas_sync_config_sha256(),
            "9de55c94f2d56d677ebde60090d378072c0032ec5a5b706a34c0aeb85e4762b0",
        ),
        (
            direct_pce_cd_archive_ppf_memory_base_tas_sync_config_sha256(),
            "49a51bfedb68bab647ebea1257688df91ca97d53729293894d102c521d0babcd",
        ),
        (
            direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256(),
            "0665b156849da174d7888fc086d0f8451e83868baf23603497afd2a7f85b0d1d",
        ),
        (
            direct_pce_multitap_cd_archive_ppf_arcade_tas_sync_config_sha256(),
            "8c20b61b935b29b47e91f758f19db167848fbe1ad50fab22afb30e360840dc1c",
        ),
        (
            direct_pce_multitap_cd_archive_ppf_memory_base_tas_sync_config_sha256(),
            "1f23ff9c1e825741ad0e3bd83932f92d776fdbbef3a5203b82b319c5a5a14a71",
        ),
        (
            direct_pce_cd_selected_archive_ppf_arcade_tas_sync_config_sha256(),
            "2605ef8c87d940d3d9f8a9d73c9528bc586e92e07bd61a68c3fa649405b72830",
        ),
        (
            direct_pce_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256(),
            "62dd814c9e4b2a64787a0740a16ca15494d8221ab26faea46189f9793caa51e6",
        ),
        (
            direct_pce_multitap_cd_selected_archive_ppf_tas_sync_config_sha256(),
            "48671b8f9ea952afab5db445e150682c577881c3890bab28592f03f85ec27033",
        ),
        (
            direct_pce_multitap_cd_selected_archive_ppf_arcade_tas_sync_config_sha256(),
            "ef33eccbf294d95ef7387ead2d904cf3bb0756c88a3342eedd50213bd7364e91",
        ),
        (
            direct_pce_multitap_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256(),
            "2444bb1c375853720c9ea290d788b0359c70b4dca033bf90e81fe54edd831caa",
        ),
        (
            direct_pce_cd_rar_ppf_arcade_tas_sync_config_sha256(),
            "823b1608877645bc0c40d8e9df9c750b83faeddfcfe82cf973e4ad0df73808ec",
        ),
        (
            direct_pce_cd_rar_ppf_memory_base_tas_sync_config_sha256(),
            "98b735ec83eefe9885f9409b9bbb01d1509dca2931fd6a646a71f50684a5e8f0",
        ),
        (
            direct_pce_multitap_cd_rar_ppf_tas_sync_config_sha256(),
            "cedc5b6afd4ad0f4010afc0e340a9c659d580fd85c42b63dbf4de1090232e422",
        ),
        (
            direct_pce_multitap_cd_rar_ppf_arcade_tas_sync_config_sha256(),
            "78355998504bc3e9ec43f73a9daff89e470ddde5437ca51efbcfc3f44c6c9aa6",
        ),
        (
            direct_pce_multitap_cd_rar_ppf_memory_base_tas_sync_config_sha256(),
            "36d6f4d16a169ffe75ed3ff03d3eae8f461898daf84bbf796422cb954a089bfd",
        ),
        (
            direct_pce_cd_selected_rar_ppf_arcade_tas_sync_config_sha256(),
            "872518ba51f828169ec89881a85d9a6233ea06659239c15334a46c2c216f6846",
        ),
        (
            direct_pce_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256(),
            "9631a759d20a01f47f5185f37ae279bee31685ea2dee2e6de60879801cc2b777",
        ),
        (
            direct_pce_multitap_cd_selected_rar_ppf_tas_sync_config_sha256(),
            "f1e704ba31e70cf6a0e9f728f10102d0d7a2e57b47c147a2f365c5d73f599b26",
        ),
        (
            direct_pce_multitap_cd_selected_rar_ppf_arcade_tas_sync_config_sha256(),
            "47c3543baf577dc796c1e6577f65d2daf98ee3650d9fce4d26c367896ac17c74",
        ),
        (
            direct_pce_multitap_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256(),
            "3bda98f0b4e00386ce9c144093402f633f384d4ed9cc42e9adc1452b63f7e7a9",
        ),
        (
            direct_pce_cd_zip_ppf_arcade_tas_sync_config_sha256(),
            "d40e1ffddd6b90abd05cbc24818730fa5357a334368ea0bc330094a8b93bdb02",
        ),
        (
            direct_pce_cd_zip_ppf_memory_base_tas_sync_config_sha256(),
            "0971a281e91f6eef8613ff35fc760a1b1128c4ce79e8f93613765d33ae7e51e4",
        ),
        (
            direct_pce_multitap_cd_zip_ppf_tas_sync_config_sha256(),
            "3985f633a1315948957ed9872e68b06b795bcb6086e8bf736bce8138005d920c",
        ),
        (
            direct_pce_multitap_cd_zip_ppf_arcade_tas_sync_config_sha256(),
            "ab3ae7748c6c7a5dda1c692badbb39555ff711bbd3b52e7c34b29d8d14bfded4",
        ),
        (
            direct_pce_multitap_cd_zip_ppf_memory_base_tas_sync_config_sha256(),
            "a208e7e58dfd8c02f71dd882cad307b7bcf9428ac7904afa657ee9a0b9eee615",
        ),
        (
            direct_pce_cd_selected_zip_ppf_arcade_tas_sync_config_sha256(),
            "e84f3a2ebc8a1bd63fa8a0a629655ff005b5f55e3a419533c898d728d49e425f",
        ),
        (
            direct_pce_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256(),
            "f885c6bcbb7e43692c659fe0c98b52a22a0fb9262a92f37bb79a83a0f738acf9",
        ),
        (
            direct_pce_multitap_cd_selected_zip_ppf_tas_sync_config_sha256(),
            "21168dd8432d1854fca5012d69e55552915fa3f631d898fd574cc48d1269fca3",
        ),
        (
            direct_pce_multitap_cd_selected_zip_ppf_arcade_tas_sync_config_sha256(),
            "715e880a738df96de450c21ca7808c9932e52d3708a7299946e541f57bbe521f",
        ),
        (
            direct_pce_multitap_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256(),
            "0d0fc6f649a2df9e8ff54951754b0d10a2dd2152f5ae2f5d285dda130ce81646",
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
fn archive_ppf_profiles_cover_every_route_card_and_controller_combination() {
    let two_button = super::direct_pce_cd_archive_ppf_tas_sync_configs_for_test();
    let multitap = super::direct_pce_multitap_cd_archive_ppf_tas_sync_configs_for_test();
    let distinct = two_button
        .into_iter()
        .chain(multitap)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(two_button.len(), 18);
    assert_eq!(multitap.len(), 18);
    assert_eq!(distinct.len(), 36);

    for (syncs, controller) in [
        (two_button, PceControllerMode::TwoButton),
        (multitap, PceControllerMode::Multitap),
    ] {
        let mut expansions = [0; 3];
        for sync in syncs {
            let profile = PceCdTasProfile::from_sync(sync).expect("archive PPF profile");
            assert!(profile.archive_ppf());
            assert!(profile.archive().is_some());
            assert_eq!(profile.controller(), controller);
            assert_eq!(profile.sync_config(), sync);
            expansions[match profile.expansion() {
                PceCdExpansion::None => 0,
                PceCdExpansion::ArcadeCard => 1,
                PceCdExpansion::MemoryBase128 => 2,
            }] += 1;
        }
        assert_eq!(expansions, [6, 6, 6]);
    }
    for sync in two_button {
        assert!(super::is_direct_pce_cd_archive_ppf_tas_sync_config_sha256(
            sync
        ));
        assert!(!super::is_direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256(sync));
    }
    for sync in multitap {
        assert!(!super::is_direct_pce_cd_archive_ppf_tas_sync_config_sha256(
            sync
        ));
        assert!(super::is_direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256(sync));
    }

    let archive = (false, false, false, true, false, false);
    for (cards, expansion) in [
        ((false, false), PceCdExpansion::None),
        ((true, false), PceCdExpansion::ArcadeCard),
        ((false, true), PceCdExpansion::MemoryBase128),
    ] {
        for controller in [PceControllerMode::TwoButton, PceControllerMode::Multitap] {
            let profile = PceCdTasProfile::from_runtime_flags(
                archive,
                true,
                (false, false, false),
                cards,
                controller,
            )
            .expect("supported archive PPF profile");
            assert!(profile.archive_ppf());
            assert_eq!(profile.expansion(), expansion);
            assert_eq!(profile.controller(), controller);
            assert_eq!(
                PceCdTasProfile::from_sync(profile.sync_config()),
                Some(profile)
            );
        }
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
            direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256(),
            "27f56bca3aa9c1a9132ed8970cd952d5c306c4f390bf4be3e354808fed7c0d32",
        ),
        (
            direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256(),
            "96f773e95dc1aaf56d68f84fa15ac3f0ccb6dc2a73b34601df97d88d2718bd1e",
        ),
        (
            direct_pce_multitap_cd_iso_tas_sync_config_sha256(),
            "e4fb2a65d6545872a510580ac9fa2b67b0aa13bba773a830e04acbf59b1b4133",
        ),
        (
            direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256(),
            "9089e25e0ba1f3a514cb7116c5accc7f466b318103f178f1d0bb299ff8f113db",
        ),
        (
            direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256(),
            "6d0410307c40f88188c06ed7c88058e5d126e9213055c988522f74c6b8ea9cda",
        ),
        (
            direct_pce_multitap_cd_ppf_tas_sync_config_sha256(),
            "97e4ce197ea0d1bba808913634cf2ad5a202ec13ee5ea82f0dc4694e7d4f68b1",
        ),
        (
            direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256(),
            "0cd9f3edecb04e8b6f0421c5d878c2eb894820884783c7c9940c1ef302bc7299",
        ),
        (
            direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256(),
            "4ea422ab3e7e65bb7cc78185ea455209dd1561d3723a6c8c6e87aa1b3a81228a",
        ),
        (
            direct_pce_multitap_cd_archive_tas_sync_config_sha256(),
            "53557f951574288745fe97e43810ed70c4cb1181ac233ad8d9faee487cdc1a9f",
        ),
        (
            direct_pce_multitap_cd_archive_arcade_tas_sync_config_sha256(),
            "b24f4b2907a0a0f08daa59afa9bae7c3854d3306331aeab762480463e4a62113",
        ),
        (
            direct_pce_multitap_cd_archive_memory_base_tas_sync_config_sha256(),
            "205f1d64637f5726276c022d7252d116330831a204f39e57dc3355e0786b948c",
        ),
        (
            direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256(),
            "295a29bd238cc15d9589a379740a1c93e020d09ed7568c99116b262caade8f71",
        ),
        (
            direct_pce_multitap_cd_selected_archive_arcade_tas_sync_config_sha256(),
            "e4ec642823d41222318246a4bf6b00d62dbdba095447669428c1780f0e607467",
        ),
        (
            direct_pce_multitap_cd_selected_archive_memory_base_tas_sync_config_sha256(),
            "11114695b266fa638d869c62bc875cdb1bb26736e106031741ec55e2df53af85",
        ),
        (
            direct_pce_multitap_cd_rar_tas_sync_config_sha256(),
            "3b1d6260a83bedc074429255744d521e5567c6e05ba324b7a012620e3a2e23f2",
        ),
        (
            direct_pce_multitap_cd_rar_arcade_tas_sync_config_sha256(),
            "32485a6f79de726d9da8d265c4387c9615b77fb879054239f4137f6518ae7d76",
        ),
        (
            direct_pce_multitap_cd_rar_memory_base_tas_sync_config_sha256(),
            "fa022284398a9d8accc01babc885c7fb639204f11d425a27fd6add577a3b79c9",
        ),
        (
            direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256(),
            "ee58cda48b80de51b22f56dfbbd56b8d9ee8641271f6c773e5c6bb730dd4d589",
        ),
        (
            direct_pce_multitap_cd_selected_rar_arcade_tas_sync_config_sha256(),
            "da2be53241f08d8e02eb234442bd04a065429a92d159cf8bbef3eb2107b7ae49",
        ),
        (
            direct_pce_multitap_cd_selected_rar_memory_base_tas_sync_config_sha256(),
            "8ebe7859b22d8f4f853ab037165677d3e53ccd4a81d0bfb92e6ef774e05d3b07",
        ),
        (
            direct_pce_multitap_cd_zip_tas_sync_config_sha256(),
            "2b9808e83322b017f19963c22ee0d0cda066e03213408702c113463fe66f299c",
        ),
        (
            direct_pce_multitap_cd_zip_arcade_tas_sync_config_sha256(),
            "38f6cbe6ca1e22aaa3fe29c05cd6cabadac8a4e285a300522be915081e081fad",
        ),
        (
            direct_pce_multitap_cd_zip_memory_base_tas_sync_config_sha256(),
            "6fb5474c6736ceec37fe224154d2541b303abfabe21cd31f105a45836cb023b8",
        ),
        (
            direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256(),
            "3e20a90524c2b33b1945cf3bd56894821348f289e289e219145844dca24abea6",
        ),
        (
            direct_pce_multitap_cd_selected_zip_arcade_tas_sync_config_sha256(),
            "2aecfdb8e2356f15e586c449856f2503f71cf018da01ac1182679b29ee3bb7d9",
        ),
        (
            direct_pce_multitap_cd_selected_zip_memory_base_tas_sync_config_sha256(),
            "55916adf02b991a578e739225982c8402c8f1d436d6815b3bcf992a0bc06ffd1",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_hex(), expected);
    }
}

#[test]
fn direct_card_multitap_runtime_flags_select_all_six_routes() {
    let routes = [
        (
            (true, false, false, false, false, false),
            PceCdTasMediaRoute::Chd,
            (true, false),
            PceCdExpansion::ArcadeCard,
            direct_pce_multitap_cd_chd_arcade_tas_sync_config_sha256(),
        ),
        (
            (true, false, false, false, false, false),
            PceCdTasMediaRoute::Chd,
            (false, true),
            PceCdExpansion::MemoryBase128,
            direct_pce_multitap_cd_chd_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, true, false, false, false, false),
            PceCdTasMediaRoute::Iso,
            (true, false),
            PceCdExpansion::ArcadeCard,
            direct_pce_multitap_cd_iso_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, true, false, false, false, false),
            PceCdTasMediaRoute::Iso,
            (false, true),
            PceCdExpansion::MemoryBase128,
            direct_pce_multitap_cd_iso_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, true, false, false, false),
            PceCdTasMediaRoute::Ppf,
            (true, false),
            PceCdExpansion::ArcadeCard,
            direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, true, false, false, false),
            PceCdTasMediaRoute::Ppf,
            (false, true),
            PceCdExpansion::MemoryBase128,
            direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256(),
        ),
    ];
    for (media_flags, media, cards, expansion, expected) in routes {
        let profile = PceCdTasProfile::from_runtime_flags(
            media_flags,
            false,
            (false, false, false),
            cards,
            PceControllerMode::Multitap,
        )
        .expect("direct card Multitap profile");
        assert_eq!(profile.media(), media);
        assert_eq!(profile.expansion(), expansion);
        assert_eq!(profile.controller(), PceControllerMode::Multitap);
        assert_eq!(profile.sync_config(), expected);
        assert_eq!(PceCdTasProfile::from_sync(expected), Some(profile));
    }
}

#[test]
fn direct_ppf_multitap_sync_set_contains_exactly_three_expansions() {
    let expected = [
        direct_pce_multitap_cd_ppf_tas_sync_config_sha256(),
        direct_pce_multitap_cd_ppf_arcade_tas_sync_config_sha256(),
        direct_pce_multitap_cd_ppf_memory_base_tas_sync_config_sha256(),
    ];
    assert_eq!(
        direct_pce_multitap_cd_ppf_tas_sync_configs_for_test(),
        expected
    );
    for sync in expected {
        assert!(is_direct_pce_multitap_cd_ppf_tas_sync_config_sha256(sync));
    }
    assert!(!is_direct_pce_multitap_cd_ppf_tas_sync_config_sha256(
        direct_pce_cd_ppf_tas_sync_config_sha256()
    ));
}

#[test]
fn archive_arcade_multitap_runtime_flags_select_all_six_routes() {
    let routes = [
        (
            (false, false, false, true, false, false),
            (false, false, false),
            direct_pce_multitap_cd_archive_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (true, false, false),
            direct_pce_multitap_cd_selected_archive_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, false, false),
            direct_pce_multitap_cd_rar_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, true, false),
            direct_pce_multitap_cd_selected_rar_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, false),
            direct_pce_multitap_cd_zip_arcade_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, true),
            direct_pce_multitap_cd_selected_zip_arcade_tas_sync_config_sha256(),
        ),
    ];
    for (media, selection, expected) in routes {
        let profile = PceCdTasProfile::from_runtime_flags(
            media,
            false,
            selection,
            (true, false),
            PceControllerMode::Multitap,
        )
        .expect("archive Arcade Card Multitap profile");
        assert_eq!(profile.expansion(), PceCdExpansion::ArcadeCard);
        assert_eq!(profile.controller(), PceControllerMode::Multitap);
        assert_eq!(profile.sync_config(), expected);
        assert_eq!(PceCdTasProfile::from_sync(expected), Some(profile));
    }
}

#[test]
fn archive_memory_base_multitap_runtime_flags_select_all_six_routes() {
    let routes = [
        (
            (false, false, false, true, false, false),
            (false, false, false),
            direct_pce_multitap_cd_archive_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, true, false, false),
            (true, false, false),
            direct_pce_multitap_cd_selected_archive_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, false, false),
            direct_pce_multitap_cd_rar_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, true, false),
            (false, true, false),
            direct_pce_multitap_cd_selected_rar_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, false),
            direct_pce_multitap_cd_zip_memory_base_tas_sync_config_sha256(),
        ),
        (
            (false, false, false, false, false, true),
            (false, false, true),
            direct_pce_multitap_cd_selected_zip_memory_base_tas_sync_config_sha256(),
        ),
    ];
    for (media, selection, expected) in routes {
        let profile = PceCdTasProfile::from_runtime_flags(
            media,
            false,
            selection,
            (false, true),
            PceControllerMode::Multitap,
        )
        .expect("archive Memory Base Multitap profile");
        assert_eq!(profile.expansion(), PceCdExpansion::MemoryBase128);
        assert_eq!(profile.controller(), PceControllerMode::Multitap);
        assert_eq!(profile.sync_config(), expected);
        assert_eq!(PceCdTasProfile::from_sync(expected), Some(profile));
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
