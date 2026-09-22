use super::*;

fn edge(row: u32, handler: u32) -> CodeConditionalHeadCommandEdge {
    CodeConditionalHeadCommandEdge {
        head_byte: 0x93,
        head_span: FileSpan {
            offset: 16,
            byte_len: 1,
        },
        dispatch_row_span: FileSpan {
            offset: row,
            byte_len: 2,
        },
        handler_cpu_address: 0xa200,
        handler_span: FileSpan {
            offset: handler,
            byte_len: 15,
        },
        operand_count: 8,
        destination_start: 0x6b1,
        destination_end_inclusive: 0x6b8,
        evidence: Vec::new(),
    }
}

#[test]
fn exact_shared_proofs_allow_duplicates_but_conflicting_ownership_is_symmetric() {
    let first = edge(100, 200);
    for (other, expected) in [
        (edge(100, 200), false),
        (edge(102, 215), false),
        (edge(100, 300), false),
        (edge(110, 200), false),
        (edge(101, 300), true),
        (edge(110, 214), true),
        (edge(110, 186), true),
        (edge(214, 300), true),
        (edge(199, 300), true),
        (edge(110, 101), true),
        (edge(110, 86), true),
    ] {
        assert_eq!(conflict(&first, &other), expected);
        assert_eq!(conflict(&other, &first), expected);
    }
}
