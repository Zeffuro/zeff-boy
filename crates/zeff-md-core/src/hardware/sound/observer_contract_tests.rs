use super::schedule::{PendingSound, SoundInterval, SoundSchedule};
use super::*;
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_z80::Cpu;

pub(super) const FENCES: [u64; 10] = [60, 105, 150, 210, 255, 300, 360, 375, 420, 465];

pub(super) fn cartridge() -> Cartridge {
    let mut bytes = vec![0; 16];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    Cartridge::new(bytes).unwrap()
}

pub(super) fn memory() -> SoundMemory {
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..8].copy_from_slice(&[0x31, 2, 1, 1, 0x34, 0x12, 0xc5, 0x76]);
    memory
}

pub(super) fn signature(
    cpu: &Cpu,
    memory: &SoundMemory,
    schedule: &SoundSchedule,
    audio: &Audio,
) -> Vec<u8> {
    let mut writer = StateWriter::new();
    memory.write_state(&mut writer).unwrap();
    schedule.write_state(cpu, &mut writer).unwrap();
    let mut bytes = writer.into_bytes();
    bytes.extend_from_slice(format!("{cpu:?}|{memory:?}|{schedule:?}|{audio:?}").as_bytes());
    bytes
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_neutral_fixture_traits_moves_codec_and_push() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<SoundRam>();
    send_sync::<SoundMemory>();
    send_sync::<PendingSound>();
    send_sync::<SoundSchedule>();
    #[cfg(not(feature = "sound-shadow-observer"))]
    assert_eq!(
        std::mem::size_of::<SoundRam>(),
        std::mem::size_of::<std::sync::Arc<[u8; SOUND_RAM_SIZE]>>()
    );
    let mut cartridge = cartridge();
    let mut memory = memory();
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut audio = Audio::default();
    let mut digest = Sha256::new();
    let mut cursor = 0;
    for deadline in FENCES {
        schedule.advance(
            &mut cpu,
            &mut memory,
            &mut cartridge,
            &mut [],
            SoundInterval::new(cursor..deadline, &mut audio),
        );
        assert!(schedule.fault.is_none());
        assert!(schedule.pending.is_none());
        digest.update(signature(&cpu, &memory, &schedule, &audio));
        cursor = deadline;
    }
    assert_eq!(&memory.ram[0x100..0x102], &[0x34, 0x12]);
    assert_eq!(cpu.cycles(), 31);
    let pending = PendingSound::prepare(&cpu, &memory, &cartridge, 465).unwrap();
    let copy = pending.clone();
    assert_eq!(pending, copy);
    assert_eq!(format!("{pending:?}"), format!("{copy:?}"));
    let mut boxed = Some(Box::new(copy));
    let moved = *boxed.take().unwrap();
    assert_eq!(pending, moved);
    let memory_copy = memory.clone();
    assert_eq!(memory, memory_copy);
    assert_eq!(format!("{memory:?}"), format!("{memory_copy:?}"));
    let mut writer = StateWriter::new();
    memory.write_state(&mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut reader = StateReader::new(&bytes);
    assert_eq!(SoundMemory::read_state(&mut reader).unwrap(), memory);
    assert!(reader.is_exhausted());
    digest.update(format!("{pending:?}|{moved:?}").as_bytes());
    emit(format!(
        "OBSERVER_NEUTRAL_FIXTURE={:02x?}",
        digest.finalize().to_vec()
    ));
    emit(format!(
        "OBSERVER_LAYOUT ram={}/{} pending={}/{}",
        std::mem::size_of::<SoundRam>(),
        std::mem::align_of::<SoundRam>(),
        std::mem::size_of::<PendingSound>(),
        std::mem::align_of::<PendingSound>()
    ));
}

pub(super) fn whole_versions() -> Vec<u8> {
    use crate::cartridge::sf001::{Revision, tests::rom};
    use crate::{Emulator, LoadConfig, Region};
    let mut digest = Sha256::new();
    for profile in 0..3 {
        for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
            let bytes = match profile {
                0 => cartridge().bytes.to_vec(),
                1 => rom(Revision::HiddenSave),
                _ => rom(Revision::UpperSave),
            };
            let config = LoadConfig {
                region,
                concurrent_sound: true,
            };
            let source = Emulator::new(bytes.clone(), config).unwrap();
            let raw = source.encode_state().unwrap();
            let frontend = source.encode_frontend_state().unwrap();
            assert_eq!(&raw[..8], b"ZMDRAW01");
            assert_eq!(
                u32::from_le_bytes(raw[8..12].try_into().unwrap()),
                crate::save_state::STATE_VERSION
            );
            assert_eq!(&frontend[..12], b"ZMDFRONT\x01\0\0\0");
            let mut restored = Emulator::new(bytes, config).unwrap();
            restored.decode_state(&raw).unwrap();
            assert_eq!(restored.encode_state().unwrap(), raw);
            restored.decode_frontend_state(&frontend).unwrap();
            assert_eq!(restored.encode_frontend_state().unwrap(), frontend);
            assert_eq!(source, source.clone());
            digest.update(raw);
            digest.update(frontend);
        }
    }
    digest.finalize().to_vec()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn observer_neutral_whole_raw20_raw21_frontend1() {
    emit(format!("OBSERVER_WHOLE_VERSIONS={:02x?}", whole_versions()));
}

pub(super) fn emit(message: String) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::console_log!("{message}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{message}");
}
