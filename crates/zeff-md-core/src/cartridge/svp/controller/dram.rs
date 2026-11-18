use super::DRAM_WORDS;
use std::sync::Arc;
use zeff_emu_common::save_state::{StateReader, StateWriter};

const PAGE_WORDS: usize = 256;
const GROUP_PAGES: usize = 16;
const GROUP_COUNT: usize = DRAM_WORDS / (PAGE_WORDS * GROUP_PAGES);

type Page = [u16; PAGE_WORDS];
type Group = [Arc<Page>; GROUP_PAGES];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Dram {
    groups: Arc<[Arc<Group>; GROUP_COUNT]>,
}

impl Dram {
    pub fn zeroed() -> Self {
        let page = Arc::new([0; PAGE_WORDS]);
        let group: Arc<Group> = Arc::new(std::array::from_fn(|_| Arc::clone(&page)));
        Self {
            groups: Arc::new(std::array::from_fn(|_| Arc::clone(&group))),
        }
    }

    pub fn read(&self, address: u16) -> u16 {
        let address = usize::from(address);
        let page = address / PAGE_WORDS;
        self.groups[page / GROUP_PAGES][page % GROUP_PAGES][address % PAGE_WORDS]
    }

    pub fn write(&mut self, address: u16, value: u16) {
        if self.read(address) == value {
            return;
        }
        let address = usize::from(address);
        let index = address / PAGE_WORDS;
        let group = &mut Arc::make_mut(&mut self.groups)[index / GROUP_PAGES];
        let page = &mut Arc::make_mut(group)[index % GROUP_PAGES];
        Arc::make_mut(page)[address % PAGE_WORDS] = value;
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        for group in self.groups.iter() {
            for page in group.iter() {
                for &word in page.iter() {
                    writer.write_u16(word);
                }
            }
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let mut value = Self::zeroed();
        for group in Arc::make_mut(&mut value.groups) {
            for page in Arc::make_mut(group) {
                for word in Arc::make_mut(page) {
                    *word = reader.read_u16()?;
                }
            }
        }
        Ok(value)
    }
}
