//! Simple DNS Server
//!
//! Implements a basic DNS server that responds to A and AAAA queries
//! with peer IP addresses from the registry.

use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::config::DnsSeedConfig;
use crate::dns::KratosDnsHandler;
use crate::registry::PeerRegistry;

/// DNS packet constants
const DNS_HEADER_SIZE: usize = 12;
const DNS_MAX_PACKET_SIZE: usize = 512;

/// DNS record types
const TYPE_A: u16 = 1;
const TYPE_AAAA: u16 = 28;
const TYPE_ANY: u16 = 255;

/// DNS flags
const FLAG_QR: u16 = 0x8000;  // Query/Response
const FLAG_AA: u16 = 0x0400;  // Authoritative Answer
const FLAG_RD: u16 = 0x0100;  // Recursion Desired

/// Run the DNS server
pub async fn run_dns_server(
    config: Arc<DnsSeedConfig>,
    registry: Arc<RwLock<PeerRegistry>>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], config.dns_port));

    // Create UDP socket
    let socket = UdpSocket::bind(addr)?;
    socket.set_nonblocking(true)?;

    let socket = Arc::new(tokio::net::UdpSocket::from_std(socket)?);

    info!("🌐 DNS server listening on {}", addr);

    let handler = Arc::new(KratosDnsHandler::new(
        registry,
        config.clone(),
    ));

    loop {
        let mut buf = [0u8; DNS_MAX_PACKET_SIZE];
        match socket.recv_from(&mut buf).await {
            Ok((len, src)) => {
                let request = buf[..len].to_vec();
                let handler = handler.clone();
                let socket = socket.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_dns_query(
                        socket.as_ref(),
                        src,
                        &request,
                        &handler,
                    ).await {
                        debug!("DNS query error from {}: {}", src, e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No data available, continue
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
            Err(e) => {
                error!("DNS socket error: {}", e);
            }
        }
    }
}

/// Handle a single DNS query
async fn handle_dns_query(
    socket: &tokio::net::UdpSocket,
    src: SocketAddr,
    request: &[u8],
    handler: &KratosDnsHandler,
) -> anyhow::Result<()> {
    if request.len() < DNS_HEADER_SIZE {
        return Ok(()); // Ignore malformed packets
    }

    // Parse header
    let id = u16::from_be_bytes([request[0], request[1]]);
    let flags = u16::from_be_bytes([request[2], request[3]]);
    let qdcount = u16::from_be_bytes([request[4], request[5]]);

    if qdcount == 0 {
        return Ok(()); // No questions
    }

    // Parse question section
    let (qname, qtype, _offset) = parse_question(&request[DNS_HEADER_SIZE..])?;

    debug!("DNS query: {} type {} from {}", qname, qtype, src);

    // Get peer IPs
    let result = handler.query(qtype == TYPE_AAAA).await;

    // Build response
    let response = build_dns_response(
        id,
        flags,
        &qname,
        qtype,
        &result.ipv4_addrs,
        &result.ipv6_addrs,
        result.ttl,
    )?;

    // Send response
    socket.send_to(&response, src).await?;

    Ok(())
}

/// Parse DNS question section
fn parse_question(data: &[u8]) -> anyhow::Result<(String, u16, usize)> {
    let mut name_parts = Vec::new();
    let mut offset = 0;

    // Parse name labels
    loop {
        if offset >= data.len() {
            anyhow::bail!("Truncated question");
        }

        let len = data[offset] as usize;
        if len == 0 {
            offset += 1;
            break;
        }

        if len > 63 {
            anyhow::bail!("Invalid label length");
        }

        offset += 1;
        if offset + len > data.len() {
            anyhow::bail!("Truncated label");
        }

        let label = std::str::from_utf8(&data[offset..offset + len])?;
        name_parts.push(label.to_lowercase());
        offset += len;
    }

    if offset + 4 > data.len() {
        anyhow::bail!("Truncated question");
    }

    let qtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
    let _qclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
    offset += 4;

    let name = name_parts.join(".");

    Ok((name, qtype, offset))
}

/// Build DNS response packet
fn build_dns_response(
    id: u16,
    request_flags: u16,
    qname: &str,
    qtype: u16,
    ipv4_addrs: &[std::net::Ipv4Addr],
    ipv6_addrs: &[std::net::Ipv6Addr],
    ttl: u32,
) -> anyhow::Result<Vec<u8>> {
    let mut response = Vec::with_capacity(DNS_MAX_PACKET_SIZE);

    // Count answers based on query type
    let ancount = match qtype {
        TYPE_A => ipv4_addrs.len() as u16,
        TYPE_AAAA => ipv6_addrs.len() as u16,
        TYPE_ANY => (ipv4_addrs.len() + ipv6_addrs.len()) as u16,
        _ => 0,
    };

    // Build header
    let flags = FLAG_QR | FLAG_AA | (request_flags & FLAG_RD);

    response.extend_from_slice(&id.to_be_bytes());
    response.extend_from_slice(&flags.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes()); // qdcount = 1
    response.extend_from_slice(&ancount.to_be_bytes()); // ancount
    response.extend_from_slice(&0u16.to_be_bytes()); // nscount = 0
    response.extend_from_slice(&0u16.to_be_bytes()); // arcount = 0

    // Build question section (echo back)
    let qname_offset = response.len();
    for part in qname.split('.') {
        response.push(part.len() as u8);
        response.extend_from_slice(part.as_bytes());
    }
    response.push(0); // End of name

    response.extend_from_slice(&qtype.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes()); // IN class

    // Build answer section
    let name_ptr = 0xC000 | (qname_offset as u16); // Compression pointer

    // Add A records
    if qtype == TYPE_A || qtype == TYPE_ANY {
        for ip in ipv4_addrs {
            if response.len() + 16 > DNS_MAX_PACKET_SIZE {
                break; // Stop if we'd exceed packet size
            }

            response.extend_from_slice(&name_ptr.to_be_bytes());
            response.extend_from_slice(&TYPE_A.to_be_bytes());
            response.extend_from_slice(&1u16.to_be_bytes()); // IN class
            response.extend_from_slice(&ttl.to_be_bytes());
            response.extend_from_slice(&4u16.to_be_bytes()); // rdlength
            response.extend_from_slice(&ip.octets());
        }
    }

    // Add AAAA records
    if qtype == TYPE_AAAA || qtype == TYPE_ANY {
        for ip in ipv6_addrs {
            if response.len() + 28 > DNS_MAX_PACKET_SIZE {
                break; // Stop if we'd exceed packet size
            }

            response.extend_from_slice(&name_ptr.to_be_bytes());
            response.extend_from_slice(&TYPE_AAAA.to_be_bytes());
            response.extend_from_slice(&1u16.to_be_bytes()); // IN class
            response.extend_from_slice(&ttl.to_be_bytes());
            response.extend_from_slice(&16u16.to_be_bytes()); // rdlength
            response.extend_from_slice(&ip.octets());
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_question() {
        // Build a test DNS question for "seed.kratos.network"
        let mut data = Vec::new();
        data.push(4); // "seed"
        data.extend_from_slice(b"seed");
        data.push(6); // "kratos"
        data.extend_from_slice(b"kratos");
        data.push(7); // "network"
        data.extend_from_slice(b"network");
        data.push(0); // end
        data.extend_from_slice(&1u16.to_be_bytes()); // A record
        data.extend_from_slice(&1u16.to_be_bytes()); // IN class

        let (name, qtype, _) = parse_question(&data).unwrap();
        assert_eq!(name, "seed.kratos.network");
        assert_eq!(qtype, TYPE_A);
    }

    #[test]
    fn test_build_response() {
        let ipv4 = vec![
            std::net::Ipv4Addr::new(192, 168, 1, 1),
            std::net::Ipv4Addr::new(10, 0, 0, 1),
        ];
        let ipv6 = vec![];

        let response = build_dns_response(
            0x1234,
            FLAG_RD,
            "seed.kratos.network",
            TYPE_A,
            &ipv4,
            &ipv6,
            60,
        ).unwrap();

        // Check header
        assert_eq!(response[0..2], [0x12, 0x34]); // ID
        // Check answer count
        assert_eq!(u16::from_be_bytes([response[6], response[7]]), 2);
    }
}
