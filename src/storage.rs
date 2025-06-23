use std::{cmp::Ordering, collections::HashMap, fmt::Debug, ops::Deref};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use uuid::Uuid;

#[derive(Debug, Eq, Clone)]
pub struct LogEntry {
    index: u64,
    term: u64,
    command: Vec<u8>,
}

impl LogEntry {
    pub fn new(index: u64, term: u64, command: Vec<u8>) -> Self {
        Self {
            index,
            term,
            command,
        }
    }
    pub fn index(&self) -> u64 {
        self.index
    }
    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn command(&self) -> &[u8] {
        &self.command
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
pub struct LogsInformation {
    last_log_index: u64,
    last_log_term: u64,
}
impl LogsInformation {
    pub fn last_log_index(&self) -> u64 {
        self.last_log_index
    }
    pub fn last_log_term(&self) -> u64 {
        self.last_log_term
    }
}

#[derive(Debug, Default)]
pub struct LogEntries(Vec<LogEntry>);

impl LogEntries {
    pub fn last_entries<const N: usize>(&self) -> Vec<LogEntry> {
        self.0
            .last_chunk::<N>()
            .map(|v| v.to_vec())
            .unwrap_or_default()
    }
    pub fn last_log_info(&self) -> LogsInformation {
        let last_entry = self.0.last();
        match last_entry {
            Some(entry) => LogsInformation {
                last_log_index: entry.index,
                last_log_term: entry.term,
            },
            None => LogsInformation {
                last_log_index: 0,
                last_log_term: 0,
            },
        }
    }

    /// We merge the new entries with the current ones. We assume that each index will always be correct and match
    /// the exact index of the log entry. We also assume that the new entries are always in order.
    pub fn merge(&mut self, new_entries: Vec<LogEntry>) -> LogsInformation {
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
        self.last_log_info()
    }

    pub fn new_entry(&mut self, term: u64, command: Vec<u8>) {
        let idx = self.last().map(|e| e.index + 1).unwrap_or_default();
        self.0.push(LogEntry::new(idx, term, command))
    }

    pub fn previous_log_entry_is_up_to_date(
        &self,
        prev_log_index: usize,
        prev_log_term: u64,
    ) -> bool {
        if prev_log_index + self.len() == 0 {
            return true;
        }
        match self.get(prev_log_index) {
            Some(log) => log.term.eq(&prev_log_term),
            None => false,
        }
    }
    pub async fn create(&mut self, data: Vec<u8>, table_name: &str) -> Result<String, String> {
        TableStorage::new(table_name)
            .await
            .map_err(|e| format!("Failed to open storage: {}", e))?
            .create(data)
            .await
            .map_err(|e| format!("Failed to create record: {}", e))
    }

    pub async fn read(&mut self, id: &str, table_name: &str) -> Result<Option<Vec<u8>>, String> {
        TableStorage::new(table_name)
            .await
            .map_err(|e| format!("Failed to open storage: {}", e))?
            .read(id)
            .await
            .map_err(|e| format!("Failed to read record: {}", e))
    }

    pub async fn update(
        &mut self,
        data: Vec<u8>,
        table_name: &str,
        item_id: &str,
    ) -> Result<bool, String> {
        TableStorage::new(table_name)
            .await
            .map_err(|e| format!("Failed to open storage: {}", e))?
            .update(item_id, data)
            .await
            .map_err(|e| format!("Failed to update record: {}", e))
    }

    pub async fn delete(&mut self, id: &str, table_name: &str) -> Result<bool, String> {
        TableStorage::new(table_name)
            .await
            .map_err(|e| format!("Failed to open storage: {}", e))?
            .delete(id)
            .await
            .map_err(|e| format!("Failed to delete record: {}", e))
    }
}

impl Deref for LogEntries {
    type Target = Vec<LogEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromIterator<LogEntry> for LogEntries {
    fn from_iter<T: IntoIterator<Item = LogEntry>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[derive(Debug)]
pub struct IndexEntry {
    pub id: String,
    pub offset: u64,
    pub length: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug)]
pub struct TableStorage {
    table_name: String,
    table_file: tokio::fs::File,
    indices_file: tokio::fs::File,
    indices: HashMap<String, IndexEntry>,
}
//NOTE: Vibecoded i don't know if it works correctly
impl TableStorage {
    pub async fn new(table_name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let table_dir = format!("data/tables/{}", table_name);
        std::fs::create_dir_all(&table_dir)?;

        let table_path = format!("{}/data.bin", table_dir);
        let indices_path = format!("{}/indices.json", table_dir);

        // Open or create table file
        let table_file = tokio::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&table_path)
            .await?;

        // Open or create indices file
        let mut indices_file = tokio::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&indices_path)
            .await?;

        // Load existing indices
        let indices = Self::load_indices(&mut indices_file).await?;

        Ok(Self {
            table_name: table_name.to_string(),
            table_file,
            indices_file,
            indices,
        })
    }

    async fn load_indices(
        indices_file: &mut tokio::fs::File,
    ) -> Result<HashMap<String, IndexEntry>, Box<dyn std::error::Error>> {
        let file_size = indices_file.metadata().await?.len();
        if file_size == 0 {
            return Ok(HashMap::new());
        }

        indices_file.seek(std::io::SeekFrom::Start(0)).await?;

        // Read number of entries
        let mut count_bytes = [0u8; 8];
        indices_file.read_exact(&mut count_bytes).await?;
        let count = u64::from_le_bytes(count_bytes) as usize;

        let mut indices = HashMap::with_capacity(count);

        for _ in 0..count {
            // Read ID length
            let mut id_len_bytes = [0u8; 4];
            indices_file.read_exact(&mut id_len_bytes).await?;
            let id_len = u32::from_le_bytes(id_len_bytes) as usize;

            // Read ID
            let mut id_bytes = vec![0u8; id_len];
            indices_file.read_exact(&mut id_bytes).await?;
            let id = String::from_utf8(id_bytes)?;

            // Read offset, length, created_at, updated_at
            let mut entry_bytes = [0u8; 32]; // 8 + 8 + 8 + 8 bytes
            indices_file.read_exact(&mut entry_bytes).await?;

            let offset = u64::from_le_bytes(entry_bytes[0..8].try_into()?);
            let length = u64::from_le_bytes(entry_bytes[8..16].try_into()?);
            let created_at = u64::from_le_bytes(entry_bytes[16..24].try_into()?);
            let updated_at = u64::from_le_bytes(entry_bytes[24..32].try_into()?);

            let entry = IndexEntry {
                id: id.clone(),
                offset,
                length,
                created_at,
                updated_at,
            };

            indices.insert(id, entry);
        }

        Ok(indices)
    }

    async fn save_indices(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Truncate file
        self.indices_file.set_len(0).await?;
        self.indices_file.seek(std::io::SeekFrom::Start(0)).await?;

        // Write number of entries
        let count = self.indices.len() as u64;
        self.indices_file.write_all(&count.to_le_bytes()).await?;

        // Write each entry
        for (id, entry) in &self.indices {
            // Write ID length and ID
            let id_bytes = id.as_bytes();
            let id_len = id_bytes.len() as u32;
            self.indices_file.write_all(&id_len.to_le_bytes()).await?;
            self.indices_file.write_all(id_bytes).await?;

            // Write entry data
            self.indices_file
                .write_all(&entry.offset.to_le_bytes())
                .await?;
            self.indices_file
                .write_all(&entry.length.to_le_bytes())
                .await?;
            self.indices_file
                .write_all(&entry.created_at.to_le_bytes())
                .await?;
            self.indices_file
                .write_all(&entry.updated_at.to_le_bytes())
                .await?;
        }

        self.indices_file.flush().await?;
        Ok(())
    }

    pub async fn create(&mut self, data: Vec<u8>) -> Result<String, Box<dyn std::error::Error>> {
        let id = Uuid::now_v7().to_string();

        let offset = self.table_file.seek(std::io::SeekFrom::End(0)).await?;

        self.table_file.write_all(&data).await?;
        self.table_file.flush().await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let index_entry = IndexEntry {
            id: id.clone(),
            offset,
            length: data.len() as u64,
            created_at: now,
            updated_at: now,
        };

        self.indices.insert(id.clone(), index_entry);

        self.save_indices().await?;

        tracing::info!(action = "create", table = %self.table_name, id = %id, size = data.len());

        Ok(id)
    }

    pub async fn read(&mut self, id: &str) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let index_entry = match self.indices.get(id) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        // Seek to the data position
        self.table_file
            .seek(std::io::SeekFrom::Start(index_entry.offset))
            .await?;

        // Read the data
        let mut buffer = vec![0u8; index_entry.length as usize];
        self.table_file.read_exact(&mut buffer).await?;

        tracing::info!(action = "read", table = %self.table_name, id = %id, size = buffer.len());

        Ok(Some(buffer))
    }

    pub async fn update(
        &mut self,
        id: &str,
        data: Vec<u8>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.indices.contains_key(id) {
            return Ok(false);
        }

        let offset = self.table_file.seek(std::io::SeekFrom::End(0)).await?;

        self.table_file.write_all(&data).await?;
        self.table_file.flush().await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if let Some(entry) = self.indices.get_mut(id) {
            entry.offset = offset;
            entry.length = data.len() as u64;
            entry.updated_at = now;
        }

        self.save_indices().await?;

        tracing::info!(action = "update", table = %self.table_name, id = %id, size = data.len());

        Ok(true)
    }

    pub async fn delete(&mut self, id: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let existed = self.indices.remove(id).is_some();

        if existed {
            self.save_indices().await?;
            tracing::info!(action = "delete", table = %self.table_name, id = %id);
        }

        Ok(existed)
    }

    pub fn list_ids(&self) -> Vec<String> {
        self.indices.keys().cloned().collect()
    }

    pub fn get_metadata(&self, id: &str) -> Option<&IndexEntry> {
        self.indices.get(id)
    }
}
