use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;
use chrono::{DateTime, Utc};
use tokio::net::TcpStream;
use tokio::time::timeout;
use crate::output::errors::NexusError;

/// Network scanner for discovering hosts on a network
pub struct NetworkScanner {
    pub timeout: Duration,
    pub concurrent_probes: usize,
    pub fingerprint: bool,
    pub probe_type: ProbeType,
}

/// Represents a discovered host on the network
#[derive(Debug, Clone)]
pub struct DiscoveredHost {
    pub address: IpAddr,
    pub hostname: Option<String>,
    pub open_ports: Vec<OpenPort>,
    pub os_classification: Option<OsClassification>,
    pub fingerprint: Option<Fingerprint>,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub response_time: Duration,
}

/// Information about an open port on a host
#[derive(Debug, Clone)]
pub struct OpenPort {
    pub port: u16,
    pub service: Option<String>,
    pub banner: Option<String>,
}

/// OS classification information
#[derive(Debug, Clone)]
pub struct OsClassification {
    pub os_family: String,          // linux, windows, bsd
    pub distribution: Option<String>,  // ubuntu, rhel, debian
    pub confidence: f32,            // 0.0 - 1.0
}

/// Fingerprint information gathered from the host
#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub ssh_banner: Option<String>,
    pub tcp_timestamps: Option<bool>,
    pub ttl: Option<u8>,
}

/// Discovery mode configuration
#[derive(Debug, Clone)]
pub enum DiscoveryMode {
    Active,
    Passive { from_arp: bool },
}

/// Types of probes to perform
#[derive(Debug, Clone)]
pub enum ProbeType {
    Ssh,
    Ping,
    TcpPorts(Vec<u16>),
}

impl NetworkScanner {
    /// Create a new network scanner with default settings
    pub fn new() -> Self {
        NetworkScanner {
            timeout: Duration::from_secs(2),
            concurrent_probes: 100,
            fingerprint: true,
            probe_type: ProbeType::Ssh,
        }
    }

    /// Create a new network scanner with specific probe type
    pub fn with_probe_type(mut self, probe_type: ProbeType) -> Self {
        self.probe_type = probe_type;
        self
    }

    /// Get the ports to scan based on probe type
    fn get_probe_ports(&self) -> Vec<u16> {
        match &self.probe_type {
            ProbeType::Ssh => vec![22],
            ProbeType::Ping => vec![22, 80, 443], // TCP ping to common ports
            ProbeType::TcpPorts(ports) => ports.clone(),
        }
    }

    /// Scan a subnet using CIDR notation (e.g., "192.168.1.0/24")
    pub async fn scan_subnet(&self, cidr: &str) -> Result<Vec<DiscoveredHost>, NexusError> {
        let ips = parse_cidr(cidr)?;
        let mut discovered = Vec::new();

        // Use semaphore to limit concurrent probes
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(self.concurrent_probes));
        let mut tasks = Vec::new();

        let ports = self.get_probe_ports();
        let require_ssh = matches!(self.probe_type, ProbeType::Ssh);

