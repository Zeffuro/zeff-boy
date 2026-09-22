use crate::drivers::{CodePointerDisposition, CodeSelectorConsumer};

pub(super) fn check(consumer: &CodeSelectorConsumer, bytes: &[u8]) {
    assert!(consumer.records.len() <= consumer.pointers.len());
    let mut selectors = std::collections::BTreeSet::new();
    for record in &consumer.records {
        assert!(selectors.insert(record.raw_selector));
        let pointer = consumer
            .pointers
            .iter()
            .find(|pointer| pointer.raw_selector == record.raw_selector)
            .unwrap();
        assert_eq!(pointer.disposition, CodePointerDisposition::Unparsed);
        assert_eq!(
            pointer.target_span.unwrap().offset,
            record.prefix_span.offset
        );
        super::source_span(record.prefix_span.into(), bytes.len());
        let at = record.prefix_span.offset as usize;
        assert_eq!(record.header, bytes[at]);
        let triple = record.header & 0x80 != 0;
        assert_eq!(record.prefix_span.byte_len, if triple { 8 } else { 3 });
        assert_eq!(record.streams.len(), if triple { 3 } else { 1 });
        for (index, stream) in record.streams.iter().enumerate() {
            super::source_span(stream.entry_span.into(), bytes.len());
            let offset = if triple { 2 + index * 2 } else { 1 };
            assert_eq!(stream.entry_span.offset as usize, at + offset);
            assert_eq!(stream.entry_span.byte_len, 2);
            assert_eq!(
                stream.target_cpu_address,
                u16::from_le_bytes([bytes[at + offset], bytes[at + offset + 1]])
            );
            assert_eq!(
                stream.target_span.is_none(),
                stream.disposition == CodePointerDisposition::Unmapped
            );
            if let Some(span) = stream.target_span {
                super::source_span(span.into(), bytes.len());
                assert_eq!(span.byte_len, 1);
            }
        }
        assert!(!record.evidence.is_empty());
        for evidence in &record.evidence {
            super::source_span(evidence.span.into(), bytes.len());
            let at = evidence.span.offset as usize;
            let end = at + evidence.span.byte_len as usize;
            assert_eq!(evidence.sha256, zeff_firmware::sha256_hex(&bytes[at..end]));
        }
    }
}
