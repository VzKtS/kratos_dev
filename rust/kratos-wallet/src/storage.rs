// Wallet storage - Secure file-based storage for wallet data

use std::fs;
use std::path::{Path, PathBuf};

use crate::crypto::{decrypt_secret, encrypt_secret, EncryptedWallet, WalletKeys};
use crate::types::{TransactionHistory, TransactionRecord};

const WALLET_FILENAME: &str = "wallet.json";
const HISTORY_FILENAME: &str = "history.json";

/// Wallet storage manager
pub struct WalletStorage {
    wallet_dir: PathBuf,
}

impl WalletStorage {
    /// Create new storage manager
    pub fn new(wallet_dir: &Path) -> Self {
        Self {
            wallet_dir: wallet_dir.to_path_buf(),
        }
    }

    /// Get wallet file path
    fn wallet_path(&self) -> PathBuf {
        self.wallet_dir.join(WALLET_FILENAME)
    }

    /// Check if wallet exists
    pub fn wallet_exists(&self) -> bool {
        self.wallet_path().exists()
    }

    /// Save wallet (encrypted)
    pub fn save_wallet(
        &self,
        keys: &WalletKeys,
        password: &str,
        rpc_url: &str,
    ) -> Result<(), String> {
        // Create directory if needed
        fs::create_dir_all(&self.wallet_dir)
            .map_err(|e| format!("Failed to create wallet directory: {}", e))?;

        // Encrypt secret key
        let mut encrypted = encrypt_secret(&keys.secret_key_bytes(), password)?;
        encrypted.rpc_url = rpc_url.to_string();

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&encrypted)
            .map_err(|e| format!("Serialization error: {}", e))?;

        // Write to file with restricted permissions
        let wallet_path = self.wallet_path();

