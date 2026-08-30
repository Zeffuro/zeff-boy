use std::io::{Cursor, Read, Write};

use zeff_emu_common::media::{MediaEvent, MediaObjectId, MediaSlotId};
use zeff_emu_common::replay::{
    ReplayEvent, ReplayGameBoyLinkEvent, ReplayGameBoyLinkState, ReplayWonderSwanLinkEvent,
    decode_replay_event_stream,
};

use super::super::*;
use super::project;

#[test]
fn decoder_rejects_corrupt_critical_entry() {
    let bytes = project().encode().unwrap();
    let corrupted = rewrite_zip(&bytes, |name, bytes| {
        if name == "start_state.bin" {
            bytes[0] ^= 0xFF;
        }
    });
    let error = TasProject::decode(&corrupted).unwrap_err().to_string();
    assert!(error.contains("SHA-256"), "error was: {error}");
}

#[test]
fn decoder_rejects_duplicate_and_unsafe_entries() {
    let duplicate = zip_with_duplicate_name();
    assert!(TasProject::decode(&duplicate).is_err());

    let traversal = zip_entries(&[
        ("manifest.json", b"{}"),
        ("integrity.json", b"{}"),
        ("start_state.bin", b""),
        ("../escape", b"bad"),
    ]);
    assert!(TasProject::decode(&traversal).is_err());
}

#[test]
fn event_stream_canonicalizes_and_rejects_trailing_bytes() {
    let stream = zeff_emu_common::replay::encode_replay_event_stream(&[
        ReplayEvent::FdsDiskSide { frame: 2, side: 0 },
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
    ])
    .unwrap();
    let decoded = decode_replay_event_stream(&stream).unwrap();
    assert_eq!(decoded[0].frame(), 1);

    let mut trailing = stream;
    trailing.push(0);
    assert!(decode_replay_event_stream(&trailing).is_err());
}

#[test]
fn event_stream_roundtrips_every_current_event_domain() {
    let state = ReplayGameBoyLinkState {
        peer_present: false,
        pending_master_byte: None,
        pending_master_response: None,
        pending_master_completion_ready: false,
        queued_master_action: None,
        pending_passive_completion: None,
        serial_generation: 0,
    };
    let events = vec![
        ReplayEvent::FdsDiskSide { frame: 1, side: 1 },
        ReplayEvent::Media {
            frame: 1,
            sequence: 1,
            event: MediaEvent::Insert {
                slot: MediaSlotId::new("drive"),
                media_id: MediaObjectId::new("disc"),
                side: Some(0),
                write_protected: true,
            },
        },
        ReplayEvent::GameBoyLinkState { frame: 2, state },
        ReplayEvent::GameBoyLinkStateAtTick {
            frame: 3,
            tick: 10,
            state,
        },
        ReplayEvent::GameBoyLink {
            frame: 3,
            tick: 20,
            event: ReplayGameBoyLinkEvent::LocalMasterStart {
                transfer_id: 1,
                clock_period_t_cycles: 512,
                out_byte: 0x34,
                serial_generation: 1,
            },
        },
        ReplayEvent::WonderSwanLink {
            frame: 4,
            session_cycle: 30,
            event: ReplayWonderSwanLinkEvent::RemoteByte {
                generation: 1,
                baud_bps: 9_600,
                byte: 0x56,
            },
        },
    ];
    let bytes = zeff_emu_common::replay::encode_replay_event_stream(&events).unwrap();
    assert_eq!(decode_replay_event_stream(&bytes).unwrap(), events);
}

fn rewrite_zip(bytes: &[u8], mut edit: impl FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        edit(&name, &mut bytes);
        entries.push((name, bytes));
    }

    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (name, bytes) in entries {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn zip_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn zip_with_duplicate_name() -> Vec<u8> {
    let mut bytes = zip_entries(&[
        ("branches/branch-a/events.bin", b"a"),
        ("branches/branch-b/events.bin", b"b"),
    ]);
    let source = b"branches/branch-b/events.bin";
    let replacement = b"branches/branch-a/events.bin";
    let mut replacements = 0;
    for offset in 0..=bytes.len() - source.len() {
        if bytes[offset..].starts_with(source) {
            bytes[offset..offset + source.len()].copy_from_slice(replacement);
            replacements += 1;
        }
    }
    assert!(replacements >= 2);
    bytes
}
