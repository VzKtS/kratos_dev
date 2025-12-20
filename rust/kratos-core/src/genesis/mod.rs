// Genesis - Configuration et bloc genesis
pub mod config;
pub mod spec;

pub use config::ChainConfig;
pub use spec::{GenesisSpec, GenesisBuilder};
