use crate::tas_project::TasDigest;

#[derive(Clone, Copy)]
enum Expansion {
    None,
    ArcadeCard,
    MemoryBase128,
}

#[derive(Clone, Copy)]
enum Controller {
    TwoButton,
    Multitap,
}

fn sync_config_sha256(route: &str, expansion: Expansion, controller: Controller) -> TasDigest {
    let mut configuration = b"zeff-tas-sync-config-v1\0".to_vec();
    configuration.extend_from_slice(route.as_bytes());
    match (expansion, controller) {
        (Expansion::None, _) => {}
        (Expansion::ArcadeCard, Controller::TwoButton) => {
            configuration.extend_from_slice(b"-arcade-card")
        }
        (Expansion::ArcadeCard, Controller::Multitap) => {
            configuration.extend_from_slice(b"-arcade-card-multitap")
        }
        (Expansion::MemoryBase128, Controller::TwoButton) => {
            configuration.extend_from_slice(b"-memory-base-128")
        }
        (Expansion::MemoryBase128, Controller::Multitap) => {
            configuration.extend_from_slice(b"-memory-base-128-multitap")
        }
    }
    configuration.extend_from_slice(
        b"\0raw-source=outer-sha256-length-plus-cue-member-plus-ordered-ppf-path-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0",
    );
    match controller {
        Controller::TwoButton => configuration.extend_from_slice(b"controller=two-button\0"),
        Controller::Multitap => configuration
            .extend_from_slice(b"controller=five-port-multitap\0ports=p1-p5-two-button\0"),
    }
    match expansion {
        Expansion::None => {
            configuration.extend_from_slice(b"memory-base=disconnected\0arcade-card=disabled\0")
        }
        Expansion::ArcadeCard => configuration.extend_from_slice(
            b"memory-base=disconnected\0arcade-card=enabled-exact-source-catalog\0",
        ),
        Expansion::MemoryBase128 => configuration
            .extend_from_slice(b"memory-base=enabled-exact-source-catalog\0arcade-card=disabled\0"),
    }
    if matches!(controller, Controller::Multitap) {
        match expansion {
            Expansion::None => {}
            Expansion::ArcadeCard => configuration
                .extend_from_slice(b"catalog-witnesses=arcade-card-and-multitap-independent\0"),
            Expansion::MemoryBase128 => configuration
                .extend_from_slice(b"catalog-witnesses=memory-base-and-multitap-independent\0"),
        }
    }
    configuration.extend_from_slice(
        b"mods=archive-contained-ordered-ppf-exact\0external-mods=ignored\0host-persistence=disabled\0native-bram=state-owned\0",
    );
    if matches!(expansion, Expansion::MemoryBase128) {
        configuration.extend_from_slice(b"native-memory-base=state-owned\0");
    }
    configuration.extend_from_slice(b"initial-input=neutral\0");
    if matches!(controller, Controller::Multitap) {
        configuration.extend_from_slice(b"multitap-active-port=none\0select=high\0clear=high\0");
    }
    configuration.extend_from_slice(b"sample-rate=48000\0overscan=full\0palette=raw-rgb\0");
    TasDigest::from_bytes(&configuration)
}

macro_rules! archive_ppf_sync {
    ($name:ident, $route:literal, $expansion:ident, $controller:ident) => {
        pub(crate) fn $name() -> TasDigest {
            sync_config_sha256($route, Expansion::$expansion, Controller::$controller)
        }
    };
}

archive_ppf_sync!(
    direct_pce_cd_archive_ppf_arcade_tas_sync_config_sha256,
    "pce-7z-unique-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_archive_ppf_memory_base_tas_sync_config_sha256,
    "pce-7z-unique-cue-ppf",
    MemoryBase128,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_archive_ppf_arcade_tas_sync_config_sha256,
    "pce-7z-selected-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256,
    "pce-7z-selected-cue-ppf",
    MemoryBase128,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_rar_ppf_arcade_tas_sync_config_sha256,
    "pce-rar-unique-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_rar_ppf_memory_base_tas_sync_config_sha256,
    "pce-rar-unique-cue-ppf",
    MemoryBase128,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_rar_ppf_arcade_tas_sync_config_sha256,
    "pce-rar-selected-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256,
    "pce-rar-selected-cue-ppf",
    MemoryBase128,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_zip_ppf_arcade_tas_sync_config_sha256,
    "pce-zip-unique-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_zip_ppf_memory_base_tas_sync_config_sha256,
    "pce-zip-unique-cue-ppf",
    MemoryBase128,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_zip_ppf_arcade_tas_sync_config_sha256,
    "pce-zip-selected-cue-ppf",
    ArcadeCard,
    TwoButton
);
archive_ppf_sync!(
    direct_pce_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256,
    "pce-zip-selected-cue-ppf",
    MemoryBase128,
    TwoButton
);

archive_ppf_sync!(
    direct_pce_multitap_cd_archive_ppf_tas_sync_config_sha256,
    "pce-7z-unique-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_archive_ppf_arcade_tas_sync_config_sha256,
    "pce-7z-unique-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_archive_ppf_memory_base_tas_sync_config_sha256,
    "pce-7z-unique-cue-ppf",
    MemoryBase128,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_archive_ppf_tas_sync_config_sha256,
    "pce-7z-selected-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_archive_ppf_arcade_tas_sync_config_sha256,
    "pce-7z-selected-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_archive_ppf_memory_base_tas_sync_config_sha256,
    "pce-7z-selected-cue-ppf",
    MemoryBase128,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_rar_ppf_tas_sync_config_sha256,
    "pce-rar-unique-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_rar_ppf_arcade_tas_sync_config_sha256,
    "pce-rar-unique-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_rar_ppf_memory_base_tas_sync_config_sha256,
    "pce-rar-unique-cue-ppf",
    MemoryBase128,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_rar_ppf_tas_sync_config_sha256,
    "pce-rar-selected-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_rar_ppf_arcade_tas_sync_config_sha256,
    "pce-rar-selected-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_rar_ppf_memory_base_tas_sync_config_sha256,
    "pce-rar-selected-cue-ppf",
    MemoryBase128,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_zip_ppf_tas_sync_config_sha256,
    "pce-zip-unique-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_zip_ppf_arcade_tas_sync_config_sha256,
    "pce-zip-unique-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_zip_ppf_memory_base_tas_sync_config_sha256,
    "pce-zip-unique-cue-ppf",
    MemoryBase128,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_zip_ppf_tas_sync_config_sha256,
    "pce-zip-selected-cue-ppf",
    None,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_zip_ppf_arcade_tas_sync_config_sha256,
    "pce-zip-selected-cue-ppf",
    ArcadeCard,
    Multitap
);
archive_ppf_sync!(
    direct_pce_multitap_cd_selected_zip_ppf_memory_base_tas_sync_config_sha256,
    "pce-zip-selected-cue-ppf",
    MemoryBase128,
    Multitap
);
