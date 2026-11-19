use super::arena::Arena;
use super::ledger::*;
use super::token::{Kind, Token};

pub(super) struct Registry {
    pub session: u32,
    pub flags: u32,
    pub suppressed: usize,
    pub parents: [u64; DEPTH],
    pub parent_depth: usize,
    pub attempts: [u64; DEPTH],
    pub attempt_depth: usize,
    pub counters: [u64; METRICS],
    pub owners: Arena<Owner>,
    generations: Arena<Generation>,
    images: Arena<Image>,
    preparations: Arena<Preparation>,
}

impl Registry {
    pub const fn new() -> Self {
        Self {
            session: 0,
            flags: 0,
            suppressed: 0,
            parents: [0; DEPTH],
            parent_depth: 0,
            attempts: [0; DEPTH],
            attempt_depth: 0,
            counters: [0; METRICS],
            owners: Arena::new(),
            generations: Arena::new(),
            images: Arena::new(),
            preparations: Arena::new(),
        }
    }

    pub fn add(&mut self, metric: Metric, amount: u64) {
        match self.counters[metric as usize].checked_add(amount) {
            Some(value) => self.counters[metric as usize] = value,
            None => self.flags |= OVERFLOW,
        }
    }

    fn generation(&mut self, address: usize) -> Option<u64> {
        let id = self.generations.insert(Generation {
            address,
            owners: 0,
            last_strong: 0,
            last_weak: 0,
        });
        if id.is_some() {
            self.add(Metric::GenerationsCreated, 1);
        } else {
            self.flags |= EXHAUSTED;
        }
        id
    }

    fn release_generation(&mut self, id: u64) {
        let Some(generation) = self.generations.get_mut(id) else {
            self.flags |= INVALID_SOURCE;
            return;
        };
        if generation.owners == 0 {
            self.flags |= INVALID_SOURCE;
            return;
        }
        generation.owners -= 1;
        if generation.owners == 0 {
            self.generations.remove(id);
        }
    }

    pub fn owner(&mut self, generation: u64) -> Token {
        if self.generations.get(generation).is_none() {
            self.flags |= INVALID_SOURCE;
            return Token::empty();
        }
        let Some(id) = self.owners.insert(Owner { generation }) else {
            self.flags |= EXHAUSTED;
            return Token::empty();
        };
        self.generations.get_mut(generation).unwrap().owners += 1;
        self.add(Metric::OwnersCreated, 1);
        Token::new(self.session, id, Kind::Owner)
    }

    pub fn allocation(&mut self, address: usize) -> Token {
        if self.parent_depth != 0 {
            self.add(Metric::DeepCopies, 1);
            self.add(Metric::CopyBytes, crate::constants::SOUND_RAM_SIZE as u64);
        }
        let Some(generation) = self.generation(address) else {
            return Token::empty();
        };
        let token = self.owner(generation);
        if token.id == 0 {
            self.generations.remove(generation);
        }
        token
    }

    pub fn source(&mut self, token: &Token) -> Option<u64> {
        if token.session == self.session
            && token.kind == Kind::Owner
            && self.owners.get(token.id).is_some()
        {
            return Some(token.id);
        }
        self.flags |= INVALID_SOURCE;
        None
    }

    pub fn snapshot(&mut self, source: &Token) -> Token {
        let Some(id) = self.source(source) else {
            return Token::empty();
        };
        self.add(Metric::Snapshots, 1);
        self.owner(self.owners.get(id).unwrap().generation)
    }

    pub fn image(&mut self, owner: &Token, parent: Option<&Token>) -> Token {
        if self.source(owner).is_none() {
            return Token::empty();
        }
        if let Some(parent) = parent {
            if parent.session != self.session
                || parent.kind != Kind::Image
                || self.images.get(parent.id).is_none()
            {
                self.flags |= INVALID_SOURCE;
            }
            self.add(Metric::ImageClones, 1);
        }
        let Some(id) = self.images.insert(Image { owner: owner.id }) else {
            self.flags |= EXHAUSTED;
            return Token::empty();
        };
        self.add(Metric::ImagesCreated, 1);
        Token::new(self.session, id, Kind::Image)
    }

