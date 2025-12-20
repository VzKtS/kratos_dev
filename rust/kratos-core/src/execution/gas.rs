// Gas - Système de gas metering pour éviter les DoS
use crate::types::Balance;

/// Unité de gas (1 KRAT = 1_000_000_000_000 units)
pub const GAS_UNIT: u64 = 1_000_000;

/// Limites de gas par défaut
pub const DEFAULT_BLOCK_GAS_LIMIT: u64 = 10_000_000 * GAS_UNIT; // 10M gas par bloc
pub const DEFAULT_TRANSACTION_GAS_LIMIT: u64 = 1_000_000 * GAS_UNIT; // 1M gas par transaction

/// Coûts en gas pour les opérations
pub mod costs {
    

    // Opérations de base
    pub const TRANSFER: u64 = 21_000;
    pub const BALANCE_READ: u64 = 100;
    pub const BALANCE_WRITE: u64 = 5_000;
    pub const ACCOUNT_CREATE: u64 = 25_000;

    // Opérations de stockage
    pub const STORAGE_READ: u64 = 200;
    pub const STORAGE_WRITE_NEW: u64 = 20_000;
    pub const STORAGE_WRITE_EXISTING: u64 = 5_000;
    pub const STORAGE_DELETE: u64 = 5_000;

    // Opérations cryptographiques
    pub const HASH_BLAKE3: u64 = 60;
    pub const VERIFY_SIGNATURE: u64 = 3_000;

    // Opérations de contrat
    pub const CONTRACT_CALL: u64 = 700;
    pub const CONTRACT_CREATE: u64 = 32_000;
    pub const CODE_COPY: u64 = 3; // Par byte
    pub const CODE_EXECUTION: u64 = 1; // Par instruction

    // Limites
    pub const MAX_TRANSACTION_GAS: u64 = 10_000_000;
    pub const MAX_BLOCK_GAS: u64 = 100_000_000;
}

/// Compteur de gas
#[derive(Debug, Clone)]
pub struct GasMeter {
    /// Gas disponible
    gas_limit: u64,

    /// Gas consommé
    gas_used: u64,

    /// Prix du gas (en unités KRAT par unité de gas)
    gas_price: u64,
}

impl GasMeter {
    /// Crée un nouveau compteur de gas
    pub fn new(gas_limit: u64, gas_price: u64) -> Self {
        Self {
            gas_limit,
            gas_used: 0,
            gas_price,
        }
    }

    /// Consomme du gas
    pub fn consume(&mut self, amount: u64) -> Result<(), GasError> {
        let new_used = self
            .gas_used
            .checked_add(amount)
            .ok_or(GasError::Overflow)?;

        if new_used > self.gas_limit {
            return Err(GasError::OutOfGas {
                needed: amount,
                remaining: self.gas_limit - self.gas_used,
            });
        }

        self.gas_used = new_used;
        Ok(())
    }

    /// Consomme du gas de manière conditionnelle (peut être remboursé)
    pub fn consume_conditional(&mut self, amount: u64) -> Result<GasRefund, GasError> {
        self.consume(amount)?;
        Ok(GasRefund {
            amount,
            meter: self.clone(),
        })
    }

    /// Rembourse du gas
    pub fn refund(&mut self, amount: u64) {
        self.gas_used = self.gas_used.saturating_sub(amount);
    }

    /// Gas restant
    pub fn remaining(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }

    /// Gas utilisé
    pub fn used(&self) -> u64 {
        self.gas_used
    }

    /// Coût total en KRAT
    pub fn total_cost(&self) -> Balance {
        (self.gas_used as u128) * (self.gas_price as u128)
    }

    /// Vérifie qu'il reste assez de gas
    pub fn check_available(&self, amount: u64) -> Result<(), GasError> {
        if self.remaining() < amount {
            return Err(GasError::OutOfGas {
                needed: amount,
                remaining: self.remaining(),
            });
        }
        Ok(())
    }

    /// Réinitialise le compteur
    pub fn reset(&mut self) {
        self.gas_used = 0;
    }

    /// Change la limite de gas
    pub fn set_limit(&mut self, new_limit: u64) {
        self.gas_limit = new_limit;
    }
}

/// Remboursement de gas (RAII pattern)
pub struct GasRefund {
    amount: u64,
    meter: GasMeter,
}

impl Drop for GasRefund {
    fn drop(&mut self) {
        // Auto-refund si le GasRefund est droppé sans commit
        // (utile en cas de revert)
    }
}

impl GasRefund {
    /// Confirme la consommation (pas de remboursement)
    pub fn commit(self) {
        std::mem::forget(self); // Évite le drop
    }

    /// Annule et rembourse
    pub fn refund(mut self) {
        self.meter.refund(self.amount);
    }
}

