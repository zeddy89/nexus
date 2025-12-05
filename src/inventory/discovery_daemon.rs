use super::discovery::NetworkScanner;
use crate::output::errors::NexusError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Daemon for continuous network discovery and monitoring
pub struct DiscoveryDaemon {
    pub watch_subnets: Vec<String>,
    pub interval: Duration,
    pub notifiers: Vec<Notifier>,
    pub state_file: PathBuf,
    scanner: NetworkScanner,
    state: DiscoveryState,
}

/// Notification destination for discovery events
#[derive(Debug, Clone)]
pub enum Notifier {
    Webhook { url: String },
    File { path: PathBuf },
    Stdout,
}

/// Events that can occur during network monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeEvent {
    HostDiscovered {
        address: IpAddr,
        hostname: Option<String>,
        open_ports: Vec<u16>,
        first_seen: DateTime<Utc>,
    },
    HostDisappeared {
        address: IpAddr,
        last_seen: DateTime<Utc>,
    },
    PortOpened {
        host: IpAddr,
        port: u16,
        service: Option<String>,
    },
    PortClosed {
        host: IpAddr,
        port: u16,
    },
}

/// Persistent state for the discovery daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryState {
    hosts: HashMap<IpAddr, HostState>,
    last_scan: Option<DateTime<Utc>>,
}

/// State information for a single host
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostState {
    address: IpAddr,
    hostname: Option<String>,
    open_ports: Vec<u16>,
    first_seen: DateTime<Utc>,
    last_seen: DateTime<Utc>,
}

impl DiscoveryDaemon {
    /// Create a new discovery daemon
    pub fn new(subnets: Vec<String>, interval: Duration) -> Self {
        DiscoveryDaemon {
            watch_subnets: subnets,
            interval,
            notifiers: vec![Notifier::Stdout],
            state_file: PathBuf::from("/tmp/nexus_discovery_state.json"),
            scanner: NetworkScanner::new(),
            state: DiscoveryState {
                hosts: HashMap::new(),
                last_scan: None,
            },
        }
    }

    /// Add a notifier to the daemon
    pub fn with_notifier(mut self, notifier: Notifier) -> Self {
        self.notifiers.push(notifier);
        self
    }

    /// Set the state file path
    pub fn with_state_file(mut self, path: PathBuf) -> Self {
        self.state_file = path;
        self
    }

    /// Configure the scanner
    pub fn with_scanner(mut self, scanner: NetworkScanner) -> Self {
        self.scanner = scanner;
        self
    }

    /// Load state from disk
    pub fn load_state(&mut self) -> Result<(), NexusError> {
        if !self.state_file.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.state_file).map_err(|e| NexusError::Io {
            message: format!("Failed to read discovery state: {}", e),
            path: Some(self.state_file.clone()),
        })?;

        self.state = serde_json::from_str(&content).map_err(|e| NexusError::Inventory {
            message: format!("Failed to parse discovery state: {}", e),
            suggestion: Some("State file may be corrupted".to_string()),
        })?;

