include!("../ordinary_clear_not_first/check.rs");
pub fn remainder(log: bool) -> (usize, usize, u64) {
    let rows = literal::remainder_records();
    let mut bad = 0;
    let mut signature = 14695981039346656037u64;
    for (index, row) in rows.iter().enumerate() {
        let id = row.get("axis").get("id").text();
        assert_eq!(id, format!("ONN{index:05}"));
        if !run_one(row) {
            bad += 1;
            for byte in id.bytes().chain(std::iter::once(b'\n')) {
                signature = (signature ^ u64::from(byte)).wrapping_mul(1099511628211);
            }
            if log && bad <= 12 {
                eprintln!("ordinary NEG/NEGX mismatch {id}");
            }
        }
    }
    (rows.len(), bad, signature)
}
