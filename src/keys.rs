use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::data::LedgerSig;

pub fn keys_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot resolve home dir")?;
    Ok(home.join(".agentdiff").join("keys"))
}

pub fn private_key_path() -> Result<PathBuf> {
    Ok(keys_dir()?.join("private.key"))
}

pub fn public_key_path() -> Result<PathBuf> {
    Ok(keys_dir()?.join("public.key"))
}

/// `~/.agentdiff/keys/archive/` — rotated key material for audit and verification.
pub fn archive_dir() -> Result<PathBuf> {
    Ok(keys_dir()?.join("archive"))
}

#[derive(Debug, Serialize, Deserialize)]
struct ArchivedKeyMeta {
    key_id: String,
    archived_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<DateTime<Utc>>,
}

/// Move the current `private.key` / `public.key` into a timestamped archive folder.
/// Returns `Ok(None)` if no keys exist on the canonical paths.
pub fn archive_current_keypair() -> Result<Option<PathBuf>> {
    let priv_path = private_key_path()?;
    let pub_path = public_key_path()?;
    if !priv_path.exists() {
        return Ok(None);
    }
    anyhow::ensure!(
        pub_path.exists(),
        "public key missing at {} — cannot archive safely",
        pub_path.display()
    );

    let vk = load_verifying_key().context("reading current public key for archive")?;
    let kid = compute_key_id(&vk);

    let dest = archive_dir()?.join(format!(
        "{}_{}",
        Utc::now().format("%Y%m%dT%H%M%SZ"),
        kid
    ));
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("creating archive dir {}", dest.display()))?;

    let dest_priv = dest.join("private.key");
    let dest_pub = dest.join("public.key");
    std::fs::rename(&priv_path, &dest_priv)
        .with_context(|| format!("archiving private key to {}", dest_priv.display()))?;
    if pub_path.exists() {
        std::fs::rename(&pub_path, &dest_pub)
            .with_context(|| format!("archiving public key to {}", dest_pub.display()))?;
    }

    let meta = ArchivedKeyMeta {
        key_id: kid,
        archived_at: Utc::now(),
        expires_at: None,
    };
    let meta_path = dest.join("archive.toml");
    std::fs::write(
        &meta_path,
        toml::to_string_pretty(&meta).context("serializing archive metadata")?,
    )
    .with_context(|| format!("writing {}", meta_path.display()))?;

    Ok(Some(dest))
}

/// Load a verifying key from the local archive when the git registry has no entry yet.
pub fn try_load_archived_verifying_key(key_id: &str) -> Result<Option<VerifyingKey>> {
    let root = match archive_dir() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    if !root.is_dir() {
        return Ok(None);
    }

    let now = Utc::now();
    for entry in std::fs::read_dir(&root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry.context("archive dir entry")?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let meta_path = path.join("archive.toml");
        let (meta_kid, expired) = if meta_path.is_file() {
            let raw = std::fs::read_to_string(&meta_path).unwrap_or_default();
            let meta: ArchivedKeyMeta = match toml::from_str(&raw) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let expired = meta
                .expires_at
                .is_some_and(|ex| ex < now);
            (meta.key_id, expired)
        } else {
            // Legacy folder: infer from public.key only.
            (String::new(), false)
        };

        if !meta_kid.is_empty() && meta_kid != key_id {
            continue;
        }
        if expired {
            continue;
        }

        let pub_path = path.join("public.key");
        if !pub_path.is_file() {
            continue;
        }

        let vk = read_verifying_key_file(&pub_path)?;
        let kid = compute_key_id(&vk);
        if kid != key_id {
            continue;
        }
        return Ok(Some(vk));
    }

    Ok(None)
}

