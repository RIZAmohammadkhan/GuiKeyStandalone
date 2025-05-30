// src/infrastructure/encryption.rs

use crate::errors::ServerError; // Assuming ServerError is in crate::errors
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

const NONCE_SIZE: usize = 12; // Standard for AES-GCM (96-bit)

/// Decrypts a payload that was encrypted with AES-256-GCM.
/// The payload is expected to be: NONCE (12 bytes) || CIPHERTEXT_WITH_TAG.
/// The authentication tag is expected to be appended to the ciphertext.
pub fn decrypt_payload(
    encrypted_data_with_nonce: &[u8],
    key: &[u8; 32],
) -> Result<Vec<u8>, ServerError> {
    if encrypted_data_with_nonce.len() < NONCE_SIZE {
        tracing::warn!(
            "Encrypted data too short to contain nonce. Length: {}",
            encrypted_data_with_nonce.len()
        );
        return Err(ServerError::Crypto(
            "Encrypted data too short for nonce.".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
        tracing::error!("Failed to create AES cipher for decryption: {}", e);
        ServerError::Crypto(format!("Failed to create AES cipher: {}", e))
    })?;

    let (nonce_bytes, ciphertext_with_tag) = encrypted_data_with_nonce.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    // The `decrypt` method of `Aes256Gcm` expects the authentication tag
    // to be part of the `ciphertext_with_tag` slice.
    cipher.decrypt(nonce, ciphertext_with_tag)
        .map_err(|e| {
            // This error often means the key is wrong, the data is corrupt, or the MAC check failed.
            tracing::warn!("AES decryption/MAC verification failed: {}. Potential key mismatch or data corruption.", e);
            ServerError::Crypto(format!("AES decryption/MAC verification failed: {}", e))
        })
}
