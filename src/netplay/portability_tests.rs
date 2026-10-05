use std::collections::BTreeMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use zeff_nes_core::emulator::Emulator;
use zeff_nes_core::emulator::rollback::{NesRollbackSession, NesRollbackSnapshot};
use zeff_nes_core::hardware::cartridge::TimingMode;
use zeff_netplay::lockstep::Player;
use zeff_netplay::rollback::{FrameInput, InputDelay, Timeline};
use zeff_netplay::wire::Message;

use crate::emu_backend::EmuBackend;
use crate::emu_backend::nes::NesBackend;

const FRAMES: u64 = 72;
const BURST: u64 = 6;
const CONFIG: [u8; 32] = [0x53; 32];

// These fixture receipts do not qualify artifacts, browser engines or network sessions.
const NATIVE_RECEIPTS: [&str; 18] = [
    "9a5d3df4fd794c36070cd9a50e7ef573e86d671400f07c7e8086c7fdee82a490",
    "5310ca96261101e4b13e2830ec48d23e269383951607438dc1e6e0e9f512fb3c",
    "6bbf06c46344f68d84b77d8999cfdaa8043db7fac9f66c4e1193f776289fbae9",
    "f54c6d264facb7da10836b6f5aad166f992de4fd776bc1f94307b35fe2368890",
    "e1ad223a52b0df5426fb1fc0a3988ca9261b9a4a2acd516d9910a91a8e2df8ca",
    "e39dc50c2de01287dbc3caa59679b6c8c2cb27d4cc4a0cf9432403bcfa0aedcd",
    "a6c91a32e51258ba64bd960977d5d15f7248902a7402b8bb4be5a68bbc1e6a6f",
    "9713b043dcb174dc67b93ec08b690523face3b73a13338209530d42aeb92bbec",
    "01b9a20c6d3858aeb5fd0909983fea7d43488c94ab1063c6e749f777fe60c71b",
    "c604c4ff156c42b2b05d020d93e7d7c601ec35d3f56cab83e5f12065eb9d1bf3",
    "6d87366c2f43211c70fe63663e8ca2ffb0150ce51ec2069c75eaac77a205462c",
    "7608826d5c71f850bf37f430d1bb325947916dec2e9675f12ba4c980b500f29d",
    "412595434d2bcc1de34ca2cfebc7ed92e58d5e150dab43890ea6748024c90273",
    "c139595266debdb2b8dc1b989d469c88f8f6226ea586f4f3ba871b7f3679a8e7",
    "c3e35f85919050bf497964edf4c85f319b0c6bd4521963fc24b6733f482305ed",
    "0ad134486b5e14e42fa03685ed160b2bda0fe8e00295b449ef1bfd89736894ea",
    "55f779bed49453dbc229b5932efdffd2fb99264c61dbc1d7711b8de635b008e5",
    "47d4a869224aec85c6b1a9ec4261f3869a92079b83fb3643f38663bb3911c1d3",
];

#[derive(Debug, PartialEq, Eq)]
struct Record {
    ports: [u16; 2],
    logical: [u8; 32],
    video: [u8; 32],
    pcm: [u8; 32],
    persistent: [u8; 32],
    samples: Vec<u32>,
}

fn media(timing: u8, mapper: u8) -> Vec<u8> {
    let mut bytes = zeff_netplay::fixture::rom();
    bytes[6] = (mapper << 4) | 2;
    bytes[7] = (mapper & 0xf0) | 0x08;
    bytes[10] = 0x70;
    bytes[12] = timing;
    bytes
}

fn backend(bytes: &[u8]) -> EmuBackend {
    let mut core = Emulator::new(bytes, 48_000.0).unwrap();
    let persistent: Vec<_> = (0..8192).map(|index| (index as u8) ^ 0x53).collect();
    if core.dump_persistent_data().is_some() {
        core.load_persistent_data(&persistent).unwrap();
        assert_eq!(core.dump_persistent_data().unwrap(), persistent);
    } else {
        for (index, &value) in persistent.iter().enumerate() {
            core.cpu_write8(0x6000 + index as u16, value);
        }
    }
    EmuBackend::Nes(Box::new(NesBackend::new(
        core,
        PathBuf::from("portability-fixture.nes"),
    )))
}

fn core(backend: &mut EmuBackend) -> &mut Emulator {
    let EmuBackend::Nes(nes) = backend else {
        unreachable!()
    };
    &mut nes.emu
}