fn read_verifying_key_file(path: &Path) -> Result<VerifyingKey> {
    let b64 = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read public key at {}", path.display()))?;
    let bytes = STANDARD
        .decode(b64.trim())
        .context("cannot base64-decode archived public key")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("archived public key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).context("invalid ed25519 public key in archive")
}

/// Generate and persist a new ed25519 keypair.
/// Errors if a private key already exists.
pub fn generate_keypair() -> Result<(PathBuf, PathBuf, String)> {
    let priv_path = private_key_path()?;
    if priv_path.exists() {
        anyhow::bail!(
            "signing key already exists at {}.\n\
             Use 'agentdiff keys rotate' to rotate.",
            priv_path.display()
        );
    }

    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let dir = keys_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating keys dir {}", dir.display()))?;

    // Write private key as base64-encoded 32-byte seed.
    let priv_b64 = STANDARD.encode(signing_key.to_bytes());
    let pub_path = public_key_path()?;
    std::fs::write(&priv_path, &priv_b64)
        .with_context(|| format!("writing private key to {}", priv_path.display()))?;

    // chmod 600 on private key (unix only).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&priv_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting permissions on {}", priv_path.display()))?;
    }

    // Write public key as base64-encoded 32-byte compressed point.
    let pub_b64 = STANDARD.encode(verifying_key.to_bytes());
    std::fs::write(&pub_path, &pub_b64)
        .with_context(|| format!("writing public key to {}", pub_path.display()))?;

    let kid = compute_key_id(&verifying_key);
    Ok((priv_path, pub_path, kid))
}

/// Load the signing key from ~/.agentdiff/keys/private.key.
pub fn load_signing_key() -> Result<SigningKey> {
    let path = private_key_path()?;
    let b64 = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read private key at {}", path.display()))?;
    let bytes = STANDARD
        .decode(b64.trim())
        .context("cannot base64-decode private key")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("private key must be 32 bytes"))?;
    Ok(SigningKey::from_bytes(&arr))
}

/// Load the verifying key from ~/.agentdiff/keys/public.key.
pub fn load_verifying_key() -> Result<VerifyingKey> {
    let path = public_key_path()?;
    let b64 = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read public key at {}", path.display()))?;
    let bytes = STANDARD
        .decode(b64.trim())
        .context("cannot base64-decode public key")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).context("invalid ed25519 public key")
}

