pub(super) fn get(index: usize) -> Vec<(u32, u16)> {
    let bytes = include_bytes!("memory.bin");
    assert_eq!(&bytes[..4], b"PNM1");
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let count = word(4);
    assert_eq!(count, 144);
    assert!(index < count);
    let start = 8 + 4 * (count + 1);
    assert_eq!(start + word(8 + 4 * count), bytes.len());
    let first = start + word(8 + 4 * index);
    let end = start + word(8 + 4 * (index + 1));
    let (pairs, remainder) = bytes[first..end].as_chunks::<6>();
    assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|pair| {
            (
                u32::from_le_bytes(pair[..4].try_into().unwrap()),
                u16::from_le_bytes(pair[4..].try_into().unwrap()),
            )
        })
        .collect()
}
