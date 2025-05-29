// src/network/encryption.rs

use crate::errors::AppError;
use aes_gcm::aead::{Aead, KeyInit, OsRng, AeadCore}; // AeadCore for generate_nonce
use aes_gcm::{Aes256Gcm, Nonce}; // Or your specific AES variant

const NONCE_SIZE: usize = 12; // Standard for AES-GCM

pub fn encrypt_payload(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, AppError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Encryption(format!("Failed to create AES cipher: {}", e)))?;
    
    let nonce_val = Aes256Gcm::generate_nonce(&mut OsRng); // Returns GenericArray
    // The Nonce type from aes-gcm is usually a wrapper around GenericArray of the correct size.
    // If encrypt takes &GenericArray directly, this conversion might not be needed.
    // Let's assume encrypt takes a Nonce type or compatible slice.
    let nonce_for_encryption = Nonce::from_slice(nonce_val.as_slice());

    // encrypt() typically appends the authentication tag to the ciphertext
    let ciphertext_with_tag = cipher.encrypt(nonce_for_encryption, data)
        .map_err(|e| AppError::Encryption(format!("AES encryption failed: {}", e)))?;

    // Prepend nonce to (ciphertext + tag)
    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext_with_tag.len());
    result.extend_from_slice(nonce_val.as_slice()); // Prepend the raw nonce bytes
    result.extend_from_slice(&ciphertext_with_tag);

    Ok(result)
}

// Decryption is primarily for the server, but useful for testing or if client ever receives encrypted data.
#[allow(dead_code)]
pub fn decrypt_payload(encrypted_data_with_nonce: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, AppError> {
    if encrypted_data_with_nonce.len() < NONCE_SIZE {
        return Err(AppError::Decryption("Encrypted data too short to contain nonce.".to_string()));
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Decryption(format!("Failed to create AES cipher for decryption: {}", e)))?;

    let (nonce_bytes, ciphertext_with_tag) = encrypted_data_with_nonce.split_at(NONCE_SIZE);
    let nonce = Nonce::from_slice(nonce_bytes);

    // decrypt() expects the ciphertext to contain the authentication tag at its end
    cipher.decrypt(nonce, ciphertext_with_tag)
        .map_err(|e| AppError::Decryption(format!("AES decryption failed (MAC check likely failed): {}", e)))
}