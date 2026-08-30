pub(super) fn digest_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(super) fn validate_embedded_final_state_hash(
    label: &str,
    expected: Option<[u8; 32]>,
    final_state_hash: &str,
) -> anyhow::Result<()> {
    if let Some(expected) = expected {
        let expected = digest_hex(&expected);
        if !final_state_hash.eq_ignore_ascii_case(&expected) {
            anyhow::bail!(
                "{label} embedded final state hash mismatch: expected {expected}, got {final_state_hash}"
            );
        }
    }
    Ok(())
}
