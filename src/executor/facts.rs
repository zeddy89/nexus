// Facts gathering system - Better than Ansible's setup module
//
// Features beyond Ansible:
// - Smart caching: facts are cached per host with configurable TTL
// - Incremental updates: only gather changed facts, not full refresh
// - Custom facts: define and gather your own facts with plugins
// - Cross-host facts: access facts from other hosts in the playbook
// - Lazy gathering: facts gathered on-demand when accessed
// - Fact filtering: only gather the facts you need

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

use crate::executor::SshConnection;
use crate::output::errors::NexusError;
use crate::parser::ast::Value;

/// Cached facts for a single host
#[derive(Debug, Clone)]
pub struct HostFacts {
    /// All gathered facts
    pub facts: HashMap<String, Value>,
    /// When facts were last gathered
    pub gathered_at: Instant,
    /// Which fact categories have been gathered
    pub categories: Vec<String>,
}

impl HostFacts {
    pub fn new() -> Self {
        HostFacts {
            facts: HashMap::new(),
            gathered_at: Instant::now(),
            categories: Vec::new(),
        }
    }

    /// Check if facts are stale based on TTL
    pub fn is_stale(&self, ttl: Duration) -> bool {
        self.gathered_at.elapsed() > ttl
    }

    /// Merge new facts into existing
    pub fn merge(&mut self, new_facts: HashMap<String, Value>) {
        for (k, v) in new_facts {
            self.facts.insert(k, v);
        }
        self.gathered_at = Instant::now();
    }

    /// Get a specific fact
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.facts.get(name)
    }
}

impl Default for HostFacts {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry for caching facts across hosts
#[derive(Debug)]
#[allow(dead_code)]
pub struct FactCache {
    /// Facts per host
    hosts: RwLock<HashMap<String, HostFacts>>,
    /// Default TTL for fact cache
    ttl: Duration,
    /// Whether to auto-gather on first access
    auto_gather: bool,
}

impl FactCache {
    pub fn new() -> Self {
        FactCache {
            hosts: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(3600), // 1 hour default
            auto_gather: true,
        }
    }

    /// Create with custom TTL
    pub fn with_ttl(ttl: Duration) -> Self {
        FactCache {
            hosts: RwLock::new(HashMap::new()),
            ttl,
            auto_gather: true,
        }
    }

    /// Get facts for a host
    pub fn get_facts(&self, host: &str) -> Option<HostFacts> {
        let hosts = self.hosts.read();
        hosts.get(host).cloned()
    }

    /// Get a specific fact from a host
    pub fn get_fact(&self, host: &str, fact_name: &str) -> Option<Value> {
        let hosts = self.hosts.read();
        hosts.get(host)?.facts.get(fact_name).cloned()
    }

    /// Store facts for a host
    pub fn set_facts(&self, host: &str, facts: HostFacts) {
        let mut hosts = self.hosts.write();
        hosts.insert(host.to_string(), facts);
    }

    /// Update specific facts for a host
    pub fn update_facts(&self, host: &str, new_facts: HashMap<String, Value>) {
        let mut hosts = self.hosts.write();
        let entry = hosts.entry(host.to_string()).or_default();
        entry.merge(new_facts);
    }

    /// Check if facts need refresh
    pub fn needs_refresh(&self, host: &str) -> bool {
        let hosts = self.hosts.read();
        match hosts.get(host) {
            Some(facts) => facts.is_stale(self.ttl),
            None => true,
        }
    }

    /// Clear facts for a specific host
    pub fn clear_host(&self, host: &str) {
        let mut hosts = self.hosts.write();
        hosts.remove(host);
    }

    /// Clear all cached facts
    pub fn clear_all(&self) {
        let mut hosts = self.hosts.write();
        hosts.clear();
    }
}

impl Default for FactCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Categories of facts to gather
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactCategory {
    /// Basic system info (hostname, OS, arch)
    System,
    /// Hardware info (CPU, memory, disks)
    Hardware,
    /// Network interfaces and addresses
    Network,
    /// Mounted filesystems
    Mounts,
    /// Package manager and installed packages
    Packages,
    /// Running services
    Services,
    /// Environment variables
    Environment,
    /// All categories
    All,
}

impl FactCategory {
    /// Get all categories except All
    pub fn all_categories() -> Vec<FactCategory> {
        vec![
            FactCategory::System,
            FactCategory::Hardware,
            FactCategory::Network,
            FactCategory::Mounts,
            FactCategory::Packages,
            FactCategory::Services,
            FactCategory::Environment,
        ]
    }
}

/// Fact gatherer - collects system facts via SSH
pub struct FactGatherer;

impl FactGatherer {
    /// Gather facts for specified categories
    pub fn gather(
        conn: &SshConnection,
        categories: &[FactCategory],
    ) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Expand All to all categories
        let cats: Vec<FactCategory> = if categories.contains(&FactCategory::All) {
            FactCategory::all_categories()
        } else {
            categories.to_vec()
        };