        for ip in ips {
            let sem_clone = sem.clone();
            let timeout_duration = self.timeout;
            let fingerprint = self.fingerprint;
            let ports_clone = ports.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                let host = Self::probe_host_internal(ip, &ports_clone, timeout_duration, fingerprint).await;

                // For SSH probe type, only return hosts with port 22 open
                if require_ssh {
                    host.filter(|h| h.open_ports.iter().any(|p| p.port == 22))
                } else {
                    host
                }
            }));
        }

        // Collect results
        for task in tasks {
            if let Ok(Some(host)) = task.await {
                discovered.push(host);
            }
        }

        Ok(discovered)
    }

    /// Probe a specific host on given ports
    pub async fn probe_host(&self, addr: IpAddr, ports: &[u16]) -> Option<DiscoveredHost> {
        Self::probe_host_internal(addr, ports, self.timeout, self.fingerprint).await
    }

    /// Internal probe implementation
    async fn probe_host_internal(
        addr: IpAddr,
        ports: &[u16],
        timeout_duration: Duration,
        do_fingerprint: bool,
    ) -> Option<DiscoveredHost> {
        let start = std::time::Instant::now();
        let mut open_ports = Vec::new();

        // Probe each port
        for &port in ports {
            if let Ok(Ok(_stream)) = timeout(
                timeout_duration,
                TcpStream::connect((addr, port))
            ).await {
                let mut open_port = OpenPort {
                    port,
                    service: identify_service(port),
                    banner: None,
                };

                // Try to grab banner if fingerprinting is enabled
                if do_fingerprint && port == 22 {
                    open_port.banner = Self::grab_ssh_banner(addr, port, timeout_duration).await;
                }

                open_ports.push(open_port);
            }
        }

        // Only return if we found at least one open port
        if open_ports.is_empty() {
            return None;
        }

        let response_time = start.elapsed();
        let now = Utc::now();

        let fingerprint = if do_fingerprint {
            Some(Self::fingerprint_host(&open_ports))
        } else {
            None
        };

        let os_classification = if do_fingerprint {
            Some(Self::classify_os(&open_ports, fingerprint.as_ref()))
        } else {
            None
        };

        Some(DiscoveredHost {
            address: addr,
            hostname: Self::resolve_hostname(addr).await,
            open_ports,
            os_classification,
            fingerprint,
            first_seen: now,
            last_seen: now,
            response_time,
        })
    }

    /// Grab SSH banner from a host
    async fn grab_ssh_banner(addr: IpAddr, port: u16, timeout_duration: Duration) -> Option<String> {
        use tokio::io::AsyncReadExt;

        match timeout(timeout_duration, TcpStream::connect((addr, port))).await {
            Ok(Ok(mut stream)) => {
                let mut buffer = [0u8; 256];
                match timeout(Duration::from_millis(500), stream.read(&mut buffer)).await {
                    Ok(Ok(n)) if n > 0 => {
                        let banner = String::from_utf8_lossy(&buffer[..n]).to_string();
                        Some(banner.trim().to_string())
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Create a fingerprint from discovered information
    fn fingerprint_host(open_ports: &[OpenPort]) -> Fingerprint {
        let ssh_banner = open_ports
            .iter()
            .find(|p| p.port == 22)
            .and_then(|p| p.banner.clone());

        Fingerprint {
            ssh_banner,
            tcp_timestamps: None,  // Would require raw socket access
            ttl: None,             // Would require raw socket access
        }
    }

    /// Classify the OS based on fingerprint information
    fn classify_os(open_ports: &[OpenPort], fingerprint: Option<&Fingerprint>) -> OsClassification {
        let mut os_family = "unknown".to_string();
        let mut distribution = None;
        let mut confidence: f32 = 0.0;

        // Check SSH banner for OS hints
        if let Some(fp) = fingerprint {
            if let Some(banner) = &fp.ssh_banner {
                let banner_lower = banner.to_lowercase();

                if banner_lower.contains("ubuntu") {
                    os_family = "linux".to_string();
                    distribution = Some("ubuntu".to_string());
                    confidence = 0.9;
                } else if banner_lower.contains("debian") {
                    os_family = "linux".to_string();
                    distribution = Some("debian".to_string());
                    confidence = 0.85;
                } else if banner_lower.contains("rhel") || banner_lower.contains("centos") {
                    os_family = "linux".to_string();
                    distribution = Some("rhel".to_string());
                    confidence = 0.85;
                } else if banner_lower.contains("openssh") {
                    os_family = "linux".to_string();
                    confidence = 0.6;
                } else if banner_lower.contains("windows") {
                    os_family = "windows".to_string();
                    confidence = 0.8;
                }
            }
        }

        // Check for Windows-specific ports
        if open_ports.iter().any(|p| p.port == 3389) {
            os_family = "windows".to_string();
            confidence = confidence.max(0.7);
        }

        OsClassification {
            os_family,
            distribution,
            confidence,
        }
    }

    /// Attempt to resolve hostname from IP
    async fn resolve_hostname(addr: IpAddr) -> Option<String> {
        // Perform reverse DNS lookup in a blocking task
        tokio::task::spawn_blocking(move || {
            match dns_lookup::lookup_addr(&addr) {
                Ok(hostname) => {
                    // Strip trailing dot from DNS names if present
                    let hostname = hostname.trim_end_matches('.').to_string();
                    // Return None if hostname is empty or same as IP address
                    if hostname.is_empty() || hostname == addr.to_string() {
                        None
                    } else {
                        Some(hostname)
                    }
                }
                Err(_) => None,
            }
        })
        .await
        .ok()
        .flatten()
    }
}

impl Default for NetworkScanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse CIDR notation into a list of IP addresses
fn parse_cidr(cidr: &str) -> Result<Vec<IpAddr>, NexusError> {
    let parts: Vec<&str> = cidr.split('/').collect();

    if parts.len() != 2 {
        return Err(NexusError::Inventory {
            message: format!("Invalid CIDR notation: {}", cidr),
            suggestion: Some("Use format like '192.168.1.0/24'".to_string()),
        });
    }

    let base_ip: Ipv4Addr = parts[0].parse().map_err(|_| NexusError::Inventory {
        message: format!("Invalid IP address: {}", parts[0]),
        suggestion: None,
    })?;

    let prefix_len: u8 = parts[1].parse().map_err(|_| NexusError::Inventory {
        message: format!("Invalid prefix length: {}", parts[1]),
        suggestion: Some("Prefix length should be between 0 and 32".to_string()),
    })?;

    if prefix_len > 32 {
        return Err(NexusError::Inventory {
            message: format!("Prefix length {} is too large", prefix_len),
            suggestion: Some("Prefix length should be between 0 and 32".to_string()),
        });
    }

    // Calculate number of hosts
    let num_hosts = 2u32.pow((32 - prefix_len) as u32);

    // Limit to reasonable subnet sizes
    if num_hosts > 65536 {
        return Err(NexusError::Inventory {
            message: format!("Subnet too large: {} hosts", num_hosts),
            suggestion: Some("Use a prefix length of /16 or higher".to_string()),
        });
    }

    let base_u32 = u32::from(base_ip);
    let mask = !0u32 << (32 - prefix_len);
    let network = base_u32 & mask;

    let mut ips = Vec::new();
    for i in 1..num_hosts - 1 {
        // Skip network and broadcast addresses
        let ip_u32 = network + i;
        ips.push(IpAddr::V4(Ipv4Addr::from(ip_u32)));
    }

    Ok(ips)
}

/// Identify common services by port number
fn identify_service(port: u16) -> Option<String> {
    match port {
        22 => Some("ssh".to_string()),
        80 => Some("http".to_string()),
        443 => Some("https".to_string()),
        3389 => Some("rdp".to_string()),
        3306 => Some("mysql".to_string()),
        5432 => Some("postgresql".to_string()),
        6379 => Some("redis".to_string()),
        27017 => Some("mongodb".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cidr() {
        let ips = parse_cidr("192.168.1.0/30").unwrap();
        assert_eq!(ips.len(), 2); // Only .1 and .2 (skip network and broadcast)
    }

    #[test]
    fn test_parse_cidr_invalid() {
        assert!(parse_cidr("invalid").is_err());
        assert!(parse_cidr("192.168.1.0/33").is_err());
    }

    #[test]
    fn test_identify_service() {
        assert_eq!(identify_service(22), Some("ssh".to_string()));
        assert_eq!(identify_service(80), Some("http".to_string()));
        assert_eq!(identify_service(9999), None);
    }
}
