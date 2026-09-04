use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::emu_backend::pce_cd::PceCdTasPpfStack;

thread_local! {
    static SYSTEM_CARDS: RefCell<BTreeMap<[u8; 32], &'static [u8]>> =
        const { RefCell::new(BTreeMap::new()) };
    static PPF_STACKS: RefCell<BTreeMap<PathBuf, PceCdTasPpfStack>> =
        const { RefCell::new(BTreeMap::new()) };
}

pub(crate) struct TestPceCdSystemCardGuard([u8; 32]);

pub(crate) fn register_test_pce_cd_system_card(
    sha256: [u8; 32],
    bytes: &'static [u8],
) -> TestPceCdSystemCardGuard {
    SYSTEM_CARDS.with(|cards| assert!(cards.borrow_mut().insert(sha256, bytes).is_none()));
    TestPceCdSystemCardGuard(sha256)
}

pub(crate) fn system_card(sha256: [u8; 32]) -> Option<&'static [u8]> {
    SYSTEM_CARDS.with(|cards| cards.borrow().get(&sha256).copied())
}

pub(crate) fn sole_system_card() -> (Option<&'static [u8]>, Option<[u8; 32]>) {
    SYSTEM_CARDS.with(|cards| {
        let cards = cards.borrow();
        if cards.len() == 1 {
            cards
                .iter()
                .next()
                .map(|(sha256, bytes)| (Some(*bytes), Some(*sha256)))
                .unwrap_or((None, None))
        } else {
            (None, None)
        }
    })
}

impl Drop for TestPceCdSystemCardGuard {
    fn drop(&mut self) {
        SYSTEM_CARDS.with(|cards| {
            cards.borrow_mut().remove(&self.0);
        });
    }
}

pub(crate) struct TestPceCdPpfStackGuard(PathBuf);

pub(crate) fn register_test_pce_cd_ppf_stack(
    path: PathBuf,
    stack: PceCdTasPpfStack,
) -> TestPceCdPpfStackGuard {
    PPF_STACKS.with(|stacks| assert!(stacks.borrow_mut().insert(path.clone(), stack).is_none()));
    TestPceCdPpfStackGuard(path)
}

pub(crate) fn ppf_stack(path: &Path) -> Option<PceCdTasPpfStack> {
    PPF_STACKS.with(|stacks| stacks.borrow().get(path).cloned())
}

impl Drop for TestPceCdPpfStackGuard {
    fn drop(&mut self) {
        PPF_STACKS.with(|stacks| {
            stacks.borrow_mut().remove(&self.0);
        });
    }
}
