// Execution - Deterministic state machine
// Principle: No Turing-complete, simple and verifiable transactions
//
// NOTE: The deprecated `executor` and `runtime` modules have been removed.
// Transaction execution is now handled by `TransactionExecutor` in `node/producer.rs`
// Block execution is coordinated by `NodeService` in `node/service.rs`

pub mod gas;

// Re-export the correct executor from node/producer for convenience
// (The actual implementation lives there to avoid circular dependencies)

