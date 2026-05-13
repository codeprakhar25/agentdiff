use std::collections::HashMap;

use anyhow::Result;
use ed25519_dalek::VerifyingKey;

use crate::cli::VerifyArgs;
use crate::keys;
use crate::store::Store;
use crate::util::{err, warn};

pub fn run(store: &Store, args: &VerifyArgs) -> Result<()> {
    // Build a key cache seeded with the local key (if present).
    // Keys are looked up by key_id on demand from the git registry.
    let mut key_cache: HashMap<String, VerifyingKey> = HashMap::new();

    match keys::load_verifying_key() {
        Ok(vk) => {
            let kid = keys::compute_key_id(&vk);
            key_cache.insert(kid, vk);
        }
        Err(_) => {
            // No local key — verification will rely entirely on the key registry.
        }
    }

    if key_cache.is_empty() {
        eprintln!(
            "{} no local key found and key registry may be empty — \
             run 'agentdiff keys init' or 'agentdiff keys register'",
            warn()
        );
    }

    let since_sha: Option<String> = match &args.since {
        Some(sha) => Some(sha.clone()),
        None => find_merge_base(&store.repo_root),
    };

    // Load raw JSON values for exact-bytes verification.
    let records = store.load_all_traces_raw()?;

    if records.is_empty() {
        println!("Nothing to verify (no traces).");
        return Ok(());
    }

    // Filter to records after `since_sha`.
    let to_verify: Vec<&serde_json::Value> = match &since_sha {
        None => records.iter().collect(),
        Some(base) => {
            let pos = records.iter().position(|r| {
                r.get("vcs")
                    .and_then(|v| v.get("revision"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.starts_with(base.as_str()))
                    .unwrap_or(false)
            });
            match pos {
                Some(i) => records[i + 1..].iter().collect(),
                None => {
                    eprintln!(
                        "{} base SHA {} not found in traces; verifying all entries",
                        warn(),
                        &base[..base.len().min(8)]
                    );
                    records.iter().collect()
                }
            }
        }
    };

    if to_verify.is_empty() {
        println!("Nothing to verify (no entries after base).");
        return Ok(());
    }

    let mut valid = 0usize;
    let mut missing_sig = 0usize;
    let mut invalid_sig = 0usize;

    for raw in &to_verify {
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let short = &id[..id.len().min(8)];

        let sig_obj = match raw.get("sig") {
            None => {
                missing_sig += 1;
                if args.strict {
                    eprintln!("{} {} — no signature (--strict)", err(), short);
                    print_summary(to_verify.len(), valid, missing_sig, invalid_sig);
                    std::process::exit(1);
                } else {
                    eprintln!("{} {} — no signature", warn(), short);
                }
                continue;
            }
            Some(s) => s,
        };

        // Resolve the verifying key: prefer the key_id recorded in the signature.
        let key_id = sig_obj.get("key_id").and_then(|v| v.as_str());
        let vk: &VerifyingKey = if let Some(kid) = key_id {
            if !key_cache.contains_key(kid) {
                // Try to fetch from registry.
                match keys::load_verifying_key_by_id(&store.repo_root, kid) {
                    Ok(k) => {
                        key_cache.insert(kid.to_string(), k);
                    }
                    Err(_) => match keys::try_load_archived_verifying_key(kid)? {
                        Some(k) => {
                            key_cache.insert(kid.to_string(), k);
                        }
                        None => {
                            invalid_sig += 1;
                            eprintln!(
                                "{} {} — key_id {} not found locally, in registry, or in ~/.agentdiff/keys/archive \
                                 (run 'agentdiff keys register' on the signing machine)",
                                err(),
                                short,
                                &kid[..kid.len().min(16)]
                            );
                            if args.strict {
                                print_summary(to_verify.len(), valid, missing_sig, invalid_sig);
                                std::process::exit(2);
                            }
                            continue;
                        }
                    },
                }
            }
            key_cache.get(kid).unwrap()
        } else if let Some(vk) = key_cache.values().next() {
            // No key_id in sig (pre-registry traces) — fall back to the first loaded key.
            vk
        } else {
            invalid_sig += 1;
            eprintln!(
                "{} {} — no key_id in signature and no local key available",
                err(),
                short
            );
            if args.strict {
                print_summary(to_verify.len(), valid, missing_sig, invalid_sig);
                std::process::exit(2);
            }
            continue;
        };

        match keys::verify_record(raw, vk) {
            Ok(()) => {
                valid += 1;
            }
            Err(e) => {
                invalid_sig += 1;
                eprintln!("{} {} — {}", err(), short, e);
                if args.strict {
                    print_summary(to_verify.len(), valid, missing_sig, invalid_sig);
                    std::process::exit(2);
                }
            }
        }
    }

    print_summary(to_verify.len(), valid, missing_sig, invalid_sig);

    if invalid_sig > 0 {
        std::process::exit(2);
    }
    if missing_sig > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn print_summary(total: usize, valid: usize, missing: usize, invalid: usize) {
    println!(
        "Verified {} entries: {} valid, {} missing sig, {} invalid",
        total, valid, missing, invalid
    );
}

fn find_merge_base(repo_root: &std::path::Path) -> Option<String> {
    // Prefer the remote's default branch (cached, no network call),
    // then fall back to main/master.
    let remote_head = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_start_matches("refs/remotes/origin/").to_string())
        .filter(|s| !s.is_empty());

    let mut candidates: Vec<String> = Vec::new();
    if let Some(b) = remote_head {
        candidates.push(b);
    }
    for fallback in ["main", "master"] {
        let fb = fallback.to_string();
        if !candidates.contains(&fb) {
            candidates.push(fb);
        }
    }

    for branch in &candidates {
        let out = std::process::Command::new("git")
            .args(["merge-base", "HEAD", branch])
            .current_dir(repo_root)
            .output()
            .ok()?;
        if out.status.success() {
            let sha = String::from_utf8(out.stdout).ok()?.trim().to_string();
            if !sha.is_empty() {
                return Some(sha);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::data::LedgerSig;

    #[test]
    fn test_short_id() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(&id[..id.len().min(8)], "550e8400");
        let short = "abc";
        assert_eq!(&short[..short.len().min(8)], "abc");
    }

    #[test]
    fn test_sig_struct_serializes() {
        let sig = LedgerSig {
            alg: "ed25519".to_string(),
            key_id: "deadbeef01234567".to_string(),
            value: "AAAA".to_string(),
        };
        let v = serde_json::to_value(&sig).unwrap();
        assert_eq!(v["alg"], "ed25519");
    }
}
