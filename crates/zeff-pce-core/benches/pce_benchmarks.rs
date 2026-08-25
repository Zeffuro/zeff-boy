use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use zeff_pce_core::hardware::{
    CD_RAW_SECTOR_BYTES, CDROM2_REGISTER_START, CdDisc, CdRom2, CdScsiPhase, CdTrack, CdTrackMode,
    PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS, PROVISIONAL_CDROM2_PHASE_TICKS,
    PROVISIONAL_CDROM2_SELECTION_TICKS, PceMachine, save_state,
};

const WARMUP_FRAMES: usize = 16;

fn build_rom() -> Vec<u8> {
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    rom
}

fn create_machine() -> PceMachine {
    let mut machine = PceMachine::new(build_rom()).expect("benchmark ROM should load");
    machine.set_sample_generation_enabled(false);
    machine
}

fn warm_up(machine: &mut PceMachine) {
    for _ in 0..WARMUP_FRAMES {
        machine
            .run_until_frame()
            .expect("benchmark frame should complete");
    }
}

fn bench_event_boundary(c: &mut Criterion) {
    let mut machine = create_machine();
    warm_up(&mut machine);

    c.bench_function("pce_event_boundary", |b| {
        b.iter(|| black_box(machine.step_boundary().expect("machine should step")));
    });
}

fn bench_frame_step(c: &mut Criterion) {
    let mut machine = create_machine();
    warm_up(&mut machine);

    c.bench_function("pce_frame_step", |b| {
        b.iter(|| black_box(machine.run_until_frame().expect("frame should complete")));
    });
}

fn bench_state_encode(c: &mut Criterion) {
    let mut machine = create_machine();
    warm_up(&mut machine);

    c.bench_function("pce_state_encode", |b| {
        b.iter(|| black_box(save_state::encode_state(&machine).expect("state should encode")));
    });
}

fn bench_state_load(c: &mut Criterion) {
    let mut machine = create_machine();
    warm_up(&mut machine);
    let state = save_state::encode_state(&machine).expect("state should encode");

    c.bench_function("pce_state_load", |b| {
        b.iter(|| {
            save_state::decode_state(&mut machine, black_box(&state)).expect("state should load");
            black_box(&machine);
        });
    });
}

const CDDA_BENCH_TRACKS: usize = 22;
const CDDA_BENCH_SECTORS_PER_TRACK: usize = 8;
const CDDA_BENCH_BLOCK_TICKS: u64 = 37_500 * 2_048;

fn cdda_benchmark_disc() -> CdDisc {
    let mut tracks = Vec::with_capacity(CDDA_BENCH_TRACKS);
    for track_index in 0..CDDA_BENCH_TRACKS {
        let mut raw = vec![0; CDDA_BENCH_SECTORS_PER_TRACK * CD_RAW_SECTOR_BYTES];
        for (sample_index, frame) in raw.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let left = (track_index as i16)
                .wrapping_mul(257)
                .wrapping_add(sample_index as i16);
            frame[..2].copy_from_slice(&left.to_le_bytes());
            frame[2..].copy_from_slice(&(-left).to_le_bytes());
        }
        tracks.push(
            CdTrack::from_index1_data(
                (track_index + 1) as u8,
                0,
                None,
                (track_index * CDDA_BENCH_SECTORS_PER_TRACK) as u32,
                CdTrackMode::Audio,
                raw,
            )
            .expect("benchmark audio track should be valid"),
        );
    }
    CdDisc::new(tracks).expect("benchmark disc should be valid")
}

fn select(cd: &mut CdRom2) {
    assert!(cd.write_physical(CDROM2_REGISTER_START, 0));
    assert_eq!(cd.phase(), CdScsiPhase::Selection);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_SELECTION_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Command);
}

fn send_command(cd: &mut CdRom2, command: &[u8]) {
    for (index, &byte) in command.iter().enumerate() {
        assert!(cd.write_physical(CDROM2_REGISTER_START + 1, byte));
        assert!(cd.write_physical(CDROM2_REGISTER_START + 2, 0x80));
        assert!(cd.write_physical(CDROM2_REGISTER_START + 2, 0));
        if index + 1 != command.len() {
            cd.advance_master_ticks(PROVISIONAL_CDROM2_NEXT_REQUEST_TICKS);
        }
    }
}

fn acknowledge(cd: &mut CdRom2) {
    let _ = cd.read_physical(CDROM2_REGISTER_START + 1);
    assert!(cd.write_physical(CDROM2_REGISTER_START + 2, 0x80));
    assert!(cd.write_physical(CDROM2_REGISTER_START + 2, 0));
}

fn looping_cdda_cdrom() -> CdRom2 {
    let mut cd = CdRom2::new(cdda_benchmark_disc());
    select(&mut cd);
    send_command(&mut cd, &[0xD8, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS * 2);
    acknowledge(&mut cd);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    acknowledge(&mut cd);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::BusFree);

    let leadout = (CDDA_BENCH_TRACKS * CDDA_BENCH_SECTORS_PER_TRACK) as u32;
    let [_, high, middle, low] = leadout.to_be_bytes();
    select(&mut cd);
    send_command(&mut cd, &[0xD9, 1, 0, high, middle, low, 0, 0, 0, 0]);
    cd.advance_master_ticks(PROVISIONAL_CDROM2_PHASE_TICKS);
    assert_eq!(cd.phase(), CdScsiPhase::Busy);
    cd
}

fn bench_cdda_transport_and_mix(c: &mut Criterion) {
    let mut cd = looping_cdda_cdrom();
    let mut output = vec![0.0; 77 * 2_048 * 2];
    c.bench_function("pce_cd_cdda_transport_and_mix", |b| {
        b.iter(|| {
            cd.advance_master_ticks(CDDA_BENCH_BLOCK_TICKS);
            cd.mix_audio_samples_into(&mut output);
            black_box(output[0]);
        });
    });
}

criterion_group!(
    benchmarks,
    bench_event_boundary,
    bench_frame_step,
    bench_state_encode,
    bench_state_load,
    bench_cdda_transport_and_mix,
);
criterion_main!(benchmarks);