fn buttons(sample: u64, player: usize) -> u16 {
    let shift = ((sample / 3 + player as u64 * 3) % 8) as u32;
    u16::from((1u8 << shift) ^ (sample.is_multiple_of(5) as u8 * 0x18))
}

fn ram(backend: &EmuBackend) -> Vec<u8> {
    let nes = backend.nes().unwrap();
    (0x6000..0x8000)
        .map(|address| nes.emu.cpu_peek8(address))
        .collect()
}

fn ports(frame: u64, delay: u64) -> [u16; 2] {
    if frame < delay {
        [0; 2]
    } else {
        [buttons(frame - delay, 0), buttons(frame - delay, 1)]
    }
}

fn observe(backend: &EmuBackend, input: [u16; 2], audio: Vec<f32>) -> Record {
    let checkpoint =
        super::identity::checkpoint(backend, backend.frame_count(), &audio, CONFIG).unwrap();
    let Message::Checkpoint {
        logical,
        video,
        audio: pcm,
        persistent,
        ..
    } = checkpoint
    else {
        unreachable!()
    };
    Record {
        ports: input,
        logical,
        video,
        pcm,
        persistent,
        samples: audio.into_iter().map(f32::to_bits).collect(),
    }
}

fn advance(backend: &mut EmuBackend, lease: &NesRollbackSession, input: FrameInput) -> Record {
    assert_eq!(backend.frame_count(), input.frame);
    let ports = input.ports.map(|buttons| u8::try_from(buttons).unwrap());
    let audio = lease.advance_frame(core(backend), ports).unwrap();
    observe(backend, input.ports, audio)
}

fn hash_record(digest: &mut Sha256, frame: u64, record: &Record) {
    digest.update(frame.to_le_bytes());
    digest.update(record.ports.map(|buttons| u8::try_from(buttons).unwrap()));
    for hash in [record.logical, record.video, record.pcm, record.persistent] {
        digest.update(hash);
    }
    digest.update((record.samples.len() as u64).to_le_bytes());
    for sample in &record.samples {
        digest.update(sample.to_le_bytes());
    }
}

fn pause_boundary(backend: &mut EmuBackend, lease: &NesRollbackSession) -> Record {
    let before = backend.encode_state_bytes().unwrap();
    let boundary = observe(backend, [0; 2], Vec::new());
    let snapshot = lease.capture(core(backend)).unwrap();
    for _ in 0..2 {
        for _ in 0..BURST {
            lease.advance_frame(core(backend), [0xff, 0xa5]).unwrap();
        }
        lease.restore(core(backend), &snapshot).unwrap();
        assert_eq!(backend.encode_state_bytes().unwrap(), before);
        assert_eq!(observe(backend, [0; 2], Vec::new()), boundary);
    }
    boundary
}

