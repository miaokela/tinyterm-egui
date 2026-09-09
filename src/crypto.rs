//! Secret storage — byte-for-byte compatible with the web client's
//! `src-tauri/src/crypto.rs`.
//!
//! Envelope format (`ttenc:v1:`):
//!
//! ```text
//! ttenc:v1:<b64(rsa_oaep_sha1(aes_key))>:<b64(nonce_12)>:<b64(tag_16)>:<b64(ciphertext)>
//! ```
//!
//! * RSA: 2048-bit PKCS#1 PEM key stored at `<db_dir>/secret-key.pem`, OAEP/SHA-1.
//! * AES: AES-256-GCM, 96-bit nonce, 128-bit tag, empty AAD.
//! * All base64 is standard (padded).
//!
//! Any value that does not carry the prefix is returned verbatim (legacy
//! passthrough), so an electerm-obfuscated or plaintext password still works.

use aes_gcm::aead::{AeadInOut, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce, Tag};
use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand_core::{OsRng, RngCore};
use rsa::pkcs1::{
    DecodeRsaPrivateKey, DecodeRsaPublicKey, EncodeRsaPrivateKey, EncodeRsaPublicKey,
};
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use std::path::{Path, PathBuf};

pub const ENCRYPTED_PREFIX: &str = "ttenc:v1:";
pub const PRIVATE_KEY_FILE: &str = "secret-key.pem";

pub fn is_encrypted_secret(value: &str) -> bool {
    value.starts_with(ENCRYPTED_PREFIX)
}

pub fn private_key_path(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .map(|p| p.join(PRIVATE_KEY_FILE))
        .unwrap_or_else(|| PathBuf::from(PRIVATE_KEY_FILE))
}

fn load_or_create_private_key(db_path: &Path) -> Result<RsaPrivateKey> {
    let key_path = private_key_path(db_path);
    if key_path.exists() {
        let pem = std::fs::read(&key_path)
            .with_context(|| format!("failed to read encryption key at {}", key_path.display()))?;
        return RsaPrivateKey::from_pkcs1_pem(
            std::str::from_utf8(&pem).context("encryption key is not valid UTF-8")?,
        )
        .context("failed to parse encryption key");
    }

    let key = RsaPrivateKey::new(&mut OsRng, 2048).context("failed to generate RSA keypair")?;
    let pem = key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .context("failed to encode RSA private key")?;
    std::fs::write(&key_path, pem.as_bytes())
        .with_context(|| format!("failed to write encryption key at {}", key_path.display()))?;
    restrict_permissions(&key_path);
    Ok(key)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {}

pub fn encrypt_secret(db_path: &Path, plaintext: &str) -> Result<String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }

    let rsa = load_or_create_private_key(db_path)?;
    let mut aes_key = [0u8; 32];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut aes_key);
    OsRng.fill_bytes(&mut nonce);

    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(aes_key));
    let mut buffer = plaintext.as_bytes().to_vec();
    let tag = cipher
        .encrypt_inout_detached(
            &Nonce::from(nonce),
            b"",
            (&mut buffer[..]).into(),
        )
        .map_err(|e| anyhow!("encrypt_aead: {e}"))?;

    let public_pem = RsaPublicKey::from(&rsa)
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .context("failed to encode RSA public key")?;
    let public_rsa =
        RsaPublicKey::from_pkcs1_pem(&public_pem).context("failed to parse RSA public key")?;

    let encrypted_key = public_rsa
        .encrypt(&mut OsRng, Oaep::new::<sha1::Sha1>(), &aes_key)
        .context("failed to wrap AES key")?;

    Ok(format!(
        "{}{}:{}:{}:{}",
        ENCRYPTED_PREFIX,
        STANDARD.encode(encrypted_key),
        STANDARD.encode(nonce),
        STANDARD.encode(tag),
        STANDARD.encode(buffer),
    ))
}

pub fn decrypt_secret(db_path: &Path, stored_value: &str) -> Result<String> {
    if stored_value.is_empty() {
        return Ok(String::new());
    }
    if !is_encrypted_secret(stored_value) {
        return Ok(stored_value.to_string());
    }

    let payload = stored_value
        .strip_prefix(ENCRYPTED_PREFIX)
        .ok_or_else(|| anyhow!("invalid encrypted secret prefix"))?;
    let mut parts = payload.split(':');
    let encrypted_key = parts.next().ok_or_else(|| anyhow!("missing encrypted key"))?;
    let nonce = parts.next().ok_or_else(|| anyhow!("missing nonce"))?;
    let tag = parts.next().ok_or_else(|| anyhow!("missing tag"))?;
    let ciphertext = parts.next().ok_or_else(|| anyhow!("missing ciphertext"))?;
    if parts.next().is_some() {
        return Err(anyhow!("invalid encrypted secret payload"));
    }

    let rsa = load_or_create_private_key(db_path)?;
    let encrypted_key = STANDARD.decode(encrypted_key)?;
    let nonce = fixed::<12>(&STANDARD.decode(nonce)?, "nonce")?;
    let tag = fixed::<16>(&STANDARD.decode(tag)?, "tag")?;
    let ciphertext = STANDARD.decode(ciphertext)?;

    let aes_key = fixed::<32>(
        &rsa.decrypt(Oaep::new::<sha1::Sha1>(), &encrypted_key)
            .context("failed to unwrap AES key")?,
        "aes key",
    )?;

    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(aes_key));
    let mut buffer = ciphertext;
    cipher
        .decrypt_inout_detached(
            &Nonce::from(nonce),
            b"",
            (&mut buffer[..]).into(),
            &Tag::from(tag),
        )
        .map_err(|e| anyhow!("decrypt_aead: {e}"))?;

    String::from_utf8(buffer).context("decrypted secret is not valid UTF-8")
}

/// Copy a decoded base64 field into a fixed-size array.
fn fixed<const N: usize>(bytes: &[u8], field: &str) -> Result<[u8; N]> {
    if bytes.len() != N {
        return Err(anyhow!("invalid {field} length: {} (expected {N})", bytes.len()));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    Ok(out)
}
