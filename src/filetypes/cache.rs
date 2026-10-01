//! Keep recently viewed summaries without retaining their source trees.
use super::{Key, Summary};
use crate::background::retire;
use std::collections::VecDeque;

const SCOPES: usize = 4;
const ROWS: usize = 1024;

#[derive(Default)]
pub(super) struct Cache {
    epoch: Option<(u64, u64)>,
    entries: VecDeque<(Key, Summary)>,
    rows: usize,
}

impl Cache {
    pub fn prepare(&mut self, key: Key) {
        let epoch = (key.document, key.generation);
        if self.epoch != Some(epoch) {
            if !self.entries.is_empty() {
                retire(std::mem::take(&mut self.entries));
            }
            self.rows = 0;
            self.epoch = Some(epoch);
        }
    }

    pub fn take(&mut self, key: Key) -> Option<Summary> {
        let index = self.entries.iter().position(|(stored, _)| *stored == key)?;
        let (_, summary) = self.entries.remove(index)?;
        self.rows -= summary.rows.len();
        Some(summary)
    }

    pub fn insert(&mut self, key: Key, summary: Summary) {
        if self.epoch != Some((key.document, key.generation)) || summary.rows.len() > ROWS {
            retire(summary);
            return;
        }
        if let Some(previous) = self.take(key) {
            retire(previous);
        }
        while self.entries.len() >= SCOPES || self.rows + summary.rows.len() > ROWS {
            let (_, old) = self.entries.pop_front().expect("nonempty bounded cache");
            self.rows -= old.rows.len();
            retire(old);
        }
        self.rows += summary.rows.len();
        self.entries.push_back((key, summary));
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        if !self.entries.is_empty() {
            retire(std::mem::take(&mut self.entries));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filetypes::Row;

    fn summary(rows: usize) -> Summary {
        Summary {
            name: "scope".into(),
            rows: (0..rows)
                .map(|i| Row {
                    extension: format!(".{i}"),
                    kind: String::new(),
                    size: String::new(),
                    files: String::new(),
                    share: String::new(),
                    fraction: 0.0,
                })
                .collect(),
        }
    }
    fn key(scope: u32) -> Key {
        Key { document: 1, generation: 0, scope }
    }

    #[test]
    fn navigation_reuses_totals_but_live_edits_and_new_documents_invalidate_them() {
        let mut cache = Cache::default();
        cache.prepare(key(0));
        cache.insert(key(0), summary(3));
        cache.prepare(key(1));
        assert_eq!(cache.take(key(0)).expect("cached root").rows.len(), 3);
        cache.insert(key(0), summary(3));
        let edited = Key { generation: 1, ..key(0) };
        cache.prepare(edited);
        assert!(cache.take(key(0)).is_none());
        cache.insert(key(0), summary(3)); // A late result from the old snapshot cannot repopulate it.
        assert!(cache.take(key(0)).is_none());
        cache.insert(edited, summary(3));
        cache.prepare(Key { document: 2, ..edited });
        assert!(cache.take(edited).is_none());
    }

    #[test]
    fn cache_bounds_both_scopes_and_total_rows() {
        let mut cache = Cache::default();
        cache.prepare(key(0));
        for scope in 0..5 {
            cache.insert(key(scope), summary(1));
        }
        assert!(cache.take(key(0)).is_none());
        assert!(cache.take(key(4)).is_some());
        cache.insert(key(5), summary(ROWS));
        assert_eq!(cache.entries.len(), 1);
        cache.insert(key(6), summary(ROWS + 1));
        assert!(cache.take(key(6)).is_none());
        assert_eq!(cache.rows, ROWS);
    }
}
