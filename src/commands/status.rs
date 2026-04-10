use anyhow::Result;
use colored::Colorize;

use crate::keys;
use crate::store::Store;

pub fn run(store: &Store) -> Result<()> {
    println!("{}", "agentdiff status".bold().cyan());
    println!();

    print_keys_status();
    print_traces_status(store)?;
    print_hook_status(store);
    print_unpushed_status(store);

    Ok(())
}

fn print_keys_status() {
    let priv_path = match keys::private_key_path() {
        Ok(p) => p,
        Err(_) => {
            println!("  {} signing keys  cannot resolve home dir", "?".yellow());
            return;
        }
    };
    let pub_path = match keys::public_key_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let priv_ok = priv_path.exists();
    let pub_ok = pub_path.exists();

    #[cfg(unix)]
    let perms_ok = {
        use std::os::unix::fs::MetadataExt;
        priv_path
            .metadata()
            .map(|m| m.mode() & 0o077 == 0)
            .unwrap_or(false)
    };
    #[cfg(not(unix))]
    let perms_ok = true;

    if priv_ok && pub_ok {
        let key_id = keys::load_verifying_key()
            .map(|vk| keys::compute_key_id(&vk))
            .unwrap_or_else(|_| "error reading".to_string());
        let perm_label = if perms_ok {
            "chmod 600 ok"
        } else {
            "chmod 600 MISSING"
        };
        let perm_color = if perms_ok {
            perm_label.green().to_string()
        } else {
            perm_label.red().to_string()
        };
        println!(
            "  {} signing keys  key_id={} ({})",
            "ok".green(),
            key_id,
            perm_color
        );
    } else if !priv_ok && !pub_ok {
        println!(
            "  {} signing keys  not initialized — run 'agentdiff keys init'",
            "!".yellow()
        );
    } else {
        println!(
            "  {} signing keys  partial — private={} public={}",
            "!".yellow(),
            if priv_ok { "ok" } else { "missing" },
            if pub_ok { "ok" } else { "missing" }
        );
    }
}

fn print_traces_status(store: &Store) -> Result<()> {
    let traces = store.load_meta_traces()?;

    if traces.is_empty() {
        println!(
            "  {} traces         none on agentdiff-meta",
            "--".dimmed()
        );
        return Ok(());
    }

    let total = traces.len();
    let signed = traces.iter().filter(|t| t.sig.is_some()).count();
    let last = traces.last().unwrap();
    let last_ts = last.timestamp.format("%Y-%m-%d %H:%M:%SZ");
    let last_id = &last.id[..last.id.len().min(8)];

    println!(
        "  {} traces         {} entries ({}/{} signed), last: {} ({})",
        "ok".green(),
        total,
        signed,
        total,
        last_id,
        last_ts
    );

    Ok(())
}

fn print_hook_status(store: &Store) {
    let hooks_dir = store.repo_root.join(".git").join("hooks");
    let pre = hooks_dir.join("pre-commit");
    let post = hooks_dir.join("post-commit");
    let pre_push = hooks_dir.join("pre-push");

    let pre_ok = pre.exists()
        && std::fs::read_to_string(&pre)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);
    let post_ok = post.exists()
        && std::fs::read_to_string(&post)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);
    let pre_push_ok = pre_push.exists()
        && std::fs::read_to_string(&pre_push)
            .map(|s| s.contains("agentdiff"))
            .unwrap_or(false);

    if pre_ok && post_ok && pre_push_ok {
        println!(
            "  {} git hooks      pre-commit + post-commit + pre-push installed",
            "ok".green()
        );
    } else {
        println!(
            "  {} git hooks      pre-commit={} post-commit={} pre-push={} — run 'agentdiff init'",
            "!".yellow(),
            if pre_ok { "ok" } else { "missing" },
            if post_ok { "ok" } else { "missing" },
            if pre_push_ok { "ok" } else { "missing" }
        );
    }
}

fn print_unpushed_status(store: &Store) {
    let branch = match store.current_branch() {
        Ok(b) => b,
        Err(_) => return,
    };

    let local_path = store.local_traces_path(&branch);
    if !local_path.exists() {
        return;
    }

    if let Ok(raw) = std::fs::read_to_string(&local_path) {
        let count = raw.lines().filter(|l| !l.trim().is_empty()).count();
        if count > 0 {
            println!(
                "  {} unpushed       {} trace(s) for branch '{}' — run 'agentdiff push'",
                "!".yellow(),
                count,
                branch
            );
        }
    }
}
