use crate::drivers::CodeInventory;

pub(super) fn check(code: &CodeInventory, bytes: &[u8]) {
    assert!(code.command_dispatches.len() <= 16);
    let mut entries = std::collections::BTreeSet::new();
    let mut fetches = std::collections::BTreeSet::new();
    for dispatch in &code.command_dispatches {
        assert!(entries.insert(dispatch.entry_cpu_address));
        super::source_span(dispatch.entry_span.into(), bytes.len());
        assert!(dispatch.target_pointer_address < 255);
        assert!(!dispatch.fetches.is_empty());
        assert!(!dispatch.evidence.is_empty());
        for evidence in &dispatch.evidence {
            super::source_span(evidence.span.into(), bytes.len());
            let start = evidence.span.offset as usize;
            let end = start + evidence.span.byte_len as usize;
            assert_eq!(
                evidence.sha256,
                zeff_firmware::sha256_hex(&bytes[start..end])
            );
        }
        for fetch in &dispatch.fetches {
            assert!(fetches.insert(fetch.cpu_address));
            assert!(code.calls.contains(&fetch.audio_call));
            super::source_span(fetch.span.into(), bytes.len());
            super::source_span(fetch.call_span.into(), bytes.len());
            let at = fetch.span.offset as usize;
            assert_eq!(
                &bytes[at..at + 3],
                &[0xb1, fetch.source_pointer_address, 0x10]
            );
            assert_eq!(fetch.call_span.byte_len, 3);
            let call = fetch.call_span.offset as usize;
            assert_eq!(bytes[call], 0x20);
            assert_eq!(
                u16::from_le_bytes([bytes[call + 1], bytes[call + 2]]),
                dispatch.entry_cpu_address
            );
            assert_eq!(
                fetch.event_cpu_address,
                fetch
                    .cpu_address
                    .checked_add(4)
                    .unwrap()
                    .checked_add_signed(i16::from(bytes[at + 3] as i8))
                    .unwrap()
            );
            assert!(fetch.event_cpu_address >= fetch.call_cpu_address + 3);
        }
    }
    assert!(fetches.len() <= 64);
}
