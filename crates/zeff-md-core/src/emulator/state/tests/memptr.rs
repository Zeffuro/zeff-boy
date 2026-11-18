use super::*;
use crate::hardware::sound::SoundFault;
use zeff_z80::{Cpu, MemptrProfile, ResetState};

pub(super) fn roundtrip() {
    let mut rom = vec![0; 0x200];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    rom[0x100..0x104].copy_from_slice(&[0x4e, 0x72, 0x27, 0]);
    let legacy = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
    )
    .unwrap();
    let old = legacy.encode_state().unwrap();
    assert_eq!(&old[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
    let mut source = legacy.clone();
    source.sound_cpu = Cpu::new_with_memptr_profile(ResetState::default(), MemptrProfile::Nmos);
    assert!(source.bus.sound.reset_asserted);
    source.step_instruction().unwrap();
    assert!(source.main_cpu_stopped());
    assert_eq!(source.sound_cpu.memptr(), None);
    source.bus.sound.request(true);
    source.bus.sound.set_reset(false);
    source.step_instruction().unwrap();
    assert!(source.bus.sound.granted);
    assert_eq!(
        source.step_sound_instruction(),
        Err(SoundFault::BusRequested)
    );
    assert_eq!(source.sound_cpu.memptr(), None);
    source.bus.sound.request(false);
    source.bus.sound.ram[..4].copy_from_slice(&[0x3a, 0xff, 0x27, 0x76]);
    let before_sound = source.bus.now.get();
    let main = (source.registers(), source.cpu_clock());
    let instruction = source.step_sound_instruction().unwrap();
    assert_eq!((instruction.opcode, instruction.cycles), (0x3a, 13));
    assert_eq!(
        source.bus.now.get(),
        (before_sound / crate::constants::SOUND_CPU_DIVISOR + 13)
            * crate::constants::SOUND_CPU_DIVISOR
    );
    assert_eq!((source.registers(), source.cpu_clock()), main);
    assert_eq!(source.sound_cpu.memptr(), Some(0x2800));
    let saved = source.encode_state().unwrap();
    assert_eq!(
        &saved[8..12],
        &crate::save_state::STATE_VERSION.to_le_bytes()
    );
    let mut restored = legacy.clone();
    restored.decode_state(&saved).unwrap();
    assert_eq!(restored.sound_cpu, source.sound_cpu);
    assert_eq!(restored.sound_schedule, source.sound_schedule);
    assert_eq!(restored.encode_state().unwrap(), saved);
    let before = restored.clone();
    let mut unsupported = saved.clone();
    unsupported[..8].copy_from_slice(b"ZMDSTATE");
    assert!(restored.decode_state(&unsupported).is_err());
    assert_eq!(restored, before);
    for version in [0_u32, crate::save_state::STATE_VERSION + 1] {
        let mut unsupported = saved.clone();
        unsupported[8..12].copy_from_slice(&version.to_le_bytes());
        assert!(restored.decode_state(&unsupported).is_err());
        assert_eq!(restored, before);
    }
    restored.decode_state(&old).unwrap();
    assert_eq!(restored.sound_cpu.memptr_profile(), MemptrProfile::Legacy);
    assert_eq!(restored.sound_cpu.memptr(), None);
    assert_eq!(restored.encode_state().unwrap(), old);
    assert_eq!(
        (&restored.sound_cpu, &restored.sound_schedule),
        (&legacy.sound_cpu, &legacy.sound_schedule)
    );
}
