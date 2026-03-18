//! PIN-based encryption for storing master passwords.
//!
//! Uses Argon2id to derive an AES-256-GCM key from a 4-digit PIN + random salt.
//! The encrypted master password is stored in the database alongside the salt,
//! nonce, and expiry timestamp. The PIN is valid for 30 days.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use anyhow::{Context, Result};
use argon2::Argon2;
use rand::RngCore;
use zeroize::Zeroize;

/// Number of days a PIN remains valid.
const PIN_VALIDITY_DAYS: i64 = 30;

/// Encrypt a master password using a 4-digit PIN.
/// Returns (encrypted_password_b64, salt_b64, nonce_b64, expires_at_iso).
pub fn encrypt_with_pin(password: &str, pin: &str) -> Result<(String, String, String, String)> {
    assert!(pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit()));

    // Generate random salt (16 bytes) and nonce (12 bytes for AES-GCM)
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce_bytes);

    // Derive a 256-bit key from PIN + salt using Argon2id
    let mut key = derive_key(pin, &salt)?;

    // Encrypt the password
    let cipher = Aes256Gcm::new_from_slice(&key).context("Failed to create AES-GCM cipher")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {e}"))?;

    // Zeroize the key
    key.zeroize();

    // Calculate expiry
    let expires_at = chrono::Utc::now() + chrono::Duration::days(PIN_VALIDITY_DAYS);
    let expires_at_str = expires_at.to_rfc3339();

    // Encode to base64
    use base64_encode as b64;
    Ok((
        b64(&ciphertext),
        b64(&salt),
        b64(&nonce_bytes),
        expires_at_str,
    ))
}

/// Decrypt a master password using a 4-digit PIN.
/// Returns the decrypted password, or an error if the PIN is wrong or data is corrupted.
pub fn decrypt_with_pin(
    encrypted_password_b64: &str,
    salt_b64: &str,
    nonce_b64: &str,
    pin: &str,
) -> Result<String> {
    assert!(pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit()));

    let ciphertext = base64_decode(encrypted_password_b64)?;
    let salt = base64_decode(salt_b64)?;
    let nonce_bytes = base64_decode(nonce_b64)?;

    if nonce_bytes.len() != 12 {
        anyhow::bail!("Invalid nonce length");
    }

    // Derive key from PIN + salt
    let mut key = derive_key(pin, &salt)?;

    // Decrypt
    let cipher = Aes256Gcm::new_from_slice(&key).context("Failed to create AES-GCM cipher")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow::anyhow!("Decryption failed — wrong PIN"))?;

    // Zeroize the key
    key.zeroize();

    String::from_utf8(plaintext).context("Decrypted password is not valid UTF-8")
}

/// Check if a PIN has expired based on the stored expiry timestamp.
pub fn is_pin_expired(expires_at: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(expires_at) {
        Ok(expiry) => chrono::Utc::now() > expiry,
        Err(_) => true, // If we can't parse it, treat as expired
    }
}

// ── Internal helpers ─────────────────────────────────────────────────

/// Derive a 256-bit AES key from a PIN and salt using Argon2id.
fn derive_key(pin: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    let argon2 = Argon2::default();
    argon2
        .hash_password_into(pin.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow::anyhow!("Key derivation failed: {e}"))?;
    Ok(key)
}

fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    // Simple base64 encoding using the standard alphabet
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut i = 0;
    while i + 2 < data.len() {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        let b2 = data[i + 2] as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        out.push(CHARS[(triple & 0x3F) as usize] as char);
        i += 3;
    }

    let remaining = data.len() - i;
    if remaining == 2 {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        let triple = (b0 << 16) | (b1 << 8);
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        out.push('=');
    } else if remaining == 1 {
        let b0 = data[i] as u32;
        let triple = b0 << 16;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        let _ = write!(out, "==");
    }

    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const DECODE: [u8; 128] = {
        let mut table = [255u8; 128];
        let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            table[chars[i] as usize] = i as u8;
            i += 1;
        }
        table
    };

    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);

    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();

    let mut i = 0;
    while i + 3 < bytes.len() {
        let b0 = DECODE[bytes[i] as usize] as u32;
        let b1 = DECODE[bytes[i + 1] as usize] as u32;
        let b2 = DECODE[bytes[i + 2] as usize] as u32;
        let b3 = DECODE[bytes[i + 3] as usize] as u32;
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6) | b3;
        out.push(((triple >> 16) & 0xFF) as u8);
        out.push(((triple >> 8) & 0xFF) as u8);
        out.push((triple & 0xFF) as u8);
        i += 4;
    }

    let remaining = bytes.len() - i;
    if remaining == 3 {
        let b0 = DECODE[bytes[i] as usize] as u32;
        let b1 = DECODE[bytes[i + 1] as usize] as u32;
        let b2 = DECODE[bytes[i + 2] as usize] as u32;
        let triple = (b0 << 18) | (b1 << 12) | (b2 << 6);
        out.push(((triple >> 16) & 0xFF) as u8);
        out.push(((triple >> 8) & 0xFF) as u8);
    } else if remaining == 2 {
        let b0 = DECODE[bytes[i] as usize] as u32;
        let b1 = DECODE[bytes[i + 1] as usize] as u32;
        let triple = (b0 << 18) | (b1 << 12);
        out.push(((triple >> 16) & 0xFF) as u8);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "MyS3cureP@ssw0rd!";
        let pin = "1234";

        let (enc, salt, nonce, expires) = encrypt_with_pin(password, pin).unwrap();
        let decrypted = decrypt_with_pin(&enc, &salt, &nonce, pin).unwrap();

        assert_eq!(decrypted, password);
        assert!(!is_pin_expired(&expires));
    }

    #[test]
    fn test_wrong_pin_fails() {
        let password = "MyS3cureP@ssw0rd!";
        let pin = "1234";

        let (enc, salt, nonce, _) = encrypt_with_pin(password, pin).unwrap();
        let result = decrypt_with_pin(&enc, &salt, &nonce, "5678");

        assert!(result.is_err());
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = b"Hello, World! This is a test of base64 encoding.";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
