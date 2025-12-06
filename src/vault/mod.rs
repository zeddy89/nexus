// Vault encryption module for secure variable storage
//
// Provides AES-256-GCM encryption with Argon2 key derivation for securing
// sensitive variables in playbooks and variable files.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use std::path::Path;
use thiserror::Error;
use zeroize::Zeroizing;

pub mod format;

pub use format::{VaultFile, VaultFormat};

/// Vault error types
#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Failed to encrypt data: {0}")]
    EncryptionError(String),

    #[error("Failed to decrypt data: {0}")]
    DecryptionError(String),

    #[error("Invalid vault format: {0}")]
    InvalidFormat(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid password")]
    InvalidPassword,

    #[error("Key derivation failed: {0}")]
    KeyDerivationError(String),
}

/// Encryption context holds the key and cipher
pub struct VaultCipher {
    cipher: Aes256Gcm,
}

impl VaultCipher {
    /// Create a new vault cipher from a password
    pub fn new(password: &str) -> Result<Self, VaultError> {
        let key = derive_key(password, None)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| VaultError::EncryptionError(e.to_string()))?;

        Ok(VaultCipher { cipher })
    }

    /// Create cipher with a specific salt (for decryption)
    pub fn with_salt(password: &str, salt: &[u8]) -> Result<Self, VaultError> {
        let key = derive_key(password, Some(salt))?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| VaultError::EncryptionError(e.to_string()))?;

        Ok(VaultCipher { cipher })
    }

    /// Encrypt data with a random nonce
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), VaultError> {
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| VaultError::EncryptionError(e.to_string()))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    /// Decrypt data with provided nonce
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, VaultError> {
        if nonce.len() != 12 {
            return Err(VaultError::DecryptionError(
                "Invalid nonce length".to_string(),
            ));
        }

        let nonce = Nonce::from_slice(nonce);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| VaultError::DecryptionError(e.to_string()))
    }
}

/// Derive a 256-bit encryption key from a password using Argon2
fn derive_key(password: &str, salt: Option<&[u8]>) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    // Use secure Argon2 parameters:
    // - Memory cost: 64 MB (65536 KiB)
    // - Iterations: 3
    // - Parallelism: 4 threads
    // - Output length: 32 bytes for AES-256
    let params = Params::new(
        65536,    // m_cost: 64 MB
        3,        // t_cost: 3 iterations
        4,        // p_cost: 4 parallel threads
        Some(32), // output length: 32 bytes for AES-256-GCM
    )
    .map_err(|e| VaultError::KeyDerivationError(format!("Invalid Argon2 params: {}", e)))?;

    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id, // Argon2id is recommended (resistant to side-channel and GPU attacks)
        Version::V0x13,              // Latest version
        params,
    );

    // Generate or use provided salt (16 bytes, cryptographically random)
    let salt_bytes = if let Some(salt_input) = salt {
        if salt_input.len() < 16 {
            return Err(VaultError::KeyDerivationError(
                "Salt must be at least 16 bytes".to_string(),
            ));
        }
        salt_input.to_vec()
    } else {
        let mut salt = vec![0u8; 16];
        OsRng.fill_bytes(&mut salt);
        salt
    };

    // Derive key material directly (32 bytes for AES-256)
    let mut key = Zeroizing::new(vec![0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), &salt_bytes, &mut key)
        .map_err(|e| VaultError::KeyDerivationError(format!("Key derivation failed: {}", e)))?;

    Ok(key)
}

/// Encrypt a string value
pub fn encrypt_string(password: &str, plaintext: &str) -> Result<String, VaultError> {
    // Generate a random salt
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);

    let cipher = VaultCipher::with_salt(password, &salt)?;
    let (ciphertext, nonce) = cipher.encrypt(plaintext.as_bytes())?;

    // Encode: salt || nonce || ciphertext
    let mut combined = Vec::new();
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a string value
pub fn decrypt_string(password: &str, encrypted: &str) -> Result<String, VaultError> {
    let combined = BASE64
        .decode(encrypted)
        .map_err(|e| VaultError::InvalidFormat(format!("Invalid base64: {}", e)))?;

    if combined.len() < 28 {
        // 16 bytes salt + 12 bytes nonce
        return Err(VaultError::InvalidFormat(
            "Encrypted data too short".to_string(),
        ));
    }

    let salt = &combined[0..16];
    let nonce = &combined[16..28];
    let ciphertext = &combined[28..];

    let cipher = VaultCipher::with_salt(password, salt)?;
    let plaintext = cipher.decrypt(ciphertext, nonce)?;

    String::from_utf8(plaintext)
        .map_err(|e| VaultError::DecryptionError(format!("Invalid UTF-8: {}", e)))
}

