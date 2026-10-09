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
const MAPPERS: [u8; 5] = [0, 1, 3, 4, 34];

// These fixture receipts do not qualify artifacts, browser engines or network sessions.
const NATIVE_RECEIPTS: [&str; 30] = [
    "841ca74ae4daa2a47680bf01cf2b5b6cad10a801e95e95373b7dc39f0f4106f4",
    "96d1c5f0328c1ff8ed09ec42b0108d3cc056a22203c28ba14b043b06c40875e4",
    "84f4f3b0af87cddbff8c3def2ebf02361134cba6ab00cc04465aaf5918e23f22",
    "f3104aaae020d1b0e24d8ed65fb5044fa5a8b1879f0688902129e74cac632110",
    "8177a08d77eec6aaf08bd46ce6a9a21cbce729315b6b43f6ffaa4c17d48c65eb",
    "680783bcb8820b62f024c86a2d1db51a2bb01f0c807b81cebce6b31545120fa7",
    "c51578c6e880a2ced1a70a8b075e8559d55d1780b89a4638ad2307d31137cbdb",
    "cc251d085e0aaee3b38cd772001ec87d2815a8b7121ba0d7d3381b5fa7817478",
    "947ce21b11066652c8c54d8f23a7f3a2a88909cfac3b081ff00915f15668734b",
    "05ff57ff50ef19b42b403d56e3f2bff05fdfadf1c4ab87125b32f66ed09bed64",
    "c20fb0c33efef5d163f2cca97e86371579b66a5401814e7ca96d39c08e0a8dcc",
    "a5d15afb6c24eec91eed12fad5c0126c83069a1a5880f8ea40f61c2cbc026d2b",
    "a0ea3ebf7c321053898cedfd7e7f7711e96036f7a656f74c5d2c14059d53ba56",
    "6a27e49b5c94e511b3426f8d13735003fe2c2ca9c98356ae9baab803f3e92756",
    "e7c40b91db0dc85b6aaf25b614147e6ef4a8f2cfb2f56ae15ef3dda5dba03e6c",
    "5662c49a488a4f858239dbe11623fab362e2d36b6f3edac17aebea0afbb6bc45",
    "dabb4c1edb3465475aa2c8217ad301c5ace4269ea342191896e93973d4f13eb7",
    "fd3c7e4e8f545937825b6cfd4c4699253326a8631117dae834513ef77d9d756e",
    "1e7a65c0b4f938138f7fb3d17b4fd033fb6388addb2cf8f645ad6911da09fb4e",
    "7afd2dde5eca8713fcc309fa5216bda7aaf7c3c4c8ae1966fb4e75f2d9f50d45",
    "2b84f07f654026bd59fd44e934f054b9d2ee2b66a1e06ec28248e025f60a9f3c",
    "553b1ddabe08e5bd9dc16a921de5bdc2fd245479337d6f7daa77dbb34077fa39",
    "b3649397f637fe98eded62955a2e3f2eb188330f95b6d3ec3d48427fbd791cda",
    "e4f1a34115ecc15696f92e42f9f008c0868b751b22b74dbae5b405efd7bbbf4b",
    "dccc74e76caabdd0b1e363b7bc02745f9f691e024cf4cdccc4c7d3b0259664f8",
    "fb9fce6c6df2630eee7e0a11b698649b9b8989cd664528837cf0b419b5a3492b",
    "9ea549ef18211333b087479c24b60cc25bcdb8b84766f08d4cd7de0343fe9d91",
    "9fa716f453c9ac703ff2095ee0c350fef471c6492662a6060aa401dbf0cd16ca",
    "c88e9c0fe154ca8abca362c3ac657124d40533420f64a9e8910d1bb7d3178ccb",
    "52593a46dd935076446a9c750aa649ea84b10372196071efcb3558910fd673ef",
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
    if matches!(mapper, 1 | 3 | 4) {
        bytes[5] = 16;
        bytes.resize(16 + 0x8000 + 0x20000, 0);
        let chr = &mut bytes[16 + 0x8000..];
        let mut pattern = 0x53a9_71c5u32;
        for byte in chr.iter_mut() {
            pattern ^= pattern << 13;
            pattern ^= pattern >> 17;
            pattern ^= pattern << 5;
            *byte = pattern as u8;
        }
        chr.copy_within(0x4000..0x6000, 0x10000);
        chr.copy_within(0xa000..0xc000, 0x18000);
        let prg = &mut bytes[16..16 + 0x8000];
        let epilogue = prg[0x100..0x200]
            .windows(4)
            .position(|bytes| bytes == [0x68, 0xaa, 0x68, 0x40])
            .unwrap()
            + 0x100;
        let mut bank_write = Vec::new();
        match mapper {
            1 => {
                bank_write.extend_from_slice(&[0xa5, 0x00]);
                for _ in 0..5 {
                    bank_write.extend_from_slice(&[0x8d, 0x00, 0xa0, 0x4a]);
                }
            }
            3 => {
                bytes[6] &= !2;
                bytes[10] = 0;
                let prg = &mut bytes[16..16 + 0x8000];
                let mut redirected = 0;
                for instruction in prg[0x100..epilogue].windows(3) {
                    redirected += usize::from(
                        matches!(instruction[0], 0x4d | 0x8d)
                            && instruction[1] <= 1
                            && instruction[2] == 0x60,
                    );
                }
                assert_eq!(redirected, 4);
                for index in 0x100..epilogue - 2 {
                    if matches!(prg[index], 0x4d | 0x8d)
                        && prg[index + 1] <= 1
                        && prg[index + 2] == 0x60
                    {
                        prg[index + 2] = 0x02;
                    }
                }
                // The bank register write must survive CNROM's ROM bus conflict.
                prg[0x1000] = 0xff;
                bank_write.extend_from_slice(&[0xa5, 0x00, 0x8d, 0x00, 0x90]);
            }
            4 => bank_write.extend_from_slice(&[
                0xa9, 0x02, 0x8d, 0x00, 0x80, 0xa5, 0x00, 0x29, 0x7f, 0x8d, 0x01, 0x80,
            ]),
            _ => unreachable!(),
        }
        bank_write.extend_from_slice(&[0x68, 0xaa, 0x68, 0x40]);
        bytes[16 + epilogue..16 + epilogue + bank_write.len()].copy_from_slice(&bank_write);
    }
    bytes
}