        Ok(())
    }

    /// Save state to disk
    pub fn save_state(&self) -> Result<(), NexusError> {
        let json =
            serde_json::to_string_pretty(&self.state).map_err(|e| NexusError::Inventory {
                message: format!("Failed to serialize discovery state: {}", e),
                suggestion: None,
            })?;

        // Ensure parent directory exists
        if let Some(parent) = self.state_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NexusError::Io {
                message: format!("Failed to create state directory: {}", e),
                path: Some(parent.to_path_buf()),
            })?;
        }

        std::fs::write(&self.state_file, json).map_err(|e| NexusError::Io {
            message: format!("Failed to write discovery state: {}", e),
            path: Some(self.state_file.clone()),
        })?;

        Ok(())
    }

    /// Run the discovery daemon forever
    pub async fn run(&mut self) -> ! {
        // Load previous state if available
        if let Err(e) = self.load_state() {
            eprintln!("Warning: Failed to load state: {}", e);
        }

        loop {
            match self.scan_once().await {
                Ok(events) => {
                    for event in events {
                        self.notify(&event).await;
                    }

                    if let Err(e) = self.save_state() {
                        eprintln!("Warning: Failed to save state: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error during scan: {}", e);
                }
            }

            tokio::time::sleep(self.interval).await;
        }
    }

    /// Run a single scan cycle and return events
    pub async fn scan_once(&mut self) -> Result<Vec<ChangeEvent>, NexusError> {
        let mut all_discovered = Vec::new();

        // Scan all configured subnets
        for subnet in &self.watch_subnets {
            match self.scanner.scan_subnet(subnet).await {
                Ok(mut hosts) => {
                    all_discovered.append(&mut hosts);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to scan subnet {}: {}", subnet, e);
                }
            }
        }

        // Convert to host states
        let new_hosts: Vec<HostState> = all_discovered
            .into_iter()
            .map(|h| HostState {
                address: h.address,
                hostname: h.hostname,
                open_ports: h.open_ports.iter().map(|p| p.port).collect(),
                first_seen: h.first_seen,
                last_seen: h.last_seen,
            })
            .collect();

        // Compare with previous state to generate events
        let events = self.compare_state(&new_hosts);

        // Update state
        self.state.hosts.clear();
        for host in new_hosts {
            self.state.hosts.insert(host.address, host);
        }
        self.state.last_scan = Some(Utc::now());

        Ok(events)
    }

    /// Compare old and new state to generate change events
    fn compare_state(&self, new_hosts: &[HostState]) -> Vec<ChangeEvent> {
        let mut events = Vec::new();

        // Convert new hosts to a map for easier lookup
        let new_map: HashMap<IpAddr, &HostState> =
            new_hosts.iter().map(|h| (h.address, h)).collect();

        // Check for new hosts and port changes
        for new_host in new_hosts {
            match self.state.hosts.get(&new_host.address) {
                None => {
                    // New host discovered
                    events.push(ChangeEvent::HostDiscovered {
                        address: new_host.address,
                        hostname: new_host.hostname.clone(),
                        open_ports: new_host.open_ports.clone(),
                        first_seen: new_host.first_seen,
                    });
                }
                Some(old_host) => {
                    // Check for new ports
                    for &port in &new_host.open_ports {
                        if !old_host.open_ports.contains(&port) {
                            events.push(ChangeEvent::PortOpened {
                                host: new_host.address,
                                port,
                                service: identify_service(port),
                            });
                        }
                    }

                    // Check for closed ports
                    for &port in &old_host.open_ports {
                        if !new_host.open_ports.contains(&port) {
                            events.push(ChangeEvent::PortClosed {
                                host: new_host.address,
                                port,
                            });
                        }
                    }
                }
            }
        }

        // Check for disappeared hosts
        for (addr, old_host) in &self.state.hosts {
            if !new_map.contains_key(addr) {
                events.push(ChangeEvent::HostDisappeared {
                    address: *addr,
                    last_seen: old_host.last_seen,
                });
            }
        }

        events
    }

    /// Send notifications for a change event
    pub async fn notify(&self, event: &ChangeEvent) {
        for notifier in &self.notifiers {
            match notifier {
                Notifier::Stdout => {
                    self.notify_stdout(event);
                }
                Notifier::File { path } => {
                    if let Err(e) = self.notify_file(event, path).await {
                        eprintln!("Failed to write to notification file: {}", e);
                    }
                }
                Notifier::Webhook { url } => {
                    if let Err(e) = self.notify_webhook(event, url).await {
                        eprintln!("Failed to send webhook notification: {}", e);
                    }
                }
            }
        }
    }

    /// Print event to stdout
    fn notify_stdout(&self, event: &ChangeEvent) {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S");

        match event {
            ChangeEvent::HostDiscovered {
                address,
                hostname,
                open_ports,
                ..
            } => {
                let hostname_str = hostname
                    .as_ref()
                    .map(|h| format!(" ({})", h))
                    .unwrap_or_default();
                println!(
                    "[{}] Host discovered: {}{} - {} open ports: {:?}",
                    timestamp,
                    address,
                    hostname_str,
                    open_ports.len(),
                    open_ports
                );
            }
            ChangeEvent::HostDisappeared { address, .. } => {
                println!("[{}] Host disappeared: {}", timestamp, address);
            }
            ChangeEvent::PortOpened {
                host,
                port,
                service,
            } => {
                let service_str = service
                    .as_ref()
                    .map(|s| format!(" ({})", s))
                    .unwrap_or_default();
                println!(
                    "[{}] Port opened on {}: {}{}",
                    timestamp, host, port, service_str
                );
            }
            ChangeEvent::PortClosed { host, port } => {
                println!("[{}] Port closed on {}: {}", timestamp, host, port);
            }
        }
    }

    /// Append event to a log file
    async fn notify_file(&self, event: &ChangeEvent, path: &PathBuf) -> Result<(), NexusError> {
        use tokio::io::AsyncWriteExt;

        let json = serde_json::to_string(event).map_err(|e| NexusError::Inventory {
            message: format!("Failed to serialize event: {}", e),
            suggestion: None,
        })?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .map_err(|e| NexusError::Io {
                message: format!("Failed to open notification file: {}", e),
                path: Some(path.clone()),
            })?;

        file.write_all(json.as_bytes())
            .await
            .map_err(|e| NexusError::Io {
                message: format!("Failed to write to notification file: {}", e),
                path: Some(path.clone()),
            })?;

        file.write_all(b"\n").await.map_err(|e| NexusError::Io {
            message: format!("Failed to write to notification file: {}", e),
            path: Some(path.clone()),
        })?;

        Ok(())
    }

    /// Send event to a webhook
    async fn notify_webhook(&self, event: &ChangeEvent, url: &str) -> Result<(), NexusError> {
        let client = reqwest::Client::new();

        let response =
            client
                .post(url)
                .json(event)
                .send()
                .await
                .map_err(|e| NexusError::Runtime {
                    function: Some("webhook".to_string()),
                    message: format!("Failed to send webhook: {}", e),
                    suggestion: Some("Check webhook URL and network connectivity".to_string()),
                })?;

        if !response.status().is_success() {
            return Err(NexusError::Runtime {
                function: Some("webhook".to_string()),
                message: format!("Webhook returned status {}", response.status()),
                suggestion: None,
            });
        }

        Ok(())
    }
}