/// Look up a verifying key from the git key registry by its key ID.
/// Reads from refs/agentdiff/keys/{key_id}:pub.key using git plumbing.
pub fn load_verifying_key_by_id(repo_root: &std::path::Path, key_id: &str) -> Result<VerifyingKey> {
    let ref_path = format!("refs/agentdiff/keys/{}:pub.key", key_id);
    let out = std::process::Command::new("git")
        .args(["cat-file", "blob", &ref_path])
        .current_dir(repo_root)
        .output()
        .context("git cat-file for key registry")?;
    anyhow::ensure!(
        out.status.success(),
        "key '{}' not found in registry (refs/agentdiff/keys/{})",
        key_id,
        key_id
    );
    let b64 = String::from_utf8(out.stdout).context("key registry entry is not valid UTF-8")?;
    let bytes = STANDARD
        .decode(b64.trim())
        .context("cannot base64-decode registry key")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("registry public key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).context("invalid ed25519 public key in registry")
}

/// Generate a keypair and write it to explicit file paths.
/// Used by `keys rotate` to generate the new key before replacing the old one.
pub fn generate_keypair_at(priv_path: &PathBuf, pub_path: &PathBuf) -> Result<(String, VerifyingKey)> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let priv_b64 = STANDARD.encode(signing_key.to_bytes());
    std::fs::write(priv_path, &priv_b64)
        .with_context(|| format!("writing private key to {}", priv_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(priv_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("setting permissions on {}", priv_path.display()))?;
    }

    let pub_b64 = STANDARD.encode(verifying_key.to_bytes());
    std::fs::write(pub_path, &pub_b64)
        .with_context(|| format!("writing public key to {}", pub_path.display()))?;

    let kid = compute_key_id(&verifying_key);
    Ok((kid, verifying_key))
}

/// Returns true if both key files exist on disk.
pub fn keys_exist() -> bool {
    private_key_path().map(|p| p.exists()).unwrap_or(false)
        && public_key_path().map(|p| p.exists()).unwrap_or(false)
}

/// First 16 hex characters of SHA-256(pubkey bytes).
pub fn compute_key_id(vk: &VerifyingKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(vk.to_bytes());
    let hash = hasher.finalize();
    hash[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

/// Sign a raw JSON value (the sig field is excluded before signing).
/// Returns a `LedgerSig` ready to embed in the ledger record.
pub fn sign_record(record: &serde_json::Value) -> Result<LedgerSig> {
    let signing_key = load_signing_key().context("run 'agentdiff keys init' first")?;
    let vk = signing_key.verifying_key();
    let kid = compute_key_id(&vk);

    let canonical = canonical_without_sig(record)?;
    let sig: Signature = signing_key.sign(canonical.as_bytes());

    Ok(LedgerSig {
        alg: "ed25519".to_string(),
        key_id: kid,
        value: STANDARD.encode(sig.to_bytes()),
    })
}

/// Verify a JSON ledger value against the provided verifying key.
/// Returns Ok(()) on valid, Err on invalid or tampered.
pub fn verify_record(record: &serde_json::Value, vk: &VerifyingKey) -> Result<()> {
    let sig_obj = record
        .get("sig")
        .context("missing 'sig' field")?;
    let sig_value = sig_obj
        .get("value")
        .and_then(|v| v.as_str())
        .context("missing sig.value")?;

    let sig_bytes = STANDARD
        .decode(sig_value)
        .context("cannot base64-decode sig.value")?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let sig = Signature::from_bytes(&sig_arr);

    let canonical = canonical_without_sig(record)?;
    vk.verify(canonical.as_bytes(), &sig)
        .context("signature verification failed — entry may have been tampered with")
}

/// Produce RFC 8785 JCS-canonical JSON with the "sig" field removed.
fn canonical_without_sig(record: &serde_json::Value) -> Result<String> {
    let mut stripped = record.clone();
    if let Some(obj) = stripped.as_object_mut() {
        obj.remove("sig");
    }
    json_canon::to_string(&stripped).context("JCS canonicalization failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn test_sign_verify_round_trip() {
        let signing_key = make_test_signing_key();
        let vk = signing_key.verifying_key();

        let record_json = serde_json::json!({
            "sha": "abc123",
            "ts": "2026-01-01T00:00:00Z",
            "agent": "claude-code",
            "model": "claude-opus-4-6",
            "session_id": "sess-1",
            "files_touched": ["src/main.rs"]
        });

        // Sign manually using internal helpers.
        let canonical = canonical_without_sig(&record_json).unwrap();
        let sig: Signature = signing_key.sign(canonical.as_bytes());
        let kid = compute_key_id(&vk);

        let mut signed = record_json.clone();
        signed.as_object_mut().unwrap().insert(
            "sig".to_string(),
            serde_json::json!({
                "alg": "ed25519",
                "key_id": kid,
                "value": STANDARD.encode(sig.to_bytes())
            }),
        );

        // Verify must pass.
        assert!(verify_record(&signed, &vk).is_ok());
    }

    #[test]
    fn test_tampered_sig_fails() {
        let signing_key = make_test_signing_key();
        let vk = signing_key.verifying_key();

        let record_json = serde_json::json!({
            "sha": "def456",
            "agent": "cursor"
        });

        let canonical = canonical_without_sig(&record_json).unwrap();
        let sig: Signature = signing_key.sign(canonical.as_bytes());

        // Flip a byte in the signature.
        let mut sig_bytes = sig.to_bytes();
        sig_bytes[0] ^= 0xff;

        let mut signed = record_json.clone();
        signed.as_object_mut().unwrap().insert(
            "sig".to_string(),
            serde_json::json!({
                "alg": "ed25519",
                "key_id": compute_key_id(&vk),
                "value": STANDARD.encode(&sig_bytes)
            }),
        );

        assert!(verify_record(&signed, &vk).is_err());
    }

    #[test]
    fn test_jcs_determinism() {
        let signing_key = make_test_signing_key();

        let record = serde_json::json!({
            "sha": "abc",
            "z_field": "last",
            "a_field": "first",
            "agent": "claude-code"
        });

        let c1 = canonical_without_sig(&record).unwrap();
        let c2 = canonical_without_sig(&record).unwrap();
        assert_eq!(c1, c2);

        let sig1: Signature = signing_key.sign(c1.as_bytes());
        let sig2: Signature = signing_key.sign(c2.as_bytes());
        assert_eq!(sig1.to_bytes(), sig2.to_bytes());
    }

    #[test]
    fn test_canonical_excludes_sig_field() {
        let record = serde_json::json!({
            "sha": "abc",
            "sig": {"alg": "ed25519", "key_id": "x", "value": "y"}
        });
        let without = serde_json::json!({ "sha": "abc" });

        let c1 = canonical_without_sig(&record).unwrap();
        let c2 = canonical_without_sig(&without).unwrap();
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_compute_key_id_is_deterministic() {
        let signing_key = make_test_signing_key();
        let vk = signing_key.verifying_key();
        let id1 = compute_key_id(&vk);
        let id2 = compute_key_id(&vk);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16); // 8 bytes = 16 hex chars
    }
}