fn backend(bytes: &[u8]) -> EmuBackend {
    let mut core = Emulator::new(bytes, 48_000.0).unwrap();
    let persistent: Vec<_> = (0..8192).map(|index| (index as u8) ^ 0x53).collect();
    if core.dump_persistent_data().is_some() {
        core.load_persistent_data(&persistent).unwrap();
        assert_eq!(core.dump_persistent_data().unwrap(), persistent);
    } else {
        let range = ram_range(core.cartridge_header().mapper_id);
        for (address, &value) in range.zip(&persistent) {
            core.cpu_write8(address, value);
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
    ram_range(nes.emu.cartridge_header().mapper_id)
        .map(|address| nes.emu.cpu_peek8(address))
        .collect()
}

fn ram_range(mapper: u16) -> std::ops::Range<u16> {
    if mapper == 3 {
        0x0200..0x0400
    } else {
        0x6000..0x8000
    }
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
    for address in [
        0x8000, 0x8100, 0xfffa, 0xfffb, 0xfffc, 0xfffd, 0xfffe, 0xffff,
    ] {
        assert_eq!(
            core(&mut subject).cpu_peek8(address),
            bytes[16 + usize::from(address - 0x8000)],
            "initial program mapping at {address:#06x}"
        );
    }
    let original = subject.encode_state_bytes().unwrap();
    if matches!(mapper, 1 | 3 | 4) {
        assert!(
            zeff_nes_core::save_state::encode_netplay_hash_state(core(&mut subject)).len() > 0xffff
        );
        let mut projected = original.clone();
        zeff_nes_core::save_state::project_replay_state_bytes(&mut projected).unwrap();
        assert!(projected.len() > 0xffff);
    }
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
        for mapper in MAPPERS {
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
        for mapper in MAPPERS {
            for delay in [0, 2] {
                println!(
                    "timing={timing} mapper={mapper} delay={delay} {}",
                    run_case(timing, mapper, delay)
                );
            }
        }
    }
}
