//! Peer Scoring System
//!
//! Tracks peer health and reliability using a scoring system.
//! Scores range from 0-200, with higher scores indicating better peers.

/// Initial score for new peers
pub const INITIAL_SCORE: i32 = 100;

/// Maximum peer score
pub const MAX_SCORE: i32 = 200;

/// Minimum peer score (peers below this are removed)
pub const MIN_SCORE: i32 = 0;

/// Score adjustments for various events
pub mod adjustments {
    /// Score increase for successful heartbeat
    pub const HEARTBEAT_SUCCESS: i32 = 1;

    /// Score decrease for missed heartbeat
    pub const HEARTBEAT_MISS: i32 = -10;

    /// Bonus for being a validator
    pub const VALIDATOR_BONUS: i32 = 20;

    /// Score decrease for stale data
    pub const STALE_PENALTY: i32 = -5;

    /// Score increase for consistent uptime (per hour)
    pub const UPTIME_BONUS: i32 = 2;
}

/// Calculate score adjustment based on peer behavior
pub fn calculate_adjustment(
    is_validator: bool,
    heartbeat_received: bool,
    hours_uptime: u64,
) -> i32 {
    let mut adjustment = 0;

    if heartbeat_received {
        adjustment += adjustments::HEARTBEAT_SUCCESS;
    } else {
        adjustment += adjustments::HEARTBEAT_MISS;
    }

    if is_validator {
        adjustment += adjustments::VALIDATOR_BONUS / 10; // Partial bonus per heartbeat
    }

    // Cap uptime bonus
    let uptime_bonus = (hours_uptime as i32 * adjustments::UPTIME_BONUS).min(50);
    adjustment += uptime_bonus / 24; // Spread over day

    adjustment
}

/// Clamp score to valid range
pub fn clamp_score(score: i32) -> i32 {
    score.max(MIN_SCORE).min(MAX_SCORE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_constants() {
        assert!(INITIAL_SCORE > MIN_SCORE);
        assert!(INITIAL_SCORE < MAX_SCORE);
    }

    #[test]
    fn test_clamp_score() {
        assert_eq!(clamp_score(-50), MIN_SCORE);
        assert_eq!(clamp_score(300), MAX_SCORE);
        assert_eq!(clamp_score(100), 100);
    }

    #[test]
    fn test_adjustment_calculation() {
        // Successful heartbeat
        let adj = calculate_adjustment(false, true, 0);
        assert!(adj > 0);

        // Missed heartbeat
        let adj = calculate_adjustment(false, false, 0);
        assert!(adj < 0);

        // Validator bonus
        let adj_normal = calculate_adjustment(false, true, 0);
        let adj_validator = calculate_adjustment(true, true, 0);
        assert!(adj_validator > adj_normal);
    }
}