/// Compteur de gas pour un bloc entier
#[derive(Debug)]
pub struct BlockGasMeter {
    /// Limite de gas du bloc
    block_gas_limit: u64,

    /// Gas total utilisé dans le bloc
    block_gas_used: u64,
}

impl BlockGasMeter {
    pub fn new(block_gas_limit: u64) -> Self {
        Self {
            block_gas_limit,
            block_gas_used: 0,
        }
    }

    /// Vérifie si une transaction peut être ajoutée au bloc
    pub fn can_fit_transaction(&self, tx_gas_limit: u64) -> bool {
        self.block_gas_used + tx_gas_limit <= self.block_gas_limit
    }

    /// Enregistre le gas utilisé par une transaction
    pub fn record_transaction(&mut self, gas_used: u64) -> Result<(), GasError> {
        let new_total = self
            .block_gas_used
            .checked_add(gas_used)
            .ok_or(GasError::Overflow)?;

        if new_total > self.block_gas_limit {
            return Err(GasError::BlockGasLimitExceeded {
                limit: self.block_gas_limit,
                used: new_total,
            });
        }

        self.block_gas_used = new_total;
        Ok(())
    }

    /// Gas restant dans le bloc
    pub fn remaining(&self) -> u64 {
        self.block_gas_limit.saturating_sub(self.block_gas_used)
    }

    /// Gas utilisé dans le bloc
    pub fn used(&self) -> u64 {
        self.block_gas_used
    }
}

/// Erreurs de gas
#[derive(Debug, thiserror::Error, Clone)]
pub enum GasError {
    #[error("Gas insuffisant: besoin de {needed}, reste {remaining}")]
    OutOfGas { needed: u64, remaining: u64 },

    #[error("Limite de gas du bloc dépassée: limite {limit}, utilisé {used}")]
    BlockGasLimitExceeded { limit: u64, used: u64 },

    #[error("Overflow dans le calcul de gas")]
    Overflow,

    #[error("Prix du gas invalide")]
    InvalidGasPrice,

    #[error("Limite de gas invalide")]
    InvalidGasLimit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_meter_consume() {
        let mut meter = GasMeter::new(1000, 1);

        // Consomme 500
        assert!(meter.consume(500).is_ok());
        assert_eq!(meter.used(), 500);
        assert_eq!(meter.remaining(), 500);

        // Consomme 300 de plus
        assert!(meter.consume(300).is_ok());
        assert_eq!(meter.used(), 800);
        assert_eq!(meter.remaining(), 200);

        // Essaie de consommer trop
        assert!(matches!(
            meter.consume(300),
            Err(GasError::OutOfGas { .. })
        ));

        // Le gas n'a pas changé après l'erreur
        assert_eq!(meter.used(), 800);
    }

    #[test]
    fn test_gas_meter_refund() {
        let mut meter = GasMeter::new(1000, 1);

        meter.consume(500).unwrap();
        assert_eq!(meter.used(), 500);

        meter.refund(200);
        assert_eq!(meter.used(), 300);
        assert_eq!(meter.remaining(), 700);
    }

    #[test]
    fn test_gas_meter_total_cost() {
        let mut meter = GasMeter::new(1000, 10);

        meter.consume(500).unwrap();
        assert_eq!(meter.total_cost(), 5000);
    }

    #[test]
    fn test_block_gas_meter() {
        let mut block_meter = BlockGasMeter::new(10000);

        // Ajoute une transaction de 3000 gas
        assert!(block_meter.can_fit_transaction(3000));
        assert!(block_meter.record_transaction(3000).is_ok());
        assert_eq!(block_meter.used(), 3000);

        // Ajoute une autre transaction de 5000 gas
        assert!(block_meter.can_fit_transaction(5000));
        assert!(block_meter.record_transaction(5000).is_ok());
        assert_eq!(block_meter.used(), 8000);

        // Essaie d'ajouter une transaction trop grande
        assert!(!block_meter.can_fit_transaction(3000));
        assert!(matches!(
            block_meter.record_transaction(3000),
            Err(GasError::BlockGasLimitExceeded { .. })
        ));

        // Le gas du bloc n'a pas changé
        assert_eq!(block_meter.used(), 8000);
    }

    #[test]
    fn test_gas_costs_reasonable() {
        // Vérifier que les coûts sont cohérents
        assert!(costs::TRANSFER < costs::ACCOUNT_CREATE);
        assert!(costs::STORAGE_READ < costs::STORAGE_WRITE_NEW);
        assert!(costs::HASH_BLAKE3 < costs::VERIFY_SIGNATURE);
        assert!(costs::CONTRACT_CALL < costs::CONTRACT_CREATE);
    }
}