/// Encrypt a file
pub fn encrypt_file(path: &Path, password: &str) -> Result<(), VaultError> {
    let content = std::fs::read_to_string(path)?;
    let vault_file = VaultFile::encrypt(&content, password)?;
    vault_file.write_to_file(path)?;
    Ok(())
}

/// Decrypt a file
pub fn decrypt_file(path: &Path, password: &str) -> Result<(), VaultError> {
    let vault_file = VaultFile::read_from_file(path)?;
    let content = vault_file.decrypt(password)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// View decrypted content without modifying the file
pub fn view_file(path: &Path, password: &str) -> Result<String, VaultError> {
    let vault_file = VaultFile::read_from_file(path)?;
    vault_file.decrypt(password)
}

/// Check if a file is vault-encrypted
pub fn is_vault_file(path: &Path) -> bool {
    if let Ok(content) = std::fs::read_to_string(path) {
        VaultFile::is_vault_format(&content)
    } else {
        false
    }
}

/// Check if a string is vault-encrypted
pub fn is_vault_string(s: &str) -> bool {
    VaultFile::is_vault_format(s)
}

/// Prompt for password securely
pub fn prompt_password(prompt: &str) -> Result<String, VaultError> {
    use std::io::Write;

    eprint!("{}", prompt);
    std::io::stderr().flush()?;

    let password = rpassword::read_password()
        .map_err(|e| VaultError::IoError(std::io::Error::other(e.to_string())))?;

    eprintln!();

    if password.is_empty() {
        return Err(VaultError::InvalidPassword);
    }

    Ok(password)
}

/// Secure password holder that clears on drop
pub struct SecurePassword(Zeroizing<String>);

impl SecurePassword {
    pub fn new(password: String) -> Self {
        SecurePassword(Zeroizing::new(password))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SecurePassword {
    fn from(s: String) -> Self {
        SecurePassword::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_string() {
        let password = "test_password";
        let plaintext = "secret data";

        let encrypted = encrypt_string(password, plaintext).unwrap();
        let decrypted = decrypt_string(password, &encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_wrong_password() {
        let password = "test_password";
        let wrong_password = "wrong_password";
        let plaintext = "secret data";

        let encrypted = encrypt_string(password, plaintext).unwrap();
        let result = decrypt_string(wrong_password, &encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn test_vault_cipher() {
        let password = "test123";
        let cipher = VaultCipher::new(password).unwrap();

        let plaintext = b"Hello, World!";
        let (ciphertext, nonce) = cipher.encrypt(plaintext).unwrap();
        let decrypted = cipher.decrypt(&ciphertext, &nonce).unwrap();

        assert_eq!(plaintext, decrypted.as_slice());
    }

    #[test]
    fn test_different_passwords_different_output() {
        let plaintext = "same data";

        let encrypted1 = encrypt_string("password1", plaintext).unwrap();
        let encrypted2 = encrypt_string("password2", plaintext).unwrap();

        assert_ne!(encrypted1, encrypted2);
    }

    #[test]
    fn test_same_password_different_output() {
        // Due to random salt and nonce, same password should produce different ciphertext
        let password = "test";
        let plaintext = "data";

        let encrypted1 = encrypt_string(password, plaintext).unwrap();
        let encrypted2 = encrypt_string(password, plaintext).unwrap();

        assert_ne!(encrypted1, encrypted2);

        // But both should decrypt correctly
        let decrypted1 = decrypt_string(password, &encrypted1).unwrap();
        let decrypted2 = decrypt_string(password, &encrypted2).unwrap();

        assert_eq!(plaintext, decrypted1);
        assert_eq!(plaintext, decrypted2);
    }
}
