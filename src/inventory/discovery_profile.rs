use serde::{Deserialize, Serialize};
use std::path::Path;
use crate::output::errors::NexusError;

/// Configuration profile for network discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryProfile {
    pub name: String,
    pub probes: Vec<ProbeConfig>,
    pub fingerprint: FingerprintConfig,
    pub classify: Vec<ClassifyRule>,
}

/// Configuration for a single probe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeConfig {
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_banner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// Configuration for fingerprinting methods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintConfig {
    pub methods: Vec<FingerprintMethod>,
}

/// Available fingerprinting methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintMethod {
    SshBanner,
    TcpTimestamps,
    TtlAnalysis,
}

/// Rule for OS classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyRule {
    pub name: String,
    pub condition: String,
    #[serde(default)]
    pub is_default: bool,
}

impl DiscoveryProfile {
    /// Load a discovery profile from a YAML file
    pub fn from_file(path: &Path) -> Result<Self, NexusError> {
        let content = std::fs::read_to_string(path).map_err(|e| NexusError::Io {
            message: format!("Failed to read discovery profile: {}", e),
            path: Some(path.to_path_buf()),
        })?;

        let profile: DiscoveryProfile = serde_yaml::from_str(&content).map_err(|e| {
            NexusError::Inventory {
                message: format!("Failed to parse discovery profile: {}", e),
                suggestion: Some("Check YAML syntax and structure".to_string()),
            }
        })?;

        Ok(profile)
    }

    /// Create a default SSH discovery profile
    pub fn default_ssh() -> Self {
        DiscoveryProfile {
            name: "ssh_discovery".to_string(),
            probes: vec![
                ProbeConfig {
                    port: 22,
                    expect_banner: Some("SSH".to_string()),
                    service: Some("ssh".to_string()),
                },
            ],
            fingerprint: FingerprintConfig {
                methods: vec![
                    FingerprintMethod::SshBanner,
                    FingerprintMethod::TcpTimestamps,
                ],
            },
            classify: vec![
                ClassifyRule {
                    name: "ubuntu".to_string(),
                    condition: "ssh_banner contains 'Ubuntu'".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "debian".to_string(),
                    condition: "ssh_banner contains 'Debian'".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "rhel".to_string(),
                    condition: "ssh_banner contains 'Red Hat' or ssh_banner contains 'CentOS'".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "linux".to_string(),
                    condition: "ssh_banner contains 'OpenSSH'".to_string(),
                    is_default: true,
                },
            ],
        }
    }

    /// Create a web server discovery profile
    pub fn default_web() -> Self {
        DiscoveryProfile {
            name: "web_discovery".to_string(),
            probes: vec![
                ProbeConfig {
                    port: 80,
                    expect_banner: None,
                    service: Some("http".to_string()),
                },
                ProbeConfig {
                    port: 443,
                    expect_banner: None,
                    service: Some("https".to_string()),
                },
                ProbeConfig {
                    port: 8080,
                    expect_banner: None,
                    service: Some("http-alt".to_string()),
                },
            ],
            fingerprint: FingerprintConfig {
                methods: vec![
                    FingerprintMethod::TcpTimestamps,
                    FingerprintMethod::TtlAnalysis,
                ],
            },
            classify: vec![
                ClassifyRule {
                    name: "web_server".to_string(),
                    condition: "port 80 or port 443 is open".to_string(),
                    is_default: true,
                },
            ],
        }
    }

