use zeff_m68k_verify::reference::{Case, Phase};

pub(super) const COHORTS: &[&str] = &[
    "LINK",
    "UNLINK",
    "RTR",
    "TRAP",
    "MOVEfromUSP",
    "MOVEtoUSP",
    "MOVEfromSR",
    "MOVEtoSR",
    "MOVEtoCCR",
];

pub(super) fn source_hash(cohort: &str) -> Option<&'static str> {
    match cohort {
        "LINK" => Some("cb1ab4e4f812c11eef19b980e52401c0ce2ea82551be26fadb9406daa2e6d550"),
        "UNLINK" => Some("ed9327770747232b80cec2c1b0c8fabdfef2f1067af78a6b21f7c20b407d78e4"),
        "RTR" => Some("05f83705c724ca94f9af80b9afab3800fee92c94c9a398a8fd774c9604727f6c"),
        "TRAP" => Some("fead4435111103b49ccdef17f60852c438d81f21f1fd9a5ffcac0c2548ad7a85"),
        "MOVEfromUSP" => Some("7dcd639e8f8619b78581940397ba5f07ad4378d879ae3b4fa90e5e2979379947"),
        "MOVEtoUSP" => Some("f43a627e07e59c5bfc83f11ef756b1c743d0249186beb7b3f12ea16b66ea7a19"),
        "MOVEfromSR" => Some("c6a416d1e93c31d1901a1296f08e7cb14e99cf8dbe5c6541878b1956751fb725"),
        "MOVEtoSR" => Some("cf7ce17aacfc6be5a9db9456bd7d7d70206c13e456691245011064e6c0a43d75"),
        "MOVEtoCCR" => Some("603ffea2c9109daa2358288934337a52976636c59d008a75a5ba71d70b304729"),
        _ => None,
    }
}

pub(super) fn cohort_matches(case: &Case) -> bool {
    let op = case.initial.prefetch[0];
    match case.cohort.as_str() {
        "LINK" => op & 0xfff8 == 0x4e50,
        "UNLINK" => op & 0xfff8 == 0x4e58,
        "RTR" => op == 0x4e77,
        "TRAP" => op & 0xfff0 == 0x4e40,
        "MOVEfromUSP" => op & 0xfff8 == 0x4e68,
        "MOVEtoUSP" => op & 0xfff8 == 0x4e60,
        "MOVEfromSR" => {
            op & 0xffc0 == 0x40c0
                && (op >> 3 & 7 == 0
                    || matches!(op >> 3 & 7, 2..=6)
                    || op >> 3 & 7 == 7 && op & 7 <= 1)
        }
        "MOVEtoSR" => op & 0xffc0 == 0x46c0 && super::super::profile::legal_status(op),
        "MOVEtoCCR" => op & 0xffc0 == 0x44c0 && super::super::profile::legal_status(op),
        _ => false,
    }
}

pub(super) fn low_supervisor(case: &Case) -> bool {
    let op = case.initial.prefetch[0];
    let vector = (op & 0xfff0 == 0x4e40).then_some(4 * (32 + u32::from(op & 15)));
    case.transactions.iter().any(|p| {
        matches!(p, Phase::Read(t)
        if t.function_code == 5 && t.address_bus < 0x400
            && vector.is_none_or(|at| t.address_bus != at && t.address_bus != at + 2))
    })
}
