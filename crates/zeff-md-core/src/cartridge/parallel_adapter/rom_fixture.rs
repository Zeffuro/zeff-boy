use super::{Model, ParallelAdapter};
use crate::cartridge::Cartridge;

pub(crate) const CASES: [(Model, usize, u32); 3] = [
    (Model::First, 0x20000, 0x420000),
    (Model::First, 0x10000, 0x420000),
    (Model::Second, 0x10000, 0x600000),
];

pub(crate) fn ordinary(model: Model, size: usize, program: &[u16]) -> Cartridge {
    let base = if model == Model::First {
        0x420000_u32
    } else {
        0x600000
    };
    let mut image = vec![0; size];
    image[..4].copy_from_slice(&(base + 0x5dc).to_be_bytes());
    image[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    for (at, words) in [
        (0x200, program),
        (0x8100, &[0x7001, 0x0640, 3, 0x4e75, 0x4e71][..]),
        (0x8000, &[0x1234, 0x5678][..]),
        (size - 2, &[0x9abc][..]),
    ] {
        for (index, word) in words.iter().enumerate() {
            image[at + index * 2..at + index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
    }
    assert!(ParallelAdapter::detect(&image).is_none());
    let cart = Cartridge::new(image).unwrap();
    assert!(cart.parallel_adapter.is_none());
    cart
}

pub(crate) fn attach(mut ordinary: Cartridge, model: Model) -> Cartridge {
    assert!(ordinary.parallel_adapter.is_none());
    ordinary.parallel_adapter = Some(ParallelAdapter::new(model));
    assert!(!ordinary.supports_rom_overlay());
    assert!(ordinary.validate_parallel_adapter().is_err());
    ordinary
}
