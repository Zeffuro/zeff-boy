use super::{Witness, witness};
use anyhow::{Result, ensure};

pub(super) struct Region {
    pub(super) kind: &'static str,
    pub(super) offset: usize,
    pub(super) byte_len: usize,
    pub(super) sha256: &'static str,
}

pub(super) const BASE: &[Region] = &[
    Region {
        kind: "driver_code",
        offset: 0x5F260,
        byte_len: 0x20E8,
        sha256: "6771f1b03a6f5601faaf82ccf02fd4ea71f4e917229f5de0fe0238418d2b3aa3",
    },
    Region {
        kind: "engine_tables",
        offset: 0xF31D0,
        byte_len: 0x2B8,
        sha256: "2833ab3ba0b0606fecc38e52fae982927e0011d7a608483acef5638992f353c7",
    },
    Region {
        kind: "cpu_set",
        offset: 0x61350,
        byte_len: 0x4,
        sha256: "07967ebdc3979cd4fd4de37fd22b3e6de8965b00dae4ac5585577c9709839652",
    },
    Region {
        kind: "callback_bridges",
        offset: 0x61E78,
        byte_len: 0xC,
        sha256: "c99a70b603223eece62ea525a01b8de0867bf9ffcb20b8126a702095f354ca16",
    },
    Region {
        kind: "signed_division",
        offset: 0x61EB0,
        byte_len: 0x98,
        sha256: "d978c39db53692496794787309f5d518a1428e0900c730a2171602ce38299b07",
    },
];

pub(super) const EXTENDED: &[Region] = &[
    Region {
        kind: "driver_code",
        offset: 0x1DC080,
        byte_len: 0x2628,
        sha256: "fbf40c872911dfcf82bae9cc0f57fe0bd5bc7346fd15da42e48b037f4e4482d0",
    },
    Region {
        kind: "engine_tables",
        offset: 0x4899C8,
        byte_len: 0x324,
        sha256: "f8829bcedff46a706721cf29953ed69f6b3bae7ac619930732be87c054a9e1ed",
    },
    Region {
        kind: "cpu_set",
        offset: 0x1E3BD4,
        byte_len: 0x4,
        sha256: "07967ebdc3979cd4fd4de37fd22b3e6de8965b00dae4ac5585577c9709839652",
    },
    Region {
        kind: "callback_bridges",
        offset: 0x1E3C1C,
        byte_len: 0xC,
        sha256: "c99a70b603223eece62ea525a01b8de0867bf9ffcb20b8126a702095f354ca16",
    },
    Region {
        kind: "signed_division",
        offset: 0x1E4088,
        byte_len: 0x98,
        sha256: "d978c39db53692496794787309f5d518a1428e0900c730a2171602ce38299b07",
    },
    Region {
        kind: "extended_copy",
        offset: 0x1E5EE8,
        byte_len: 0x60,
        sha256: "79749a95af8d1e1b6b7f16d76da958e2faee958f09e7e4e4d1509e9840a85a44",
    },
];

pub(super) fn verify(
    bytes: &[u8],
    main: usize,
    init: usize,
    selector: usize,
    regions: &[Region],
) -> Result<Vec<Witness>> {
    let driver = regions
        .first()
        .ok_or_else(|| anyhow::anyhow!("GSF driver fingerprint is missing"))?;
    ensure!(
        driver.offset.checked_add(16) == Some(main)
            && (driver.offset..driver.offset + driver.byte_len).contains(&init)
            && (driver.offset..driver.offset + driver.byte_len).contains(&selector),
        "GSF callbacks do not belong to the qualified driver image"
    );
    regions
        .iter()
        .map(|region| {
            let actual = witness(bytes, region.kind, region.offset, region.byte_len)?;
            ensure!(
                actual.sha256 == region.sha256,
                "GSF complete driver code or dispatch data is modified or unsupported"
            );
            Ok(actual)
        })
        .collect()
}
