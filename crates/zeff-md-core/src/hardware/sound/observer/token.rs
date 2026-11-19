use super::{ledger::*, observed, tracked};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Empty,
    Owner,
    Image,
}

pub(crate) struct Token {
    pub(super) session: u32,
    pub(super) id: u64,
    pub(super) kind: Kind,
}

impl Token {
    #[cfg(test)]
    pub(crate) fn identity(&self) -> u64 {
        self.id
    }

    pub const fn empty() -> Self {
        Self {
            session: 0,
            id: 0,
            kind: Kind::Empty,
        }
    }

    pub(super) fn new(session: u32, id: u64, kind: Kind) -> Self {
        Self { session, id, kind }
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        if self.session != 0 {
            tracked(|registry| registry.close(self));
        }
    }
}

pub(crate) struct CloneScope {
    session: u32,
    depth: usize,
}

pub(crate) fn deep_clone(source: &Token) -> CloneScope {
    observed(|registry| {
        registry.source(source);
        if registry.parent_depth == DEPTH {
            registry.flags |= EXHAUSTED;
            return CloneScope {
                session: 0,
                depth: 0,
            };
        }
        registry.parents[registry.parent_depth] = source.id;
        registry.parent_depth += 1;
        CloneScope {
            session: registry.session,
            depth: registry.parent_depth,
        }
    })
    .unwrap_or(CloneScope {
        session: 0,
        depth: 0,
    })
}

impl Drop for CloneScope {
    fn drop(&mut self) {
        if self.session == 0 {
            return;
        }
        tracked(|registry| {
            if registry.session != self.session || registry.parent_depth != self.depth {
                registry.flags |= BAD_ORDER;
            } else {
                registry.parent_depth -= 1;
            }
        });
    }
}

pub(crate) struct Attempt {
    session: u32,
    id: u64,
    success: bool,
}

impl Attempt {
    pub fn begin() -> Self {
        observed(|registry| Self {
            session: registry.session,
            id: registry.begin_attempt(),
            success: false,
        })
        .unwrap_or(Self {
            session: 0,
            id: 0,
            success: false,
        })
    }

    pub fn succeed(&mut self) {
        self.success = true;
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if self.session == 0 {
            return;
        }
        tracked(|registry| {
            if registry.session == self.session {
                registry.end_attempt(self.id, self.success);
            } else {
                registry.flags |= INVALID_SOURCE;
            }
        });
    }
}