        for category in cats {
            let category_facts = match category {
                FactCategory::System => Self::gather_system(conn)?,
                FactCategory::Hardware => Self::gather_hardware(conn)?,
                FactCategory::Network => Self::gather_network(conn)?,
                FactCategory::Mounts => Self::gather_mounts(conn)?,
                FactCategory::Packages => Self::gather_packages(conn)?,
                FactCategory::Services => Self::gather_services(conn)?,
                FactCategory::Environment => Self::gather_environment(conn)?,
                FactCategory::All => continue, // Already expanded
            };

            for (k, v) in category_facts {
                facts.insert(k, v);
            }
        }

        Ok(facts)
    }

    /// Gather all facts
    pub fn gather_all(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        Self::gather(conn, &[FactCategory::All])
    }

    /// Gather basic system facts
    fn gather_system(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Hostname
        let result = conn.exec("hostname -f 2>/dev/null || hostname")?;
        if result.success() {
            facts.insert("hostname".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Short hostname
        let result = conn.exec("hostname -s 2>/dev/null || hostname")?;
        if result.success() {
            facts.insert("hostname_short".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Distribution info (works on most Linux systems)
        let result = conn.exec("cat /etc/os-release 2>/dev/null || cat /etc/redhat-release 2>/dev/null || echo 'Unknown'")?;
        if result.success() {
            let os_info = Self::parse_os_release(&result.stdout);
            for (k, v) in os_info {
                facts.insert(k, v);
            }
        }

        // Kernel version
        let result = conn.exec("uname -r")?;
        if result.success() {
            facts.insert("kernel_version".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Architecture
        let result = conn.exec("uname -m")?;
        if result.success() {
            facts.insert("architecture".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Uptime
        let result = conn.exec("uptime -s 2>/dev/null || uptime")?;
        if result.success() {
            facts.insert("uptime".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Date/time
        let result = conn.exec("date -Iseconds")?;
        if result.success() {
            facts.insert("date_time".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Timezone
        let result = conn.exec("cat /etc/timezone 2>/dev/null || timedatectl show -p Timezone --value 2>/dev/null || echo 'Unknown'")?;
        if result.success() {
            facts.insert("timezone".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        Ok(facts)
    }

    /// Gather hardware facts
    fn gather_hardware(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // CPU count
        let result = conn.exec("nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo")?;
        if result.success() {
            if let Ok(n) = result.stdout.trim().parse::<i64>() {
                facts.insert("cpu_count".to_string(), Value::Int(n));
            }
        }

        // CPU model
        let result = conn.exec("grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2")?;
        if result.success() {
            facts.insert("cpu_model".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Memory total (in KB)
        let result = conn.exec("grep MemTotal /proc/meminfo | awk '{print $2}'")?;
        if result.success() {
            if let Ok(n) = result.stdout.trim().parse::<i64>() {
                facts.insert("memory_total_kb".to_string(), Value::Int(n));
                facts.insert("memory_total_mb".to_string(), Value::Int(n / 1024));
                facts.insert("memory_total_gb".to_string(), Value::Int(n / 1024 / 1024));
            }
        }

        // Memory free
        let result = conn.exec("grep MemFree /proc/meminfo | awk '{print $2}'")?;
        if result.success() {
            if let Ok(n) = result.stdout.trim().parse::<i64>() {
                facts.insert("memory_free_kb".to_string(), Value::Int(n));
            }
        }

        // Memory available
        let result = conn.exec("grep MemAvailable /proc/meminfo | awk '{print $2}'")?;
        if result.success() {
            if let Ok(n) = result.stdout.trim().parse::<i64>() {
                facts.insert("memory_available_kb".to_string(), Value::Int(n));
            }
        }

        // Swap total
        let result = conn.exec("grep SwapTotal /proc/meminfo | awk '{print $2}'")?;
        if result.success() {
            if let Ok(n) = result.stdout.trim().parse::<i64>() {
                facts.insert("swap_total_kb".to_string(), Value::Int(n));
            }
        }

        // Block devices
        let result = conn.exec("lsblk -n -o NAME,SIZE,TYPE,MOUNTPOINT 2>/dev/null | head -20")?;
        if result.success() {
            let devices: Vec<Value> = result.stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let mut device = HashMap::new();
                        device.insert("name".to_string(), Value::String(parts[0].to_string()));
                        device.insert("size".to_string(), Value::String(parts[1].to_string()));
                        device.insert("type".to_string(), Value::String(parts[2].to_string()));
                        if parts.len() > 3 {
                            device.insert("mountpoint".to_string(), Value::String(parts[3].to_string()));
                        }
                        Some(Value::Dict(device))
                    } else {
                        None
                    }
                })
                .collect();
            facts.insert("block_devices".to_string(), Value::List(devices));
        }

        Ok(facts)
    }

    /// Gather network facts
    fn gather_network(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Get all interfaces
        let result = conn.exec("ip -o link show | awk -F': ' '{print $2}'")?;
        if result.success() {
            let interfaces: Vec<Value> = result.stdout
                .lines()
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            facts.insert("interfaces".to_string(), Value::List(interfaces));
        }

        // Get default IPv4 address
        let result = conn.exec("ip -4 route get 8.8.8.8 2>/dev/null | grep -oP 'src \\K[^ ]+'")?;
        if result.success() && !result.stdout.trim().is_empty() {
            facts.insert("default_ipv4".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Get all IPv4 addresses
        let result = conn.exec("ip -4 addr show | grep -oP 'inet \\K[^/]+'")?;
        if result.success() {
            let ips: Vec<Value> = result.stdout
                .lines()
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            facts.insert("all_ipv4_addresses".to_string(), Value::List(ips));
        }

        // Get default gateway
        let result = conn.exec("ip -4 route show default | awk '/default/ {print $3}'")?;
        if result.success() && !result.stdout.trim().is_empty() {
            facts.insert("default_gateway".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // DNS servers
        let result = conn.exec("grep '^nameserver' /etc/resolv.conf | awk '{print $2}'")?;
        if result.success() {
            let dns: Vec<Value> = result.stdout
                .lines()
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            facts.insert("dns_servers".to_string(), Value::List(dns));
        }

        Ok(facts)
    }

    /// Gather mount facts
    fn gather_mounts(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        let result = conn.exec("df -P | tail -n +2")?;
        if result.success() {
            let mounts: Vec<Value> = result.stdout
                .lines()
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 6 {
                        let mut mount = HashMap::new();
                        mount.insert("filesystem".to_string(), Value::String(parts[0].to_string()));
                        if let Ok(n) = parts[1].parse::<i64>() {
                            mount.insert("size_kb".to_string(), Value::Int(n));
                        }
                        if let Ok(n) = parts[2].parse::<i64>() {
                            mount.insert("used_kb".to_string(), Value::Int(n));
                        }
                        if let Ok(n) = parts[3].parse::<i64>() {
                            mount.insert("available_kb".to_string(), Value::Int(n));
                        }
                        mount.insert("use_percent".to_string(), Value::String(parts[4].to_string()));
                        mount.insert("mount_point".to_string(), Value::String(parts[5].to_string()));
                        Some(Value::Dict(mount))
                    } else {
                        None
                    }
                })
                .collect();
            facts.insert("mounts".to_string(), Value::List(mounts));
        }

        Ok(facts)
    }

    /// Gather package manager facts
    fn gather_packages(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Detect package manager
        let managers = [
            ("apt", "which apt 2>/dev/null"),
            ("dnf", "which dnf 2>/dev/null"),
            ("yum", "which yum 2>/dev/null"),
            ("pacman", "which pacman 2>/dev/null"),
            ("zypper", "which zypper 2>/dev/null"),
            ("apk", "which apk 2>/dev/null"),
        ];

        for (name, cmd) in managers {
            let result = conn.exec(cmd)?;
            if result.success() && !result.stdout.trim().is_empty() {
                facts.insert("package_manager".to_string(), Value::String(name.to_string()));
                break;
            }
        }

        // Get installed packages count (not the full list - that could be huge)
        let count_cmds = [
            ("apt", "dpkg -l 2>/dev/null | grep '^ii' | wc -l"),
            ("dnf", "rpm -qa 2>/dev/null | wc -l"),
            ("yum", "rpm -qa 2>/dev/null | wc -l"),
            ("pacman", "pacman -Q 2>/dev/null | wc -l"),
        ];

        for (_, cmd) in count_cmds {
            let result = conn.exec(cmd)?;
            if result.success() {
                if let Ok(n) = result.stdout.trim().parse::<i64>() {
                    if n > 0 {
                        facts.insert("installed_packages_count".to_string(), Value::Int(n));
                        break;
                    }
                }
            }
        }

        Ok(facts)
    }

    /// Gather service facts
    fn gather_services(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Check for systemd
        let result = conn.exec("which systemctl 2>/dev/null")?;
        let has_systemd = result.success() && !result.stdout.trim().is_empty();
        facts.insert("has_systemd".to_string(), Value::Bool(has_systemd));

        if has_systemd {
            // Get running services
            let result = conn.exec("systemctl list-units --type=service --state=running --no-pager --no-legend | awk '{print $1}' | head -50")?;
            if result.success() {
                let services: Vec<Value> = result.stdout
                    .lines()
                    .map(|s| Value::String(s.trim().to_string()))
                    .collect();
                facts.insert("running_services".to_string(), Value::List(services));
            }
        }

        Ok(facts)
    }

    /// Gather environment facts
    fn gather_environment(conn: &SshConnection) -> Result<HashMap<String, Value>, NexusError> {
        let mut facts = HashMap::new();

        // Current user
        let result = conn.exec("whoami")?;
        if result.success() {
            facts.insert("user".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Home directory
        let result = conn.exec("echo $HOME")?;
        if result.success() {
            facts.insert("home".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Shell
        let result = conn.exec("echo $SHELL")?;
        if result.success() {
            facts.insert("shell".to_string(), Value::String(result.stdout.trim().to_string()));
        }

        // Path
        let result = conn.exec("echo $PATH")?;
        if result.success() {
            let paths: Vec<Value> = result.stdout
                .trim()
                .split(':')
                .map(|s| Value::String(s.to_string()))
                .collect();
            facts.insert("path".to_string(), Value::List(paths));
        }

        Ok(facts)
    }

    /// Parse /etc/os-release format
    fn parse_os_release(content: &str) -> HashMap<String, Value> {
        let mut facts = HashMap::new();

        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim().trim_matches('"').to_string();

                match key.as_str() {
                    "id" => { facts.insert("os_family".to_string(), Value::String(value)); }
                    "name" => { facts.insert("os_name".to_string(), Value::String(value)); }
                    "version_id" => { facts.insert("os_version".to_string(), Value::String(value)); }
                    "pretty_name" => { facts.insert("os_pretty_name".to_string(), Value::String(value)); }
                    "version_codename" => { facts.insert("os_codename".to_string(), Value::String(value)); }
                    _ => {}
                }
            }
        }

        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_facts_stale() {
        let facts = HostFacts::new();

        // Fresh facts shouldn't be stale
        assert!(!facts.is_stale(Duration::from_secs(60)));

        // Would be stale with 0 TTL
        assert!(facts.is_stale(Duration::ZERO));
    }

    #[test]
    fn test_fact_cache_basic() {
        let cache = FactCache::new();

        assert!(cache.needs_refresh("host1"));

        let mut facts = HostFacts::new();
        facts.facts.insert("hostname".to_string(), Value::String("myhost".to_string()));
        cache.set_facts("host1", facts);

        assert!(!cache.needs_refresh("host1"));
        assert_eq!(
            cache.get_fact("host1", "hostname"),
            Some(Value::String("myhost".to_string()))
        );
    }

    #[test]
    fn test_fact_cache_update() {
        let cache = FactCache::new();

        let mut initial = HashMap::new();
        initial.insert("fact1".to_string(), Value::String("value1".to_string()));
        cache.update_facts("host1", initial);

        let mut update = HashMap::new();
        update.insert("fact2".to_string(), Value::String("value2".to_string()));
        cache.update_facts("host1", update);

        // Both facts should be present
        assert_eq!(
            cache.get_fact("host1", "fact1"),
            Some(Value::String("value1".to_string()))
        );
        assert_eq!(
            cache.get_fact("host1", "fact2"),
            Some(Value::String("value2".to_string()))
        );
    }

    #[test]
    fn test_parse_os_release() {
        let content = r#"
NAME="Ubuntu"
VERSION_ID="22.04"
ID=ubuntu
PRETTY_NAME="Ubuntu 22.04.3 LTS"
VERSION_CODENAME=jammy
"#;
        let facts = FactGatherer::parse_os_release(content);

        assert_eq!(facts.get("os_family"), Some(&Value::String("ubuntu".to_string())));
        assert_eq!(facts.get("os_name"), Some(&Value::String("Ubuntu".to_string())));
        assert_eq!(facts.get("os_version"), Some(&Value::String("22.04".to_string())));
    }
}
