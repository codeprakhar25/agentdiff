use anyhow::{Context, Result};
use colored::Colorize;

use crate::keys;
use crate::store;
use crate::util::ok;

pub fn run_init() -> Result<()> {
    let (priv_path, pub_path, kid) = keys::generate_keypair()?;
    println!("  {} signing keypair initialized", ok());
    println!("  Private key: {} (chmod 600)", priv_path.display());
    println!("  Public key:  {}", pub_path.display());
    println!("  Key ID:      {}", kid);
    println!();
    println!("  Future trace entries will be signed automatically after each commit.");
    println!("  Run {} to verify the audit trail.", "agentdiff verify".cyan());
    Ok(())
}

/// Register the local public key in the git key registry (refs/agentdiff/keys/{key_id}).
/// This makes the key discoverable by `agentdiff verify` on other machines.
pub fn run_register(store: &crate::store::Store) -> Result<()> {
    let vk = keys::load_verifying_key()
        .context("no public key found — run 'agentdiff keys init' first")?;
    let kid = keys::compute_key_id(&vk);
    let pub_path = keys::public_key_path()?;
    let pub_b64 = std::fs::read_to_string(&pub_path)
        .with_context(|| format!("reading public key from {}", pub_path.display()))?;

    let ref_name = format!("refs/agentdiff/keys/{kid}");
    store::write_to_ref(
        &store.repo_root,
        &ref_name,
        "pub.key",
        pub_b64.trim(),
        &format!("agentdiff: register key {kid}"),
    )?;

    println!("  {} key {} registered in local registry ({})", ok(), kid, ref_name);
    println!(
        "  Run {} to push the key registry to GitHub so teammates can verify your signatures.",
        "agentdiff push".cyan()
    );
    Ok(())
}

/// Rotate the local keypair: back up existing keys, generate new ones, register in registry.
pub fn run_rotate(store: &crate::store::Store) -> Result<()> {
    let priv_path = keys::private_key_path()?;
    let pub_path = keys::public_key_path()?;

    // Back up old keys if they exist.
    if priv_path.exists() {
        let bak_priv = priv_path.with_extension("key.bak");
        let bak_pub = pub_path.with_extension("key.bak");
        std::fs::rename(&priv_path, &bak_priv)
            .with_context(|| format!("backing up private key to {}", bak_priv.display()))?;
        std::fs::rename(&pub_path, &bak_pub)
            .with_context(|| format!("backing up public key to {}", bak_pub.display()))?;
        println!(
            "  Old keys backed up to {} and {}",
            bak_priv.display(),
            bak_pub.display()
        );
    }

    let (kid, _vk) = keys::generate_keypair_at(&priv_path, &pub_path)?;
    println!("  {} new keypair generated", ok());
    println!("  Private key: {} (chmod 600)", priv_path.display());
    println!("  Public key:  {}", pub_path.display());
    println!("  Key ID:      {}", kid);

    // Register new key in local registry.
    let pub_b64 = std::fs::read_to_string(&pub_path)
        .with_context(|| format!("reading new public key from {}", pub_path.display()))?;
    let ref_name = format!("refs/agentdiff/keys/{kid}");
    store::write_to_ref(
        &store.repo_root,
        &ref_name,
        "pub.key",
        pub_b64.trim(),
        &format!("agentdiff: register rotated key {kid}"),
    )?;
    println!("  {} new key {} registered in local registry", ok(), kid);
    println!(
        "  Run {} to push the updated key registry to GitHub.",
        "agentdiff push".cyan()
    );
    Ok(())
}
