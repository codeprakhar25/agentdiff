/// Internal command called by the post-commit git hook to sign the last
/// trace entry in the local buffer. Silent no-op if keys are not initialized.
use anyhow::{Context, Result};

use crate::keys;
use crate::store::Store;

pub fn run(store: &Store) -> Result<()> {
    // Signing is opt-in — silently succeed if keys are not set up.
    if !keys::keys_exist() {
        return Ok(());
    }

    let branch = match store.current_branch() {
        Ok(b) => b,
        Err(_) => return Ok(()), // detached HEAD, skip
    };

    let traces_path = store.local_traces_path(&branch);
    if !traces_path.exists() {
        return Ok(());
    }

    let raw = std::fs::read_to_string(&traces_path)
        .with_context(|| format!("reading traces {}", traces_path.display()))?;

    let mut lines: Vec<&str> = raw.lines().collect();
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    let last_line = match lines.last() {
        Some(l) => *l,
        None => return Ok(()),
    };

    let mut entry: serde_json::Value =
        serde_json::from_str(last_line).context("parsing last trace entry")?;

    // Already signed — nothing to do.
    if entry.get("sig").is_some() {
        return Ok(());
    }

    let sig = keys::sign_record(&entry)?;
    entry.as_object_mut().unwrap().insert(
        "sig".to_string(),
        serde_json::to_value(&sig).context("serializing sig")?,
    );

    let new_last = serde_json::to_string(&entry).context("re-serializing entry")?;
    lines.pop();
    lines.push(&new_last);

    let new_content = lines.join("\n") + "\n";
    std::fs::write(&traces_path, new_content)
        .with_context(|| format!("writing traces {}", traces_path.display()))?;

    Ok(())
}
