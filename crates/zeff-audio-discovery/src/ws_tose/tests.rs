use super::profiles::Profile;

pub(super) const PROFILE: Profile = Profile {
    name: "ws-tose-eight-slot-synthetic",
    len: 0x20000,
    fixed: 0x14000,
    end: 0x14500,
    hash: "",
    segment: 0xf000,
    init: 0x4400,
    selector: 0x4440,
    tick: 0x4480,
    status: 0x44c0,
    slots: 0x1500,
    wave: 0x4020,
    envelope: 0x4040,
    frequency: 0x4200,
    counts: &[1],
};

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.len];
    bytes[..18].copy_from_slice(&[
        2, 0, 0, 0, 0, 0, 8, 0, 15, 0, 255, 0, 0xfd, 0, 0x30, 8, 0xb0, 0xf0,
    ]);
    bytes[0x14000..0x14006].copy_from_slice(&[255, 255, 0, 0, 0, 0]);
    bytes[0x14020..0x14030].fill(0xf0);
    bytes[0x14040..0x14042].copy_from_slice(&0x4050_u16.to_le_bytes());
    bytes[0x14050..0x14140].fill(0xff);
    let init = [
        0x33, 0xc0, 0x8e, 0xc0, 0xbf, 0x80, 0, 0xb9, 16, 0, 0xb0, 0xf0, 0xf3, 0xaa, 0xb0, 2, 0xe6,
        0x8f, 0xb0, 8, 0xe6, 0x91, 0xcb,
    ];
    bytes[0x14400..0x14400 + init.len()].copy_from_slice(&init);
    bytes[0x14440] = 0xcb;
    let tick = [
        0xb8, 0, 4, 0xe7, 0x80, 0xb0, 0xff, 0xe6, 0x88, 0xb0, 0x41, 0xe6, 0x90, 0xcb,
    ];
    bytes[0x14480..0x14480 + tick.len()].copy_from_slice(&tick);
    bytes[0x144c0] = 0xcb;
    bytes[0x1fff6..0x20000].copy_from_slice(&[1, 1, 0, 0, 1, 0, 4, 0, 0, 0]);
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    bytes.len() == PROFILE.len
        && bytes.get(PROFILE.fixed..PROFILE.end) == synthetic_rom().get(PROFILE.fixed..PROFILE.end)
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<super::WsToseSong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    super::scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn identifies_loop_and_native_contract() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].tracks[0].note_count, 1);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let prepared = super::prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(prepared.bytes.len(), bytes.len());
    assert_eq!(prepared.wait_end - prepared.wait_start, 7);
    assert_eq!(&prepared.bytes[0x1fff0..0x1fff5], &[0xea, 0, 0xe0, 0, 0xf0]);
    for span in &songs[0].mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
    }
}

#[test]
fn rejects_immediate_loop_external_branch_and_out_of_bank_read() {
    for command in [[0xb0, 0xf0], [0xa9, 0xfe], [0xac, 2]] {
        let mut bytes = synthetic_rom();
        bytes[14..16].copy_from_slice(&command);
        if command[0] == 0xac {
            bytes[16..18].copy_from_slice(&0xfffe_u16.to_le_bytes());
        }
        assert!(inventory(&bytes).is_empty());
    }
}

#[test]
fn rejects_forged_inventory_and_code_mutation() {
    let mut bytes = synthetic_rom();
    let mut songs = inventory(&bytes);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    songs[0].index = 1;
    assert!(super::prepare_rom(&bytes, &songs[0], &cancel).is_err());
    bytes[0x14480] ^= 1;
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn cancellation_and_candidate_limits_are_typed() {
    let cancel = std::sync::atomic::AtomicBool::new(true);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(
        super::scan(&synthetic_rom(), &mut Vec::new(), &mut budget, 1),
        Err(crate::ScanStop::Cancelled)
    );
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        super::scan(&synthetic_rom(), &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
}
