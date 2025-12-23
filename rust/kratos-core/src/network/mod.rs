// Network - P2P networking layer using libp2p
// Principle: Simple gossip, automatic peer discovery, peer scoring

pub mod behaviour;
pub mod dns_seeds;
pub mod dns_seed_client;
pub mod peer;
pub mod protocol;
pub mod rate_limit;
pub mod request;
pub mod service;
pub mod sync;
pub mod warp_sync;

pub use dns_seeds::{DnsSeedResolver, DnsSeedRegistry, DnsSeedInfo, parse_bootnode};
pub use dns_seed_client::{
    DnsSeedClient, HeartbeatService, HeartbeatMessage, HeartbeatResponse,
    NetworkStateInfo, SecurityState, IdPeersFile, NodeInfo,
    HEARTBEAT_PORT, HEARTBEAT_INTERVAL_SECS,
};
pub use peer::{PeerManager, PeerInfo, PeerState, PeerStats};
pub use request::{
    BlockRequest, BlockResponse, SyncRequest, SyncResponse,
    StatusRequest, StatusResponse, KratosRequest, KratosResponse,
};
pub use service::{NetworkService, NetworkEvent};
pub use sync::{SyncManager, SyncState};
pub use warp_sync::{WarpSyncManager, WarpSyncState, StateSnapshot};

