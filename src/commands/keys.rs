use anyhow::{Context, Result};
use colored::Colorize;

use crate::cli::RotateKeysArgs;
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

/// Rotate the local keypair: archive existing keys, generate new ones, register in registry.
pub fn run_rotate(store: &crate::store::Store, args: &RotateKeysArgs) -> Result<()> {
    let priv_path = keys::private_key_path()?;
    let pub_path = keys::public_key_path()?;

    if priv_path.exists() {
        if let Some(archived) = keys::archive_current_keypair()? {
            println!(
                "  {} previous keypair archived to {}",
                ok(),
                archived.display()
            );
        }
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

    if let Some(n) = args.resign_last.filter(|n| *n > 0) {
        resign_last_local_traces(store, n)?;
    }

    Ok(())
}

/// Re-sign the last `n` JSONL records in the current branch's local trace buffer.
fn resign_last_local_traces(store: &crate::store::Store, n: usize) -> Result<()> {
    let branch = store
        .current_branch()
        .context("detached HEAD — use a branch to re-sign the local trace buffer")?;
    let path = store.local_traces_path(&branch);
    if !path.exists() {
        anyhow::bail!(
            "no local trace buffer at {} — nothing to re-sign",
            path.display()
        );
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let mut lines: Vec<String> = raw.lines().map(String::from).collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }
    anyhow::ensure!(!lines.is_empty(), "local trace buffer is empty");

    let take = n.min(lines.len());
    let start = lines.len() - take;
    for i in start..lines.len() {
        let mut val: serde_json::Value = serde_json::from_str(&lines[i])
            .with_context(|| format!("parsing trace line {}", i + 1))?;
        if let Some(obj) = val.as_object_mut() {
            obj.remove("sig");
        }
        let sig = keys::sign_record(&val)?;
        val.as_object_mut()
            .context("trace entry must be a JSON object")?
            .insert("sig".to_string(), serde_json::to_value(&sig)?);
        lines[i] = serde_json::to_string(&val)?;
    }

    std::fs::write(&path, lines.join("\n") + "\n")
        .with_context(|| format!("writing {}", path.display()))?;

    let label = if take == 1 { "entry" } else { "entries" };
    println!(
        "  {} re-signed last {} local trace {}",
        ok(),
        take,
        label
    );
    Ok(())
}