    pub fn close(&mut self, token: &Token) {
        if token.session != self.session {
            self.flags |= INVALID_SOURCE;
            return;
        }
        match token.kind {
            Kind::Owner => {
                if let Some(owner) = self.owners.remove(token.id) {
                    self.release_generation(owner.generation);
                    self.add(Metric::OwnersClosed, 1);
                    return;
                }
            }
            Kind::Image => {
                if self.images.remove(token.id).is_some() {
                    self.add(Metric::ImagesClosed, 1);
                    return;
                }
            }
            Kind::Empty => {}
        }
        self.flags |= INVALID_SOURCE;
    }

    pub fn before_mut(&mut self, token: &Token, address: usize, strong: usize, weak: usize) {
        let Some(owner) = self.source(token) else {
            return;
        };
        let id = self.owners.get(owner).unwrap().generation;
        let generation = self.generations.get_mut(id).unwrap();
        if generation.address != address || strong == 0 {
            self.flags |= INVALID_SOURCE;
        }
        generation.last_strong = strong;
        generation.last_weak = weak;
        self.add(Metric::MutBorrows, 1);
    }

    pub fn after_mut(&mut self, token: &Token, address: usize) {
        let Some(owner) = self.source(token) else {
            return;
        };
        let old = self.owners.get(owner).unwrap().generation;
        let previous = *self.generations.get(old).unwrap();
        if previous.address == address {
            if previous.last_strong != 1 || previous.last_weak != 0 {
                self.flags |= INVALID_SOURCE;
            }
            return;
        }
        if previous.last_strong > 1 {
            self.add(Metric::SharedDetaches, 1);
            self.add(Metric::CopyBytes, crate::constants::SOUND_RAM_SIZE as u64);
        } else if previous.last_weak != 0 {
            self.add(Metric::WeakDissociations, 1);
        } else {
            self.flags |= INVALID_SOURCE;
        }
        if let Some(generation) = self.generation(address) {
            self.generations.get_mut(generation).unwrap().owners = 1;
            self.owners.get_mut(owner).unwrap().generation = generation;
            self.release_generation(old);
        }
    }

    pub fn begin_attempt(&mut self) -> u64 {
        if self.attempt_depth == DEPTH {
            self.flags |= EXHAUSTED;
            return 0;
        }
        let Some(id) = self.preparations.insert(Preparation { writes: 0 }) else {
            self.flags |= EXHAUSTED;
            return 0;
        };
        self.attempts[self.attempt_depth] = id;
        self.attempt_depth += 1;
        self.add(Metric::Attempts, 1);
        id
    }

    pub fn access(&mut self, ram: bool, write: bool) {
        self.add(
            match (ram, write) {
                (true, false) => Metric::RamReads,
                (true, true) => Metric::RamWrites,
                (false, false) => Metric::OtherReads,
                (false, true) => Metric::OtherWrites,
            },
            1,
        );
        if ram && write && self.attempt_depth != 0 {
            let id = self.attempts[self.attempt_depth - 1];
            let preparation = self.preparations.get_mut(id).unwrap();
            match preparation.writes.checked_add(1) {
                Some(writes) => preparation.writes = writes,
                None => self.flags |= OVERFLOW,
            }
        }
    }

    pub fn end_attempt(&mut self, id: u64, success: bool) {
        if self.attempt_depth == 0 || self.attempts[self.attempt_depth - 1] != id {
            self.flags |= BAD_ORDER;
            return;
        }
        let Some(preparation) = self.preparations.remove(id) else {
            self.flags |= BAD_ORDER;
            return;
        };
        self.attempt_depth -= 1;
        self.add(
            if success {
                Metric::Successes
            } else {
                Metric::Failures
            },
            1,
        );
        if success {
            self.add(
                match preparation.writes {
                    0 => Metric::ZeroWrites,
                    1 => Metric::OneWrite,
                    2 => Metric::TwoWrites,
                    _ => Metric::ManyWrites,
                },
                1,
            );
        }
    }

    pub fn publication(&mut self, image: &Token) {
        if image.session != self.session
            || image.kind != Kind::Image
            || self
                .images
                .get(image.id)
                .is_none_or(|record| self.owners.get(record.owner).is_none())
        {
            self.flags |= INVALID_SOURCE;
        }
        self.add(Metric::Publications, 1);
    }

    #[cfg(any(test, feature = "ram-copy-census"))]
    pub fn report(&self) -> Report {
        Report {
            scope: "DirectedOwnerThreadOnly",
            full_coverage: false,
            flags: self.flags,
            live_owners: self.owners.len(),
            live_images: self.images.len(),
            counters: self.counters,
        }
    }
}
