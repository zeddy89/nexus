// Vault file format handling
//
// Format: $NEXUS_VAULT;1.0;AES256
//         <base64-encoded-encrypted-content>

use super::{VaultCipher, VaultError};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use std::path::Path;

/// Vault file format identifier
pub const VAULT_HEADER: &str = "$NEXUS_VAULT";
pub const VAULT_VERSION: &str = "1.0";
pub const VAULT_CIPHER: &str = "AES256";

/// Vault file format versions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultFormat {
    V1_0,
}

impl std::str::FromStr for VaultFormat {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1.0" => Ok(VaultFormat::V1_0),
            _ => Err(()),
        }
    }
}

impl VaultFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            VaultFormat::V1_0 => "1.0",
        }
    }
}

/// A vault-encrypted file
#[derive(Debug)]
pub struct VaultFile {
    pub format: VaultFormat,
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

impl VaultFile {
    /// Check if content is vault-formatted
    pub fn is_vault_format(content: &str) -> bool {
        content.trim().starts_with(VAULT_HEADER)
    }

    /// Create a new encrypted vault file
    pub fn encrypt(plaintext: &str, password: &str) -> Result<Self, VaultError> {
        use aes_gcm::aead::OsRng;

        // Generate random salt
        let mut salt = vec![0u8; 16];
        OsRng.fill_bytes(&mut salt);

        // Create cipher with salt
        let cipher = VaultCipher::with_salt(password, &salt)?;

        // Encrypt the content
        let (ciphertext, nonce) = cipher.encrypt(plaintext.as_bytes())?;

        Ok(VaultFile {
            format: VaultFormat::V1_0,
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Decrypt the vault file
    pub fn decrypt(&self, password: &str) -> Result<String, VaultError> {
        let cipher = VaultCipher::with_salt(password, &self.salt)?;
        let plaintext = cipher.decrypt(&self.ciphertext, &self.nonce)?;

        String::from_utf8(plaintext)
            .map_err(|e| VaultError::DecryptionError(format!("Invalid UTF-8: {}", e)))
    }

    /// Read a vault file from disk
    pub fn read_from_file(path: &Path) -> Result<Self, VaultError> {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Write vault file to disk
    pub fn write_to_file(&self, path: &Path) -> Result<(), VaultError> {
        let content = self.format_as_string();
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Parse vault format from string
    pub fn parse(content: &str) -> Result<Self, VaultError> {
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() {
            return Err(VaultError::InvalidFormat("Empty vault file".to_string()));
        }

        // Parse header: $NEXUS_VAULT;1.0;AES256
        let header = lines[0];
        let parts: Vec<&str> = header.split(';').collect();

        if parts.len() != 3 {
            return Err(VaultError::InvalidFormat(format!(
                "Invalid header format: {}",
                header
            )));
        }

        if parts[0] != VAULT_HEADER {
            return Err(VaultError::InvalidFormat(format!(
                "Invalid vault header: {}",
                parts[0]
            )));
        }

        let format = parts[1].parse::<VaultFormat>().map_err(|_| {
            VaultError::InvalidFormat(format!("Unsupported vault version: {}", parts[1]))
        })?;

        if parts[2] != VAULT_CIPHER {
            return Err(VaultError::InvalidFormat(format!(
                "Unsupported cipher: {}",
                parts[2]
            )));
        }

        // Get the base64-encoded data (all remaining lines joined)
        let encoded_data = lines[1..].join("");

        if encoded_data.is_empty() {
            return Err(VaultError::InvalidFormat(
                "No encrypted data found".to_string(),
            ));
        }

        // Decode base64
        let decoded = BASE64.decode(&encoded_data).map_err(|e| {
            VaultError::InvalidFormat(format!("Invalid base64 encoding: {}", e))
        })?;

        // Extract salt, nonce, and ciphertext
        // Format: [16 bytes salt][12 bytes nonce][remaining bytes ciphertext]
        if decoded.len() < 28 {
            return Err(VaultError::InvalidFormat(
                "Encrypted data too short".to_string(),
            ));
        }

        let salt = decoded[0..16].to_vec();
        let nonce = decoded[16..28].to_vec();
        let ciphertext = decoded[28..].to_vec();

        Ok(VaultFile {
            format,
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Format as a string for writing to file
    pub fn format_as_string(&self) -> String {
        // Combine salt + nonce + ciphertext
        let mut combined = Vec::new();
        combined.extend_from_slice(&self.salt);
        combined.extend_from_slice(&self.nonce);
        combined.extend_from_slice(&self.ciphertext);

        // Encode to base64
        let encoded = BASE64.encode(&combined);

        // Split into lines of 80 characters for readability
        let mut lines = vec![format!(
            "{};{};{}",
            VAULT_HEADER,
            self.format.as_str(),
            VAULT_CIPHER
        )];

        for chunk in encoded.as_bytes().chunks(80) {
            lines.push(String::from_utf8_lossy(chunk).to_string());
        }

        lines.join("\n")
    }
}

/// Parse vault-encrypted inline value (for YAML !vault tag)
pub fn parse_vault_value(content: &str, password: &str) -> Result<String, VaultError> {
    let vault_file = VaultFile::parse(content)?;
    vault_file.decrypt(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_vault_format() {
        assert!(VaultFile::is_vault_format("$NEXUS_VAULT;1.0;AES256\nabcd"));
        assert!(!VaultFile::is_vault_format("regular text"));
        assert!(!VaultFile::is_vault_format(""));
    }

    #[test]
    fn test_encrypt_decrypt_vault_file() {
        let password = "test_password";
        let plaintext = "secret: my_secret_value\napi_key: abc123";

        let vault = VaultFile::encrypt(plaintext, password).unwrap();
        let decrypted = vault.decrypt(password).unwrap();

        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_vault_file_format_string() {
        let password = "test_password";
        let plaintext = "secret data";

        let vault = VaultFile::encrypt(plaintext, password).unwrap();
        let formatted = vault.format_as_string();

        // Should start with header
        assert!(formatted.starts_with("$NEXUS_VAULT;1.0;AES256"));

        // Should have at least 2 lines
        let lines: Vec<&str> = formatted.lines().collect();
        assert!(lines.len() >= 2);
    }

    #[test]
    fn test_parse_vault_file() {
        let password = "test_password";
        let plaintext = "secret data";

        let vault = VaultFile::encrypt(plaintext, password).unwrap();
        let formatted = vault.format_as_string();

        // Parse it back
        let parsed = VaultFile::parse(&formatted).unwrap();
        let decrypted = parsed.decrypt(password).unwrap();

        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_wrong_password_fails() {
        let password = "correct";
        let wrong = "wrong";
        let plaintext = "secret";

        let vault = VaultFile::encrypt(plaintext, password).unwrap();
        let result = vault.decrypt(wrong);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_format() {
        let result = VaultFile::parse("not a vault file");
        assert!(result.is_err());

        let result = VaultFile::parse("$NEXUS_VAULT");
        assert!(result.is_err());

        let result = VaultFile::parse("$NEXUS_VAULT;1.0;AES256");
        assert!(result.is_err()); // No data
    }

    #[test]
    fn test_multiline_base64() {
        let password = "test";
        // Create content that will result in long base64
        let plaintext = "a".repeat(200);

        let vault = VaultFile::encrypt(&plaintext, password).unwrap();
        let formatted = vault.format_as_string();

        // Should have multiple lines
        let lines: Vec<&str> = formatted.lines().collect();
        assert!(lines.len() > 2);

        // Should still parse correctly
        let parsed = VaultFile::parse(&formatted).unwrap();
        let decrypted = parsed.decrypt(password).unwrap();

        assert_eq!(plaintext, decrypted);
    }
}