fn run_case(timing: u8, mapper: u8, delay: u64) -> String {
    let bytes = media(timing, mapper);
    let mut subject = backend(&bytes);
    let mut reference = backend(&bytes);
    assert_eq!(
        core(&mut subject).cartridge_header().mapper_id,
        u16::from(mapper)
    );
    let expected_timing = match timing {
        0 => TimingMode::Ntsc,
        1 => TimingMode::Pal,
        3 => TimingMode::Dendy,
        _ => unreachable!(),
    };
    assert_eq!(core(&mut subject).resolved_timing_mode(), expected_timing);
    let original = subject.encode_state_bytes().unwrap();
    let initial_sram = core(&mut subject).dump_persistent_data();
    let initial_ram = ram(&subject);
    assert_eq!(initial_sram.is_some(), mapper == 34);
    let lease = core(&mut subject).begin_rollback_session().unwrap();
    let reference_lease = core(&mut reference).begin_rollback_session().unwrap();
    let initial = lease.capture(core(&mut subject)).unwrap();
    let mut timeline = Timeline::with_delay(Player::One, InputDelay::new(delay).unwrap());
    let mut receipt = Sha256::new();
    receipt.update(b"ZeffNetplay-NES-native-wasm-fixture/v1\0");
    receipt.update([timing, mapper, delay as u8]);
    hash_record(&mut receipt, 0, &observe(&subject, [0; 2], Vec::new()));
    let mut samples = 0;
    let mut audible = false;
    let mut replayed = 0;
    for first in (0..FRAMES).step_by(BURST as usize) {
        let mut snapshots: BTreeMap<u64, NesRollbackSnapshot> = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        let mut sent = Vec::new();
        for frame in first..first + BURST {
            snapshots.insert(frame, lease.capture(core(&mut subject)).unwrap());
            let (scheduled, _) = timeline.sample_local(buttons(frame, 0)).unwrap();
            sent.push((scheduled, buttons(frame, 1)));
            let input = timeline.next_frame().unwrap().unwrap();
            outputs.insert(frame, advance(&mut subject, &lease, input));
            timeline.advance(input).unwrap();
        }
        for (frame, remote) in sent.into_iter().rev() {
            timeline.receive_remote(frame, remote).unwrap();
        }
        let correction = timeline.correction().unwrap();
        if let Some(input) = correction.first() {
            lease
                .restore(core(&mut subject), snapshots.get(&input.frame).unwrap())
                .unwrap();
            for &input in &correction {
                outputs.insert(input.frame, advance(&mut subject, &lease, input));
            }
            replayed += correction.len();
        }
        timeline.corrected(&correction).unwrap();
        assert_eq!(timeline.confirmed_frame(), first + BURST);
        assert_eq!(timeline.prediction_depth(), 0);
        for frame in first..first + BURST {
            let expected = advance(
                &mut reference,
                &reference_lease,
                FrameInput {
                    frame,
                    ports: ports(frame, delay),
                },
            );
            let actual = outputs.remove(&frame).unwrap();
            assert_eq!(
                (
                    actual.ports,
                    actual.logical,
                    actual.video,
                    actual.pcm,
                    actual.persistent
                ),
                (
                    expected.ports,
                    expected.logical,
                    expected.video,
                    expected.pcm,
                    expected.persistent
                ),
                "timing {timing}, mapper {mapper}, delay {delay}, frame {frame}"
            );
            assert!(
                actual.samples == expected.samples,
                "PCM bits differ at frame {frame}"
            );
            samples += actual.samples.len();
            audible |= actual.samples.iter().any(|&sample| sample != 0);
            hash_record(&mut receipt, frame + 1, &actual);
        }
        if matches!(first + BURST, 24 | 48) {
            let boundary = pause_boundary(&mut subject, &lease);
            assert_eq!(boundary, observe(&reference, [0; 2], Vec::new()));
            hash_record(&mut receipt, first + BURST, &boundary);
        }
    }
    assert!(samples > 50_000 && audible && replayed > 0);
    assert_ne!(ram(&subject), initial_ram);
    if initial_sram.is_some() {
        assert_ne!(core(&mut subject).dump_persistent_data(), initial_sram);
    }
    lease.restore(core(&mut subject), &initial).unwrap();
    assert_eq!(subject.encode_state_bytes().unwrap(), original);
    assert_eq!(core(&mut subject).dump_persistent_data(), initial_sram);
    assert_eq!(ram(&subject), initial_ram);
    hash_record(&mut receipt, 0, &observe(&subject, [0; 2], Vec::new()));
    drop(lease);
    assert_eq!(
        subject.load_state_from_bytes(original.clone()).unwrap(),
        zeff_emu_common::StateRestoreOutcome::Exact
    );
    assert_eq!(subject.encode_state_bytes().unwrap(), original);
    assert_eq!(core(&mut subject).dump_persistent_data(), initial_sram);
    assert_eq!(ram(&subject), initial_ram);
    const_hex::encode(receipt.finalize())
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn native_wasm_nes_portability_receipts() {
    let mut row = 0;
    for timing in [0, 1, 3] {
        for mapper in [0, 1, 34] {
            for delay in [0, 2] {
                let actual = run_case(timing, mapper, delay);
                assert_eq!(
                    actual, NATIVE_RECEIPTS[row],
                    "timing {timing}, mapper {mapper}, delay {delay}"
                );
                row += 1;
            }
        }
    }
    assert_eq!(row, NATIVE_RECEIPTS.len());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "prints fixture receipts for an independently verified native baseline"]
fn emit_native_nes_portability_receipts() {
    for timing in [0, 1, 3] {
        for mapper in [0, 1, 34] {
            for delay in [0, 2] {
                println!(
                    "timing={timing} mapper={mapper} delay={delay} {}",
                    run_case(timing, mapper, delay)
                );
            }
        }
    }
}
