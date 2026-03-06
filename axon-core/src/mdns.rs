use crate::protocol::{Capability, PeerInfo};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, info};

const SERVICE_TYPE: &str = "_axon._udp.local.";

/// Events emitted by mDNS discovery.
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    PeerDiscovered(PeerInfo),
    PeerRemoved(Vec<u8>),
}

/// mDNS-based peer discovery for the local network.
pub struct MdnsDiscovery {
    daemon: ServiceDaemon,
    peer_id_hex: String,
}

impl MdnsDiscovery {
    /// Register this node on the local network and start browsing for peers.
    pub fn new(
        peer_id_hex: String,
        port: u16,
        capabilities: Vec<Capability>,
    ) -> Result<(Self, mpsc::Receiver<DiscoveryEvent>), mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;

        // Register our service
        let instance_name = format!("axon-{}", &peer_id_hex[..8]);
        let cap_tags: Vec<String> = capabilities.iter().map(|c| c.tag()).collect();

        let mut properties = HashMap::new();
        properties.insert("peer_id".to_string(), peer_id_hex.clone());
        properties.insert("caps".to_string(), cap_tags.join(","));

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &instance_name,
            "",
            port,
            properties,
        )?;

        daemon.register(service_info)?;
        info!("Registered mDNS service: {}.{}", instance_name, SERVICE_TYPE);

        // Browse for peers
        let receiver = daemon.browse(SERVICE_TYPE)?;

        // Spawn a task to process mDNS events and forward them
        let (event_tx, event_rx) = mpsc::channel(64);
        let my_peer_id = peer_id_hex.clone();

        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        // Extract peer info from properties
                        if let Some(peer_id_str) = info.get_property_val_str("peer_id") {
                            // Don't discover ourselves
                            if peer_id_str == my_peer_id {
                                continue;
                            }

                            let caps_str = info
                                .get_property_val_str("caps")
                                .unwrap_or("")
                                .to_string();
                            let capabilities = parse_capability_tags(&caps_str);

                            let addr = if let Some(addr) = info.get_addresses().iter().next() {
                                format!("{}:{}", addr, info.get_port())
                            } else {
                                continue;
                            };

                            let peer_id = hex_decode(peer_id_str);

                            let peer_info = PeerInfo {
                                peer_id,
                                addr,
                                capabilities,
                                last_seen: now_secs(),
                            };

                            info!("mDNS: discovered peer {} at {}", peer_id_str, peer_info.addr);
                            let _ = event_tx.blocking_send(DiscoveryEvent::PeerDiscovered(peer_info));
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        debug!("mDNS: service removed: {}", fullname);
                    }
                    ServiceEvent::SearchStarted(_) => {
                        debug!("mDNS: search started");
                    }
                    _ => {}
                }
            }
        });

        Ok((
            Self {
                daemon,
                peer_id_hex,
            },
            event_rx,
        ))
    }

    /// Unregister from mDNS.
    pub fn shutdown(&self) {
        let instance_name = format!("axon-{}", &self.peer_id_hex[..8]);
        let fullname = format!("{}.{}", instance_name, SERVICE_TYPE);
        let _ = self.daemon.unregister(&fullname);
        let _ = self.daemon.shutdown();
        info!("mDNS discovery shut down");
    }
}

fn parse_capability_tags(s: &str) -> Vec<Capability> {
    if s.is_empty() {
        return vec![];
    }
    s.split(',')
        .filter_map(|tag| {
            let parts: Vec<&str> = tag.split(':').collect();
            if parts.len() == 3 {
                let version = parts[2]
                    .strip_prefix('v')
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                Some(Capability::new(parts[0], parts[1], version))
            } else {
                None
            }
        })
        .collect()
}

fn hex_decode(s: &str) -> Vec<u8> {
    if !s.is_ascii() {
        return vec![];
    }
    // Guard against odd-length strings to avoid out-of-bounds slice
    let len = s.len() & !1; // round down to even
    (0..len)
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_capabilities() {
        let caps = parse_capability_tags("");
        assert!(caps.is_empty());
    }

    #[test]
    fn parse_single_capability() {
        let caps = parse_capability_tags("llm:chat:v1");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].namespace, "llm");
        assert_eq!(caps[0].name, "chat");
        assert_eq!(caps[0].version, 1);
    }

    #[test]
    fn parse_multiple_capabilities() {
        let caps = parse_capability_tags("llm:chat:v1,code:review:v2,echo:ping:v1");
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[1].namespace, "code");
        assert_eq!(caps[1].version, 2);
    }

    #[test]
    fn parse_malformed_capability_skipped() {
        let caps = parse_capability_tags("llm:chat:v1,bad,echo:ping:v1");
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn hex_decode_valid() {
        let result = hex_decode("48656c6c6f");
        assert_eq!(result, b"Hello");
    }

    #[test]
    fn hex_decode_peer_id() {
        let original = vec![0xab, 0xcd, 0xef, 0x12];
        let hex_str: String = original.iter().map(|b| format!("{:02x}", b)).collect();
        let decoded = hex_decode(&hex_str);
        assert_eq!(decoded, original);
    }

    #[test]
    fn hex_decode_non_ascii_returns_empty() {
        let result = hex_decode("caf\u{00e9}");
        assert!(result.is_empty());
    }

    #[test]
    fn hex_decode_empty_string() {
        let result = hex_decode("");
        assert!(result.is_empty());
    }

    #[test]
    fn hex_decode_odd_length() {
        let result = hex_decode("abc");
        assert_eq!(result, vec![0xab]);
    }
}
