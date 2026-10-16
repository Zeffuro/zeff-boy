use super::*;
use crate::hardware::vdp::Vdp;
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[test]
fn hardware_irq_is_sampled_into_status_replay_without_changing_manual_line() {
    let mut rom = vec![0; 16];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 0, 8]);
    let cartridge = Cartridge::new(rom).unwrap();
    let mut memory = SoundMemory::default();
    memory.set_reset(false);
    memory.ram[..4].copy_from_slice(&[0x3a, 0x00, 0x40, 0x76]);
    let vdp = Vdp::bootstrap_mode5();
    let irq = vdp
        .interrupts()
        .sound_interrupt(*vdp.registers(), crate::Region::NtscUs);
    let at = 224 * crate::constants::VDP_LINE_TICKS + 40;
    let mut audio = Audio::default();
    audio.advance_to(at);
    let mut cpu = Cpu::new();
    let mut schedule = SoundSchedule::default();
    let mut interval = SoundInterval::new(at..at + 151, &mut audio);
    interval.sound_irq = Some(irq);
    schedule.advance(&mut cpu, &mut memory, &cartridge, &mut [], interval);
    let pending = schedule.pending.as_ref().unwrap();
    assert!(pending.memory.irq_asserted);
    assert!(pending.replay.unwrap().irq_asserted);
    assert!(!memory.irq_asserted);
    let mut writer = StateWriter::new();
    schedule.write_state(&cpu, &mut writer).unwrap();
    let bytes = writer.into_bytes();
    let mut restored = SoundSchedule::default();
    let mut restored_cpu = Cpu::new();
    restored
        .read_state(&mut restored_cpu, &mut StateReader::new(&bytes))
        .unwrap();
    assert_eq!(restored, schedule);
    assert_eq!(restored_cpu, cpu);
    let mut pending = schedule.pending.take().unwrap();
    pending
        .refresh_opn2(&cpu, &memory, &cartridge, &audio)
        .unwrap();
    assert!(pending.memory.irq_asserted);
    assert!(!memory.irq_asserted);
}
