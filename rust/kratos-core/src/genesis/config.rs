// Configuration de la chaîne - Unified KratOs configuration
use crate::consensus::epoch::{EPOCH_DURATION_BLOCKS, SLOT_DURATION_SECS};
use crate::contracts::krat::{
    INITIAL_BURN_RATE_BPS, INITIAL_EMISSION_RATE_BPS, INITIAL_SUPPLY,
};
use serde::{Deserialize, Serialize};

/// Configuration de la chaîne
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// Nom de la chaîne
    pub chain_name: String,

    /// ID de la chaîne (0 = root chain)
    pub chain_id: u32,

    /// Configuration du consensus
    pub consensus: ConsensusConfig,

    /// Configuration du réseau
    pub network: NetworkConfig,

    /// Configuration de la tokenomics
    pub tokenomics: TokenomicsConfig,
}

/// Configuration du consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Durée d'une epoch (en blocs)
    pub epoch_duration: u64,

    /// Durée d'un slot (en secondes)
    pub slot_duration: u64,

    /// Nombre minimum de validateurs
    pub min_validators: usize,

    /// Nombre maximum de validateurs
    pub max_validators: usize,
}

/// Configuration du réseau
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Port d'écoute P2P
    pub listen_port: u16,

    /// Bootnodes (peers initiaux)
    pub bootnodes: Vec<String>,

    /// Nom du protocole
    pub protocol_name: String,

    /// Version du protocole
    pub protocol_version: u32,
}

/// Configuration de la tokenomics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenomicsConfig {
    /// Supply initiale
    pub initial_supply: u128,

    /// Taux d'émission initial (en bps)
    pub initial_emission_rate: u32,

    /// Taux de burn initial (en bps)
    pub initial_burn_rate: u32,
}

impl ChainConfig {
    /// Unified KratOs configuration
    /// This is the SINGLE SOURCE OF TRUTH for consensus, network, and tokenomics.
    pub fn mainnet() -> Self {
        Self {
            chain_name: "KratOs".to_string(),
            chain_id: 0,
            consensus: ConsensusConfig {
                epoch_duration: EPOCH_DURATION_BLOCKS,
                slot_duration: SLOT_DURATION_SECS,
                min_validators: 10,
                max_validators: 1000,
            },
            network: NetworkConfig {
                listen_port: 30333,
                bootnodes: vec![],
                protocol_name: "/kratos/1.0.0".to_string(),
                protocol_version: 1,
            },
            tokenomics: TokenomicsConfig {
                initial_supply: INITIAL_SUPPLY,
                initial_emission_rate: INITIAL_EMISSION_RATE_BPS,
                initial_burn_rate: INITIAL_BURN_RATE_BPS,
            },
        }
    }

    /// Charge depuis un fichier JSON
    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Sauvegarde vers un fichier JSON
    pub fn to_file(&self, path: &str) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self::mainnet()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_config() {
        let config = ChainConfig::mainnet();
        assert_eq!(config.chain_name, "KratOs");
        assert_eq!(config.chain_id, 0);
        assert_eq!(config.consensus.min_validators, 10);
    }

    #[test]
    fn test_default_config() {
        let config = ChainConfig::default();
        assert_eq!(config.chain_name, "KratOs");
        assert_eq!(config.chain_id, 0);
    }
}
