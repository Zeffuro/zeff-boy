use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
};
use zeff_md_core::{ControllerKind, Region, TimingModel};

pub(crate) const ONE: [u16; 6] = [0x0101, 0x0402, 0x0101, 0x0402, 0x0101, 0x0402];
pub(crate) const TWO: [u16; 6] = [0x080a, 0x0241, 0x080a, 0x0241, 0x080a, 0x0241];
pub(crate) fn words(kind: ControllerKind) -> ([u16; 6], [u16; 6]) {
    if kind == ControllerKind::ThreeButton {
        return (ONE, TWO);
    }
    (
        [0x0115, 0x04a2, 0x0115, 0x04a2, 0x0115, 0x04a2],
        [0x082e, 0x02d1, 0x082e, 0x02d1, 0x082e, 0x02d1],
    )
}
pub(crate) const EXTRA_PINS: [[u8; 4]; 2] = [[0x63, 0x56, 0x3f, 0x1f], [0x5c, 0x69, 0x3f, 0x2f]];
pub(crate) const PINS: [[u8; 4]; 2] = [[0x67, 0x5d, 0x33, 0x11], [0x5e, 0x6b, 0x32, 0x23]];

pub(crate) fn rom() -> Vec<u8> {
    let mut program = Vec::new();
    let write = |program: &mut Vec<u16>, address: u32, value: u8| {
        program.extend([
            0x13fc,
            u16::from(value),
            (address >> 16) as u16,
            address as u16,
        ]);
    };
    for (address, value) in [
        (0xc00011, 0x85),
        (0xc00011, 0x06),
        (0xc00011, 0x90),
        (0xa10009, 0x40),
        (0xa1000b, 0x40),
    ] {
        write(&mut program, address, value);
    }
    let loop_pc = 0x200 + program.len() * 2;
    write(&mut program, 0xa10003, 0);
    write(&mut program, 0xa10005, 0);
    program.extend([0x343c, 0x07ff, 0x51ca, 0xfffe]);
    for (level, offset) in [
        (0x40, Some(0)),
        (0, Some(2)),
        (0x40, None),
        (0, None),
        (0x40, Some(4)),
        (0, Some(6)),
    ] {
        write(&mut program, 0xa10003, level);
        write(&mut program, 0xa10005, level);
        let Some(offset) = offset else {
            continue;
        };
        for port in 0..2u16 {
            program.extend([0x1039 | (port << 9), 0x00a1, 3 + port * 2]);
            program.extend([0x13c0 | port, 0x00ff, 0x0100 + offset + port]);
        }
    }
    program.extend([0x4ef9, 0, loop_pc as u16]);
    let mut rom = vec![0; 0x4000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = b'U';
    for (index, word) in program.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

pub(crate) fn config(
    region: Region,
    timing: TimingModel,
    kind: ControllerKind,
) -> BackendLoadConfig {
    BackendLoadConfig {
        sample_rate: Some(48_000),
        md_region: Some(region),
        md_timing: timing,
        md_controller: kind,
        md_load_save_ram: false,
        md_host_persistence: true,
        ..Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn loaded(
    config: BackendLoadConfig,
) -> (crate::test_support::TestDirectory, EmuBackend) {
    let directory = crate::test_support::test_directory("md-netplay").unwrap();
    let path = directory.path().join("actual.md");
    std::fs::write(&path, rom()).unwrap();
    let backend = load_backend_from_rom_source(ActiveSystem::Md, &path, &path, None, config)
        .unwrap()
        .backend;
    (directory, backend)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn loaded(mut config: BackendLoadConfig) -> EmuBackend {
    let bytes = rom();
    config.netplay_browser_media = crate::emu_backend::loader::NetplayRomMedia::browser(&bytes);
    let path = std::path::Path::new("actual.md");
    load_backend_from_rom_source(ActiveSystem::Md, path, path, Some(bytes), config)
        .unwrap()
        .backend
}

pub(crate) fn md(backend: &EmuBackend) -> &crate::emu_backend::md::MdBackend {
    let EmuBackend::Md(md) = backend else {
        panic!("MD fixture lost its backend")
    };
    md
}
pub(crate) fn bits(audio: &[f32]) -> Vec<u32> {
    audio.iter().map(|sample| sample.to_bits()).collect()
}

pub(crate) struct Baseline {
    address: usize,
    raw: Vec<u8>,
    core: zeff_md_core::Emulator,
    hashes: zeff_md_core::emulator::rollback::MdRollbackHashes,
    publication: bool,
}
impl Baseline {
    pub(crate) fn capture(backend: &EmuBackend) -> Self {
        let md = md(backend);
        let config =
            zeff_md_core::emulator::rollback::MdRollbackConfig::capture(md.core()).unwrap();
        Self {
            address: md.core() as *const _ as usize,
            raw: backend.encode_state_bytes().unwrap(),
            core: md.core().clone(),
            hashes: config.boundary_hashes(md.core()).unwrap(),
            publication: md.host_persistence_enabled(),
        }
    }
    pub(crate) fn verify(&self, backend: &EmuBackend) {
        let md = md(backend);
        assert!(!md.rollback_owned());
        assert_eq!(md.host_persistence_enabled(), self.publication);
        self.verify_original(backend);
    }
    pub(crate) fn verify_restoring(&self, backend: &mut EmuBackend) {
        assert!(md(backend).rollback_owned());
        assert!(!md(backend).host_persistence_enabled());
        self.verify_original(backend);
        assert!(backend.flush_battery_sram().is_err());
        let EmuBackend::Md(md) = backend else {
            unreachable!()
        };
        assert!(matches!(
            md.test_normal_core_mut(),
            Err(crate::emu_backend::md::MdOwnershipError::Restoring)
        ));
        assert!(matches!(
            md.arm_start_failure_for_test(
                crate::netplay::session::md_start_failure::Stage::AbortVerification
            ),
            Err(crate::emu_backend::md::MdOwnershipError::Restoring)
        ));
    }
    fn verify_original(&self, backend: &EmuBackend) {
        let md = md(backend);
        assert_eq!(md.core() as *const _ as usize, self.address);
        assert_eq!(backend.encode_state_bytes().unwrap(), self.raw);
        assert!(md.core() == &self.core);
        let config =
            zeff_md_core::emulator::rollback::MdRollbackConfig::capture(md.core()).unwrap();
        assert_eq!(config.boundary_hashes(md.core()).unwrap(), self.hashes);
        assert_eq!(
            bits(&md.core().clone().drain_audio_samples()),
            bits(&self.core.clone().drain_audio_samples())
        );
    }
}
