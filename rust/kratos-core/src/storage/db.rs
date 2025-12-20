// Database - Abstraction RocksDB
use rocksdb::{Options, DB};
use std::path::Path;
use std::sync::Arc;

/// Wrapper autour de RocksDB
pub struct Database {
    db: Arc<DB>,
}

impl Database {
    /// Ouvre ou crée une base de données
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, DatabaseError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Limiter l'accumulation de fichiers pour éviter "Too many open files"
        opts.set_keep_log_file_num(5);
        opts.set_max_manifest_file_size(64 * 1024 * 1024); // 64MB max par MANIFEST
        opts.set_max_background_jobs(2);

        // Nettoyage automatique des anciens fichiers WAL
        opts.set_recycle_log_file_num(2);

        let db = DB::open(&opts, path).map_err(|e| DatabaseError::OpenFailed(e.to_string()))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Lit une valeur
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DatabaseError> {
        self.db
            .get(key)
            .map_err(|e| DatabaseError::ReadFailed(e.to_string()))
    }

    /// Écrit une valeur
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), DatabaseError> {
        self.db
            .put(key, value)
            .map_err(|e| DatabaseError::WriteFailed(e.to_string()))
    }

    /// Supprime une clé
    pub fn delete(&self, key: &[u8]) -> Result<(), DatabaseError> {
        self.db
            .delete(key)
            .map_err(|e| DatabaseError::WriteFailed(e.to_string()))
    }

    /// Vérifie si une clé existe
    pub fn exists(&self, key: &[u8]) -> Result<bool, DatabaseError> {
        Ok(self.get(key)?.is_some())
    }

    /// Batch write (transaction atomique)
    pub fn batch_write(&self, ops: Vec<WriteOp>) -> Result<(), DatabaseError> {
        let mut batch = rocksdb::WriteBatch::default();

        for op in ops {
            match op {
                WriteOp::Put { key, value } => batch.put(&key, &value),
                WriteOp::Delete { key } => batch.delete(&key),
            }
        }

        self.db
            .write(batch)
            .map_err(|e| DatabaseError::WriteFailed(e.to_string()))
    }

    /// Itère sur toutes les clés avec un préfixe donné
    /// FIX: Handle RocksDB errors gracefully instead of panicking
    pub fn prefix_iterator<'a>(&'a self, prefix: &'a [u8]) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)> + 'a {
        let iter = self.db.prefix_iterator(prefix);
        iter.filter_map(|item| {
            // FIX: Use filter_map to skip errors instead of unwrap() which panics
            match item {
                Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                Err(e) => {
                    // Log the error but continue iteration
                    tracing::warn!("Database iteration error (skipping): {}", e);
                    None
                }
            }
        })
        .take_while(move |(key, _)| key.starts_with(prefix))
    }

    /// Itère sur toutes les clés avec un préfixe donné, returning Result for each item
    /// Use this when you need to handle errors explicitly
    pub fn prefix_iterator_with_errors<'a>(&'a self, prefix: &'a [u8]) -> impl Iterator<Item = Result<(Vec<u8>, Vec<u8>), DatabaseError>> + 'a {
        let iter = self.db.prefix_iterator(prefix);
        iter.map(|item| {
            item.map(|(key, value)| (key.to_vec(), value.to_vec()))
                .map_err(|e| DatabaseError::ReadFailed(e.to_string()))
        })
        .take_while(move |result| {
            match result {
                Ok((key, _)) => key.starts_with(prefix),
                Err(_) => true, // Continue iteration on error to let caller handle it
            }
        })
    }
}

/// Opération d'écriture pour batch
#[derive(Debug, Clone)]
pub enum WriteOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// Erreurs de base de données
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Échec d'ouverture de la DB: {0}")]
    OpenFailed(String),

    #[error("Échec de lecture: {0}")]
    ReadFailed(String),

    #[error("Échec d'écriture: {0}")]
    WriteFailed(String),

    #[error("Sérialisation échouée: {0}")]
    SerializationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_database_basic_ops() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();

        // Put
        db.put(b"key1", b"value1").unwrap();

        // Get
        let value = db.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));

        // Exists
        assert!(db.exists(b"key1").unwrap());
        assert!(!db.exists(b"key2").unwrap());

        // Delete
        db.delete(b"key1").unwrap();
        assert!(!db.exists(b"key1").unwrap());
    }

    #[test]
    fn test_database_batch() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();

        let ops = vec![
            WriteOp::Put {
                key: b"key1".to_vec(),
                value: b"value1".to_vec(),
            },
            WriteOp::Put {
                key: b"key2".to_vec(),
                value: b"value2".to_vec(),
            },
        ];

        db.batch_write(ops).unwrap();

        assert!(db.exists(b"key1").unwrap());
        assert!(db.exists(b"key2").unwrap());
    }
}