    /// Create a comprehensive discovery profile
    pub fn default_comprehensive() -> Self {
        DiscoveryProfile {
            name: "comprehensive_discovery".to_string(),
            probes: vec![
                ProbeConfig {
                    port: 22,
                    expect_banner: Some("SSH".to_string()),
                    service: Some("ssh".to_string()),
                },
                ProbeConfig {
                    port: 80,
                    expect_banner: None,
                    service: Some("http".to_string()),
                },
                ProbeConfig {
                    port: 443,
                    expect_banner: None,
                    service: Some("https".to_string()),
                },
                ProbeConfig {
                    port: 3389,
                    expect_banner: None,
                    service: Some("rdp".to_string()),
                },
                ProbeConfig {
                    port: 3306,
                    expect_banner: None,
                    service: Some("mysql".to_string()),
                },
                ProbeConfig {
                    port: 5432,
                    expect_banner: None,
                    service: Some("postgresql".to_string()),
                },
            ],
            fingerprint: FingerprintConfig {
                methods: vec![
                    FingerprintMethod::SshBanner,
                    FingerprintMethod::TcpTimestamps,
                    FingerprintMethod::TtlAnalysis,
                ],
            },
            classify: vec![
                ClassifyRule {
                    name: "windows".to_string(),
                    condition: "port 3389 is open".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "ubuntu".to_string(),
                    condition: "ssh_banner contains 'Ubuntu'".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "debian".to_string(),
                    condition: "ssh_banner contains 'Debian'".to_string(),
                    is_default: false,
                },
                ClassifyRule {
                    name: "linux".to_string(),
                    condition: "port 22 is open".to_string(),
                    is_default: true,
                },
            ],
        }
    }

    /// Save the profile to a YAML file
    pub fn to_file(&self, path: &Path) -> Result<(), NexusError> {
        let yaml = serde_yaml::to_string(self).map_err(|e| NexusError::Inventory {
            message: format!("Failed to serialize discovery profile: {}", e),
            suggestion: None,
        })?;

        std::fs::write(path, yaml).map_err(|e| NexusError::Io {
            message: format!("Failed to write discovery profile: {}", e),
            path: Some(path.to_path_buf()),
        })?;

        Ok(())
    }

    /// Validate the profile configuration
    pub fn validate(&self) -> Result<(), NexusError> {
        if self.name.is_empty() {
            return Err(NexusError::Inventory {
                message: "Discovery profile name cannot be empty".to_string(),
                suggestion: None,
            });
        }

        if self.probes.is_empty() {
            return Err(NexusError::Inventory {
                message: "Discovery profile must have at least one probe".to_string(),
                suggestion: Some("Add a probe configuration to the 'probes' list".to_string()),
            });
        }

        for probe in &self.probes {
            if probe.port == 0 {
                return Err(NexusError::Inventory {
                    message: "Probe port cannot be 0".to_string(),
                    suggestion: Some("Use a valid port number (1-65535)".to_string()),
                });
            }
        }

        if self.fingerprint.methods.is_empty() {
            return Err(NexusError::Inventory {
                message: "Discovery profile must have at least one fingerprint method".to_string(),
                suggestion: Some("Add a fingerprint method like 'ssh_banner' or 'tcp_timestamps'".to_string()),
            });
        }

        Ok(())
    }
}

impl Default for DiscoveryProfile {
    fn default() -> Self {
        Self::default_ssh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ssh_profile() {
        let profile = DiscoveryProfile::default_ssh();
        assert_eq!(profile.name, "ssh_discovery");
        assert_eq!(profile.probes.len(), 1);
        assert_eq!(profile.probes[0].port, 22);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn test_default_web_profile() {
        let profile = DiscoveryProfile::default_web();
        assert_eq!(profile.name, "web_discovery");
        assert_eq!(profile.probes.len(), 3);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn test_comprehensive_profile() {
        let profile = DiscoveryProfile::default_comprehensive();
        assert_eq!(profile.name, "comprehensive_discovery");
        assert!(profile.probes.len() >= 5);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn test_validation() {
        let mut profile = DiscoveryProfile::default_ssh();

        // Valid profile
        assert!(profile.validate().is_ok());

        // Empty name
        profile.name = String::new();
        assert!(profile.validate().is_err());

        // Reset name
        profile.name = "test".to_string();

        // No probes
        profile.probes.clear();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn test_serialization() {
        let profile = DiscoveryProfile::default_ssh();
        let yaml = serde_yaml::to_string(&profile).unwrap();

        assert!(yaml.contains("name:"));
        assert!(yaml.contains("probes:"));
        assert!(yaml.contains("fingerprint:"));

        // Test deserialization
        let deserialized: DiscoveryProfile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(deserialized.name, profile.name);
        assert_eq!(deserialized.probes.len(), profile.probes.len());
    }
}
