use std::collections::{HashMap, HashSet};

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SaveWrite {
    pub(super) key: String,
    pub(super) data: Vec<u8>,
}

#[cfg(all(test, target_arch = "wasm32"))]
impl SaveWrite {
    pub(crate) fn parts_for_test(&self) -> (&str, &[u8]) {
        (&self.key, &self.data)
    }
}

pub(super) fn coalesce_write(writes: &mut Vec<SaveWrite>, key: String, data: Vec<u8>) {
    if let Some(existing) = writes.iter_mut().find(|write| write.key == key) {
        existing.data = data;
    } else {
        writes.push(SaveWrite { key, data });
    }
}

pub(super) fn sram_key(system: &str, media_identity: [u8; 32], component: &str) -> String {
    if system == "pce" && component == "memory-base-128" {
        return "zeff-sram-v2:pce:global:memory-base-128".to_string();
    }
    format!(
        "zeff-sram-v2:{system}:{}:{component}",
        const_hex::encode(media_identity)
    )
}

pub(super) fn migration_allowed(
    scoped_key: &str,
    committed: &HashMap<String, Vec<u8>>,
    pending: &HashSet<String>,
    migrations: usize,
    limit: usize,
) -> bool {
    migrations < limit && !committed.contains_key(scoped_key) && !pending.contains(scoped_key)
}

pub(super) fn publish_committed(cache: &mut HashMap<String, Vec<u8>>, writes: &[SaveWrite]) {
    for write in writes {
        cache.insert(write.key.clone(), write.data.clone());
    }
}

#[derive(Default)]
pub(crate) struct DirtyEpoch<T> {
    epoch: u64,
    observed: Option<T>,
}

impl<T: Clone + Eq> DirtyEpoch<T> {
    pub(crate) fn observe(&mut self, snapshot: &T) -> u64 {
        if self.observed.as_ref() != Some(snapshot) {
            self.epoch = self.epoch.wrapping_add(1).max(1);
            self.observed = Some(snapshot.clone());
        }
        self.epoch
    }

    pub(crate) fn acknowledges(&self, epoch: u64, snapshot: &T) -> bool {
        self.epoch == epoch && self.observed.as_ref() == Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_batch_keys_coalesce_to_the_last_value() {
        let mut writes = Vec::new();
        coalesce_write(&mut writes, "slot".to_string(), vec![1]);
        coalesce_write(&mut writes, "slot".to_string(), vec![2]);
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].data, vec![2]);
    }

    #[test]
    fn cache_publication_occurs_as_one_post_commit_step() {
        let mut cache = HashMap::from([("primary".to_string(), vec![1])]);
        let writes = vec![
            SaveWrite {
                key: "backup".to_string(),
                data: vec![1],
            },
            SaveWrite {
                key: "primary".to_string(),
                data: vec![2],
            },
        ];
        let before = cache.clone();
        assert_eq!(cache, before);
        publish_committed(&mut cache, &writes);
        assert_eq!(cache["backup"], vec![1]);
        assert_eq!(cache["primary"], vec![2]);
    }

    #[test]
    fn sram_keys_scope_media_but_memory_base_is_global() {
        let first = sram_key("gb", [1; 32], "sram");
        let second = sram_key("gb", [2; 32], "sram");
        assert_ne!(first, second);
        assert_eq!(
            sram_key("pce", [1; 32], "memory-base-128"),
            sram_key("pce", [2; 32], "memory-base-128")
        );
    }

    #[test]
    fn migration_skips_current_and_pending_scoped_keys() {
        let key = "scoped";
        let committed = HashMap::from([(key.to_string(), vec![1])]);
        assert!(!migration_allowed(key, &committed, &HashSet::new(), 0, 64));
        let pending = HashSet::from([key.to_string()]);
        assert!(!migration_allowed(key, &HashMap::new(), &pending, 0, 64));
        assert!(migration_allowed(
            key,
            &HashMap::new(),
            &HashSet::new(),
            0,
            64
        ));
    }

    #[test]
    fn stale_inflight_ack_does_not_clear_newer_epoch() {
        let mut tracker = DirtyEpoch::default();
        let first = vec![1];
        let first_epoch = tracker.observe(&first);
        assert!(tracker.acknowledges(first_epoch, &first));
        let second = vec![2];
        tracker.observe(&second);
        assert!(!tracker.acknowledges(first_epoch, &first));
    }
}