/// Identify service by port number
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
    fn test_daemon_creation() {
        let daemon =
            DiscoveryDaemon::new(vec!["192.168.1.0/24".to_string()], Duration::from_secs(60));

        assert_eq!(daemon.watch_subnets.len(), 1);
        assert_eq!(daemon.interval, Duration::from_secs(60));
    }

    #[test]
    fn test_compare_state_new_host() {
        let daemon = DiscoveryDaemon::new(vec![], Duration::from_secs(60));

        let new_hosts = vec![HostState {
            address: "192.168.1.10".parse().unwrap(),
            hostname: Some("host1".to_string()),
            open_ports: vec![22, 80],
            first_seen: Utc::now(),
            last_seen: Utc::now(),
        }];

        let events = daemon.compare_state(&new_hosts);

        assert_eq!(events.len(), 1);
        match &events[0] {
            ChangeEvent::HostDiscovered { address, .. } => {
                assert_eq!(address.to_string(), "192.168.1.10");
            }
            _ => panic!("Expected HostDiscovered event"),
        }
    }

    #[test]
    fn test_compare_state_disappeared_host() {
        let mut daemon = DiscoveryDaemon::new(vec![], Duration::from_secs(60));

        // Set up initial state with one host
        daemon.state.hosts.insert(
            "192.168.1.10".parse().unwrap(),
            HostState {
                address: "192.168.1.10".parse().unwrap(),
                hostname: Some("host1".to_string()),
                open_ports: vec![22],
                first_seen: Utc::now(),
                last_seen: Utc::now(),
            },
        );

        // Compare with empty new state
        let events = daemon.compare_state(&[]);

        assert_eq!(events.len(), 1);
        match &events[0] {
            ChangeEvent::HostDisappeared { address, .. } => {
                assert_eq!(address.to_string(), "192.168.1.10");
            }
            _ => panic!("Expected HostDisappeared event"),
        }
    }

    #[test]
    fn test_compare_state_port_changes() {
        let mut daemon = DiscoveryDaemon::new(vec![], Duration::from_secs(60));

        // Set up initial state
        daemon.state.hosts.insert(
            "192.168.1.10".parse().unwrap(),
            HostState {
                address: "192.168.1.10".parse().unwrap(),
                hostname: Some("host1".to_string()),
                open_ports: vec![22, 80],
                first_seen: Utc::now(),
                last_seen: Utc::now(),
            },
        );

        // New state with different ports
        let new_hosts = vec![HostState {
            address: "192.168.1.10".parse().unwrap(),
            hostname: Some("host1".to_string()),
            open_ports: vec![22, 443], // 80 closed, 443 opened
            first_seen: Utc::now(),
            last_seen: Utc::now(),
        }];

        let events = daemon.compare_state(&new_hosts);

        // Should have port opened and port closed events
        assert_eq!(events.len(), 2);

        let has_port_opened = events
            .iter()
            .any(|e| matches!(e, ChangeEvent::PortOpened { port: 443, .. }));
        let has_port_closed = events
            .iter()
            .any(|e| matches!(e, ChangeEvent::PortClosed { port: 80, .. }));

        assert!(has_port_opened, "Expected PortOpened event for port 443");
        assert!(has_port_closed, "Expected PortClosed event for port 80");
    }

    #[test]
    fn test_event_serialization() {
        let event = ChangeEvent::HostDiscovered {
            address: "192.168.1.10".parse().unwrap(),
            hostname: Some("test".to_string()),
            open_ports: vec![22, 80],
            first_seen: Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("host_discovered"));
        assert!(json.contains("192.168.1.10"));
    }
}
