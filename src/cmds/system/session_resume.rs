//! rtk resume: generates a compact context summary of the current/last session.
//! Saves to ~/.rtk-resume.md so the next session starts with full context
//! without wasting turns re-explaining what was done.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

fn find_last_session_transcript() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let history_path = home.join(".claude").join("history.jsonl");
    if !history_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&history_path).ok()?;
    let last_line = content.trim().lines().last()?;
    let entry: serde_json::Value = serde_json::from_str(last_line).ok()?;

    let session_id = entry["sessionId"].as_str()?;
    let project = entry["project"].as_str()?;
    let slug = project.replace('/', "-");

    let transcript = home
        .join(".claude")
        .join("projects")
        .join(&slug)
        .join(format!("{}.jsonl", session_id));

    if transcript.exists() {
        Some(transcript)
    } else {
        None
    }
}

#[derive(Default)]
struct ResumeContext {
    files_edited: Vec<String>,
    commands_run: Vec<String>,
    user_messages: Vec<String>,
    error_count: usize,
}

fn extract_context(transcript_path: &PathBuf) -> Result<ResumeContext> {
    let content =
        fs::read_to_string(transcript_path).context("Failed to read session transcript")?;

    let mut ctx = ResumeContext::default();
    let mut seen_files = std::collections::HashSet::new();
    let mut seen_cmds = std::collections::HashSet::new();

    for line in content.lines() {
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let msg = &entry["message"];
        let role = msg["role"].as_str().unwrap_or("");

        // Capture user messages (first 60 chars each, max 8)
        if role == "user" && ctx.user_messages.len() < 8 {
            if let Some(content) = msg["content"].as_str() {
                let trimmed = content.trim();
                if trimmed.len() > 3 {
                    let preview = if trimmed.len() > 60 {
                        format!("{}…", &trimmed[..60])
                    } else {
                        trimmed.to_string()
                    };
                    ctx.user_messages.push(preview);
                }
            }
        }

        // Extract tool uses from assistant messages
        if role == "assistant" {
            if let Some(content_arr) = msg["content"].as_array() {
                for item in content_arr {
                    if item["type"].as_str() != Some("tool_use") {
                        continue;
                    }
                    let tool = item["name"].as_str().unwrap_or("");
                    let input = &item["input"];

                    match tool {
                        "Write" | "Edit" | "MultiEdit" => {
                            if let Some(path) = input["file_path"].as_str() {
                                let short = shorten_path(path);
                                if seen_files.insert(short.clone()) {
                                    ctx.files_edited.push(short);
                                }
                            }
                        }
                        "Bash" => {
                            if let Some(cmd) = input["command"].as_str() {
                                let short = cmd.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
                                if seen_cmds.insert(short.clone()) && ctx.commands_run.len() < 12 {
                                    ctx.commands_run.push(short);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Count errors from tool results
        if let Some(content_arr) = msg["content"].as_array() {
            for item in content_arr {
                if item["type"].as_str() == Some("tool_result") {
                    if let Some(c) = item["content"].as_str() {
                        if c.contains("error") || c.contains("Error") || c.contains("FAILED") {
                            ctx.error_count += 1;
                        }
                    }
                }
            }
        }
    }

    Ok(ctx)
}

fn shorten_path(path: &str) -> String {
    // Show last 2 components of path for brevity
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        path.to_string()
    } else {
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    }
}

pub fn run(output_path: Option<&str>, verbose: u8) -> Result<()> {
    let transcript = match find_last_session_transcript() {
        Some(p) => p,
        None => {
            println!("Nenhuma sessão ativa encontrada.");
            return Ok(());
        }
    };

    if verbose > 0 {
        eprintln!("Lendo transcript: {}", transcript.display());
    }

    let ctx = extract_context(&transcript)?;

    let mut summary = String::new();
    summary.push_str("# Contexto da sessão anterior\n\n");

    if !ctx.user_messages.is_empty() {
        summary.push_str("## O que foi feito\n");
        for msg in &ctx.user_messages {
            summary.push_str(&format!("- {}\n", msg));
        }
        summary.push('\n');
    }

    if !ctx.files_edited.is_empty() {
        summary.push_str("## Arquivos modificados\n");
        for f in &ctx.files_edited {
            summary.push_str(&format!("- `{}`\n", f));
        }
        summary.push('\n');
    }

    if !ctx.commands_run.is_empty() {
        summary.push_str("## Comandos executados\n");
        for cmd in ctx.commands_run.iter().take(8) {
            summary.push_str(&format!("- `{}`\n", cmd));
        }
        summary.push('\n');
    }

    if ctx.error_count > 0 {
        summary.push_str(&format!(
            "## Nota\n{} erros encontrados durante a sessão.\n\n",
            ctx.error_count
        ));
    }

    summary.push_str(
        "_Gerado por `rtk resume`. Cole no início da nova sessão para retomar sem re-explicar._\n",
    );

    let out_path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".rtk-resume.md"));

    fs::write(&out_path, &summary).with_context(|| format!("Failed to write {}", out_path.display()))?;

    println!("Resumo salvo em: {}", out_path.display());
    println!();
    print!("{}", summary);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_path() {
        assert_eq!(shorten_path("/a/b/c/d.rs"), "…/c/d.rs");
        assert_eq!(shorten_path("src/main.rs"), "src/main.rs");
        assert_eq!(shorten_path("/single"), "/single");
    }

    #[test]
    fn test_extract_context_empty() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{{\"message\":{{\"role\":\"user\",\"content\":\"hello world test\"}},\"timestamp\":1000}}").unwrap();
        let ctx = extract_context(&f.path().to_path_buf()).unwrap();
        assert_eq!(ctx.user_messages.len(), 1);
        assert_eq!(ctx.user_messages[0], "hello world test");
    }
}
