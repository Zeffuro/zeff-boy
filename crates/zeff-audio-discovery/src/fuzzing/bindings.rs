use crate::drivers::{CodeInventory, CodePointerDisposition};

pub(super) fn check(code: &CodeInventory, bytes: &[u8]) {
    let mut count = 0;
    let mut edges = 0;
    for consumer in &code.selector_consumers {
        for record in &consumer.records {
            for stream in &record.streams {
                if let Some(edge) = &stream.conditional_head_command_edge {
                    edges += 1;
                    for span in [edge.head_span, edge.dispatch_row_span, edge.handler_span] {
                        super::source_span(span.into(), bytes.len());
                    }
                    assert_eq!(edge.head_span.byte_len, 1);
                    let row = edge.dispatch_row_span.offset as usize;
                    assert_eq!(
                        u16::from_le_bytes([bytes[row], bytes[row + 1]]),
                        edge.handler_cpu_address
                    );
                    assert!(stream.fetch_binding.is_some());
                    assert_eq!(stream.target_span, Some(edge.head_span));
                    assert!(edge.head_byte >= 0x80);
                    assert_eq!(edge.operand_count, 8);
                    assert_eq!(edge.handler_span.byte_len, 15);
                    assert_eq!(edge.dispatch_row_span.byte_len, 2);
                    assert_eq!(
                        edge.destination_start.checked_add(7),
                        Some(edge.destination_end_inclusive)
                    );
                    assert!((0x200..=0x7ff).contains(&edge.destination_start));
                    assert!(edge.destination_end_inclusive <= 0x7ff);
                    assert_eq!(bytes[edge.head_span.offset as usize], edge.head_byte);
                    for evidence in &edge.evidence {
                        super::source_span(evidence.span.into(), bytes.len());
                        let at = evidence.span.offset as usize;
                        let end = at + evidence.span.byte_len as usize;
                        assert_eq!(evidence.sha256, zeff_firmware::sha256_hex(&bytes[at..end]));
                    }
                }
                let Some(binding) = &stream.fetch_binding else {
                    continue;
                };
                count += 1;
                assert_ne!(record.header & 0x80, 0);
                assert_eq!(record.streams.len(), 3);
                assert_eq!(stream.disposition, CodePointerDisposition::Unparsed);
                assert!(stream.target_span.is_some());
                assert!((0x200..0x7ff).contains(&binding.state_pointer_address));
                assert!(
                    code.command_dispatches
                        .iter()
                        .flat_map(|d| &d.fetches)
                        .any(|f| f.cpu_address == binding.fetch_cpu_address
                            && f.span == binding.fetch_span)
                );
                for span in [
                    binding.scheduler_span,
                    binding.consumer_entry_span,
                    binding.fetch_span,
                ] {
                    super::source_span(span.into(), bytes.len());
                    assert_ne!(span.byte_len, 0);
                }
                assert!(!binding.evidence.is_empty());
                for evidence in &binding.evidence {
                    super::source_span(evidence.span.into(), bytes.len());
                    let at = evidence.span.offset as usize;
                    let end = at + evidence.span.byte_len as usize;
                    assert_eq!(evidence.sha256, zeff_firmware::sha256_hex(&bytes[at..end]));
                }
            }
        }
    }
    assert!(count <= 48);
    assert!(edges <= 48);
}
