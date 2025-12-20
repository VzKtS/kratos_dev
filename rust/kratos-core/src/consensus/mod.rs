// Consensus - Slow and resilient PoS
// Principle: Fast power is dangerous. Slowness is a safeguard.

pub mod pos;
pub mod validator;
pub mod validator_credits;
pub mod vrf_selection;
pub mod epoch;
pub mod validation;
pub mod metrics;
pub mod slashing;
pub mod vc_decay;
pub mod economics;
pub mod clock_health;

