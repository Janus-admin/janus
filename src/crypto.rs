use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Context, Result};
use rand::RngCore;

// ── Internal hex helpers ──────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(anyhow!("Odd-length hex string"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| anyhow!("{}", e)))
        .collect()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Encrypt `plaintext` with AES-256-GCM.
/// Returns `"hex(nonce):hex(ciphertext)"` suitable for DB storage.
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("Invalid key length: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;

    Ok(format!(
        "{}:{}",
        hex_encode(&nonce_bytes),
        hex_encode(&ciphertext)
    ))
}

/// Decrypt a value produced by `encrypt`.  Input format: `"hex(nonce):hex(ct)"`.
pub fn decrypt(encoded: &str, key: &[u8; 32]) -> Result<String> {
    let (nonce_hex, ct_hex) = encoded
        .split_once(':')
        .ok_or_else(|| anyhow!("Ciphertext missing ':' separator"))?;

    let nonce_bytes = hex_decode(nonce_hex).context("Nonce hex decode failed")?;
    if nonce_bytes.len() != 12 {
        return Err(anyhow!("Expected 12-byte nonce, got {}", nonce_bytes.len()));
    }

    let ciphertext = hex_decode(ct_hex).context("Ciphertext hex decode failed")?;

    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("Invalid key length: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|e| anyhow!("Decryption failed: {}", e))?;

    String::from_utf8(plaintext).context("Decrypted bytes are not valid UTF-8")
}

/// Parse a hex-encoded 32-byte encryption key.
/// Generate with: `openssl rand -hex 32`
pub fn parse_key(key_hex: &str) -> Result<[u8; 32]> {
    let bytes = hex_decode(key_hex).context("Key hex decode failed")?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "Encryption key must be 32 bytes (64 hex chars); got {}",
            bytes.len()
        ));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = [0u8; 32];
        let plaintext = "my-secret-provider-key";
        let enc = encrypt(plaintext, &key).unwrap();
        let dec = decrypt(&enc, &key).unwrap();
        assert_eq!(dec, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let enc = encrypt("hello", &key1).unwrap();
        assert!(decrypt(&enc, &key2).is_err());
    }
}
