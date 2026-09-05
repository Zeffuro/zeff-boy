use super::*;

#[test]
fn direct_pce_cd_chd_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        chd_loader_and_project("tas-control-direct-pce-cd-chd");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_iso_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        iso_loader_and_project("tas-control-direct-pce-cd-iso");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_ppf_memory_base_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _memory_base_catalog) =
        ppf_loader_and_project("tas-control-direct-pce-cd-ppf");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Enabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Disabled,
    );
}

#[test]
fn direct_pce_cd_ppf_arcade_worker_executes_advances_caches_and_rolls_back() {
    let (_directory, loader, project, _arcade_catalog) =
        ppf_arcade_loader_and_project("tas-control-direct-pce-cd-ppf-arcade");
    execute_direct_pce_cd_worker(
        loader,
        project,
        zeff_pce_core::hardware::PceMemoryBaseMode::Disabled,
        zeff_pce_core::hardware::PceArcadeCardMode::Enabled,
    );
}
