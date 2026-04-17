use crate::cli::ShowArgs;
use crate::store::Store;
use crate::util::{ok, print_command_header, print_not_initialized};
use anyhow::Result;
use colored::Colorize;

pub fn run(store: &Store, args: &ShowArgs) -> Result<()> {
    if !store.is_initialized() {
        print_not_initialized();
        return Ok(());
    }

    // Try UUID prefix first, then SHA prefix
    let trace = store.find_trace(&args.sha)?;
    let trace = match trace {
        Some(t) => t,
        None => {
            // Fall back to SHA search
            let by_sha = store.find_traces_by_sha(&args.sha)?;
            match by_sha.into_iter().next() {
                Some(t) => t,
                None => anyhow::bail!("No trace found for ID/SHA prefix: {}", args.sha),
            }
        }
    };

    let meta = trace.agentdiff_metadata();

    print_command_header("show");
    println!("  {}", trace.id.dimmed());
    println!();
    println!("  Trace ID:  {}", trace.id);
    println!("  Version:   {}", trace.version);
    println!("  Timestamp: {}", trace.timestamp.to_rfc3339());

    if let Some(ref vcs) = trace.vcs {
        println!("  VCS:       {} @ {}", vcs.vcs_type, vcs.revision.dimmed());
    }

    if let Some(ref tool) = trace.tool {
        println!("  Agent:     {}", crate::util::agent_color_str(&tool.name));
        if let Some(ref ver) = tool.version {
            println!("  Version:   {}", ver);
        }
    }

    if let Some(ref m) = meta {
        if let Some(ref author) = m.author {
            println!("  Author:    {}", author);
        }
        if let Some(ref session) = m.session_id {
            println!("  Session:   {}", session);
        }
        if let Some(ref prompt) = m.prompt_excerpt {
            println!("  Prompt:    {}", prompt);
        }
        if let Some(ref hash) = m.prompt_hash {
            println!("  Prompt Hash: {}", hash.dimmed());
        }
        if let Some(ref intent) = m.intent {
            if !intent.is_empty() {
                println!("  Intent:    {}", intent);
            }
        }
        if let Some(trust) = m.trust {
            println!("  Trust:     {}", trust);
        }
        if !m.flags.is_empty() {
            println!("  Flags:     {}", m.flags.join(", "));
        }
        if !m.files_read.is_empty() {
            println!("  Files Read: {}", m.files_read.join(", "));
        }
    }

    println!();
    println!("  Files:");
    for file in &trace.files {
        for conv in &file.conversations {
            let model = conv
                .contributor
                .model_id
                .as_deref()
                .unwrap_or("unknown");
            let ranges_text: Vec<String> = conv
                .ranges
                .iter()
                .map(|r| {
                    if r.start_line == r.end_line {
                        r.start_line.to_string()
                    } else {
                        format!("{}-{}", r.start_line, r.end_line)
                    }
                })
                .collect();
            println!(
                "    - {}  [{}]  ({}, {})",
                file.path,
                ranges_text.join(", ").dimmed(),
                conv.contributor.contributor_type,
                model.dimmed()
            );
        }
    }

    if trace.sig.is_some() {
        println!();
        println!("  {} signed", ok());
    }

    println!();
    Ok(())
}
