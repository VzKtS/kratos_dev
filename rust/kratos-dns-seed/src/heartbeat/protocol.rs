//! Heartbeat Protocol Constants and Utilities

/// Protocol version for heartbeat messages
pub const HEARTBEAT_PROTOCOL_VERSION: u32 = 1;

/// Magic bytes for protocol identification
pub const PROTOCOL_MAGIC: [u8; 4] = *b"KRAT";

/// Message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    Heartbeat = 1,
    HeartbeatResponse = 2,
    PeersRequest = 3,
    PeersResponse = 4,
}

impl TryFrom<u8> for MessageType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(MessageType::Heartbeat),
            2 => Ok(MessageType::HeartbeatResponse),
            3 => Ok(MessageType::PeersRequest),
            4 => Ok(MessageType::PeersResponse),
            _ => Err(()),
        }
    }
}
