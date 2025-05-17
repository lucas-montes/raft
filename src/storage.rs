use std::{cmp::Ordering, fmt::Debug, ops::Deref};

#[derive(Debug, Eq)]
pub struct LogEntry {
    index: u64,
    term: u64,
    command: String,
}

impl LogEntry {
    pub fn new(index: u64, term: u64, command: String) -> Self {
        Self {
            index,
            term,
            command,
        }
    }
}

impl PartialEq for LogEntry {
    fn eq(&self, other: &Self) -> bool {
        self.index.eq(&other.index) && self.term.eq(&other.term)
    }
}

impl PartialOrd for LogEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LogEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.index.cmp(&other.index)
    }
}

#[derive(Debug, Default)]
pub struct LogEntries(Vec<LogEntry>);

impl LogEntries {
    pub fn last_log_info(&self) -> (u64, u64) {
        let last_entry = self.0.last();
        match last_entry {
            Some(entry) => (entry.term, entry.index),
            None => (0, 0),
        }
    }

    /// We merge the new entries with the current ones. We assume that each index will always be correct and match
    /// the exact index of the log entry. We also assume that the new entries are always in order.
    fn merge(&mut self, new_entries: Vec<LogEntry>) {
        for entry in new_entries {
            let idx = entry.index as usize;
            match self.get(idx) {
                Some(log) => {
                    if log.term != entry.term {
                        self.0.truncate(idx);
                        self.0.push(entry);
                    }
                }
                None => {
                    self.0.push(entry);
                }
            }
        }
    }

    // fn new_entry(&mut self, term: u64, command: String) {
    //     let idx = self.last().map(|e| e.index + 1).unwrap_or_default();
    //     self.0.push(LogEntry::new(idx, term, command))
    // }

    fn previous_log_entry_is_up_to_date(&self, prev_log_index: usize, prev_log_term: u64) -> bool {
        if prev_log_index + self.len() == 0 {
            return true;
        }
        match self.get(prev_log_index) {
            Some(log) => log.term.eq(&prev_log_term),

            None => {
                return false;
            }
        }
    }
}

impl Deref for LogEntries {
    type Target = Vec<LogEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
