use crate::cli::{LedgerAction, LedgerArgs};
use crate::data::{LedgerRecord, NotesContributor, NotesRecord};
use crate::store::Store;
use anyhow::Result;
use colored::Colorize;
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn run(store: &Store, args: &LedgerArgs) -> Result<()> {
    match args.action {
        LedgerAction::Repair => cmd_repair(store),
        LedgerAction::ImportNotes => cmd_import_notes(store),
    }
}

fn cmd_repair(store: &Store) -> Result<()> {
    let records = store.load_ledger_records()?;
    let path = store.ledger_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut out = String::new();
    for rec in records {
        out.push_str(&serde_json::to_string(&rec)?);
        out.push('\n');
    }
    std::fs::write(&path, out)?;

    println!(
        "{} repaired ledger at {}",
        "ok".green(),
        path.display().to_string().dimmed()
    );
    Ok(())
}

fn cmd_import_notes(store: &Store) -> Result<()> {
    let existing = store.load_ledger_records()?;
    let mut existing_sha: HashSet<String> = existing.into_iter().map(|r| r.sha).collect();
    let notes = store.load_notes_records()?;
    if notes.is_empty() {
        println!("{} no notes found to import", "--".dimmed());
        return Ok(());
    }

    let mut to_append = Vec::new();
    for note in notes {
        if existing_sha.contains(&note.commit) {
            continue;
        }
        let rec = notes_to_ledger(note);
        existing_sha.insert(rec.sha.clone());
        to_append.push(rec);
    }

    if to_append.is_empty() {
        println!("{} no new note records to import", "--".dimmed());
        return Ok(());
    }

    let path = store.ledger_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    for rec in to_append {
        out.push_str(&serde_json::to_string(&rec)?);
        out.push('\n');
    }
    std::fs::write(&path, out)?;

    println!("{} imported notes into {}", "ok".green(), path.display());
    Ok(())
}

fn notes_to_ledger(note: NotesRecord) -> LedgerRecord {
    let contributors: HashMap<String, NotesContributor> = note
        .contributors
        .iter()
        .cloned()
        .map(|c| (c.id.clone(), c))
        .collect();

    let mut by_contributor: HashMap<String, usize> = HashMap::new();
    let mut lines_map: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    let mut touched: BTreeMap<String, ()> = BTreeMap::new();
    for f in &note.files {
        *by_contributor.entry(f.contributor_id.clone()).or_insert(0) += 1;
        lines_map
            .entry(f.path.clone())
            .or_default()
            .extend(f.ranges.clone());
        touched.insert(f.path.clone(), ());
    }

    let primary_id = by_contributor
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(id, _)| id);

    let primary = primary_id
        .as_ref()
        .and_then(|id| contributors.get(id))
        .cloned();

    let prompt_excerpt = primary
        .as_ref()
        .map(|c| c.prompt_excerpt.clone())
        .unwrap_or_default();
    let prompt_hash = primary
        .as_ref()
        .map(|c| {
            if c.prompt_hash.is_empty() {
                sha256_text(&prompt_excerpt)
            } else {
                c.prompt_hash.clone()
            }
        })
        .unwrap_or_else(|| sha256_text(&prompt_excerpt));

    LedgerRecord {
        sha: note.commit,
        ts: note.generated_at,
        agent: primary
            .as_ref()
            .map(|c| c.agent.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        model: primary
            .as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        session_id: primary
            .as_ref()
            .map(|c| c.session_ref.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        author: None,
        files_touched: touched.into_keys().collect(),
        lines: lines_map.into_iter().collect(),
        prompt_excerpt,
        prompt_hash,
        files_read: Vec::new(),
        intent: primary.map(|c| c.intent).filter(|v| !v.is_empty()),
        trust: None,
        flags: vec!["imported-from-notes".to_string()],
        tool: Some("imported-note".to_string()),
        mode: None,
        attribution: std::collections::HashMap::new(),
    }
}

fn sha256_text(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let out = hasher.finalize();
    format!("{:x}", out)
}