        // On Unix, set file permissions to 600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            use std::io::Write;

            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&wallet_path)
                .map_err(|e| format!("Failed to create wallet file: {}", e))?;

            file.write_all(json.as_bytes())
                .map_err(|e| format!("Failed to write wallet file: {}", e))?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&wallet_path, json)
                .map_err(|e| format!("Failed to write wallet file: {}", e))?;
        }

        Ok(())
    }

    /// Load wallet (decrypted)
    pub fn load_wallet(&self, password: &str) -> Result<(WalletKeys, String), String> {
        let wallet_path = self.wallet_path();

        // Read file
        let json = fs::read_to_string(&wallet_path)
            .map_err(|e| format!("Failed to read wallet file: {}", e))?;

        // Deserialize
        let encrypted: EncryptedWallet = serde_json::from_str(&json)
            .map_err(|e| format!("Invalid wallet format: {}", e))?;

        // Decrypt secret key
        let secret = decrypt_secret(&encrypted, password)?;

        // Create keys from secret
        let keys = WalletKeys::from_secret(secret);

        Ok((keys, encrypted.rpc_url))
    }

    /// Delete wallet (use with caution!)
    #[allow(dead_code)]
    pub fn delete_wallet(&self) -> Result<(), String> {
        let wallet_path = self.wallet_path();
        if wallet_path.exists() {
            fs::remove_file(&wallet_path)
                .map_err(|e| format!("Failed to delete wallet: {}", e))?;
        }
        Ok(())
    }

    /// Get wallet info without decryption (public key, rpc_url)
    #[allow(dead_code)]
    pub fn get_wallet_info(&self) -> Result<(String, String), String> {
        let wallet_path = self.wallet_path();

        let json = fs::read_to_string(&wallet_path)
            .map_err(|e| format!("Failed to read wallet file: {}", e))?;

        let encrypted: EncryptedWallet = serde_json::from_str(&json)
            .map_err(|e| format!("Invalid wallet format: {}", e))?;

        let account_id = hex::encode(encrypted.public_key);

        Ok((account_id, encrypted.rpc_url))
    }

    // =========================================================================
    // TRANSACTION HISTORY STORAGE
    // =========================================================================

    /// Get history file path
    fn history_path(&self) -> PathBuf {
        self.wallet_dir.join(HISTORY_FILENAME)
    }

    /// Load transaction history from disk
    pub fn load_history(&self) -> TransactionHistory {
        let history_path = self.history_path();

        if !history_path.exists() {
            return TransactionHistory::new();
        }

        match fs::read_to_string(&history_path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_else(|_| TransactionHistory::new()),
            Err(_) => TransactionHistory::new(),
        }
    }

    /// Save transaction history to disk
    pub fn save_history(&self, history: &TransactionHistory) -> Result<(), String> {
        // Ensure directory exists
        fs::create_dir_all(&self.wallet_dir)
            .map_err(|e| format!("Failed to create wallet directory: {}", e))?;

        let json = serde_json::to_string_pretty(history)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        let history_path = self.history_path();

        // On Unix, set file permissions to 600
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;

            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&history_path)
                .map_err(|e| format!("Failed to create history file: {}", e))?;

            file.write_all(json.as_bytes())
                .map_err(|e| format!("Failed to write history file: {}", e))?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&history_path, json)
                .map_err(|e| format!("Failed to write history file: {}", e))?;
        }

        Ok(())
    }

    /// Add a transaction to history
    pub fn add_transaction(&self, record: TransactionRecord) -> Result<(), String> {
        let mut history = self.load_history();
        history.add(record);
        self.save_history(&history)
    }

    /// Get transaction history
    pub fn get_history(&self) -> TransactionHistory {
        self.load_history()
    }

    /// Clear transaction history
    #[allow(dead_code)]
    pub fn clear_history(&self) -> Result<(), String> {
        let history_path = self.history_path();
        if history_path.exists() {
            fs::remove_file(&history_path)
                .map_err(|e| format!("Failed to delete history: {}", e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_and_load_wallet() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        // Generate keys
        let keys = WalletKeys::generate();
        let original_account = keys.account_id_hex();
        let password = "test_password";
        let rpc_url = "http://127.0.0.1:9933";

        // Save wallet
        storage.save_wallet(&keys, password, rpc_url).unwrap();

        assert!(storage.wallet_exists());

        // Load wallet
        let (loaded_keys, loaded_rpc) = storage.load_wallet(password).unwrap();

        assert_eq!(loaded_keys.account_id_hex(), original_account);
        assert_eq!(loaded_rpc, rpc_url);
    }

    #[test]
    fn test_wrong_password() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        let keys = WalletKeys::generate();
        let password = "correct_password";

        storage.save_wallet(&keys, password, "http://localhost").unwrap();

        let result = storage.load_wallet("wrong_password");
        assert!(result.is_err());
    }

    #[test]
    fn test_wallet_not_exists() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        assert!(!storage.wallet_exists());
    }

    #[test]
    fn test_transaction_history_storage() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        // Initially empty
        let history = storage.load_history();
        assert!(history.is_empty());

        // Add a transaction
        let record = TransactionRecord::new_sent(
            "0x1234567890abcdef".to_string(),
            "0xrecipient".to_string(),
            1_000_000_000_000, // 1 KRAT
            1700000000,
            0,
        );

        storage.add_transaction(record).unwrap();

        // Reload and verify
        let history = storage.load_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history.transactions[0].hash, "0x1234567890abcdef");
    }

    #[test]
    fn test_transaction_history_no_duplicates() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        // Add same transaction twice
        let record1 = TransactionRecord::new_sent(
            "0xsamehash".to_string(),
            "0xrecipient".to_string(),
            1_000_000_000_000,
            1700000000,
            0,
        );

        let record2 = TransactionRecord::new_sent(
            "0xsamehash".to_string(),
            "0xrecipient".to_string(),
            2_000_000_000_000, // Different amount
            1700000001,
            1,
        );

        storage.add_transaction(record1).unwrap();
        storage.add_transaction(record2).unwrap();

        // Should only have one (first one)
        let history = storage.load_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history.transactions[0].amount, 1_000_000_000_000);
    }

    #[test]
    fn test_transaction_history_ordering() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::new(dir.path());

        // Add transactions
        for i in 0..5 {
            let record = TransactionRecord::new_sent(
                format!("0xhash{}", i),
                "0xrecipient".to_string(),
                1_000_000_000_000,
                1700000000 + i as u64,
                i as u64,
            );
            storage.add_transaction(record).unwrap();
        }

        // Newest should be first
        let history = storage.load_history();
        assert_eq!(history.len(), 5);
        assert_eq!(history.transactions[0].hash, "0xhash4");
        assert_eq!(history.transactions[4].hash, "0xhash0");
    }
}
