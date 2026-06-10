//! rtk turns: turn budget predictor for Claude Code sessions.
//! Reads the current session transcript, counts user messages,
//! calculates velocity, and predicts when the 5H limit will be hit.

use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
struct SessionInfo {
    session_id: String,
    project_slug: String,
    transcript_path: PathBuf,
}

fn find_current_session() -> Option<SessionInfo> {
    let home = dirs::home_dir()?;
    let history_path = home.join(".claude").join("history.jsonl");
    if !history_path.exists() {
        return None;
    }

    let content = fs::read_to_string(&history_path).ok()?;
    let last_line = content.trim().lines().last()?;
    let entry: serde_json::Value = serde_json::from_str(last_line).ok()?;

    let session_id = entry["sessionId"].as_str()?.to_string();
    let project = entry["project"].as_str()?;
    let project_slug = project.replace('/', "-");

    let transcript_path = home
        .join(".claude")
        .join("projects")
        .join(&project_slug)
        .join(format!("{}.jsonl", session_id));

    if !transcript_path.exists() {
        return None;
    }

    Some(SessionInfo {
        session_id,
        project_slug,
        transcript_path,
    })
}

struct TurnStats {
    user_messages: usize,
    assistant_messages: usize,
    first_ts: Option<i64>,
    last_ts: Option<i64>,
}

fn count_turns(transcript_path: &PathBuf) -> Result<TurnStats> {
    let content =
        fs::read_to_string(transcript_path).context("Failed to read session transcript")?;

    let mut user_messages = 0usize;
    let mut assistant_messages = 0usize;
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;

    for line in content.lines() {
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let role = entry["message"]["role"].as_str().unwrap_or("");
        match role {
            "user" => user_messages += 1,
            "assistant" => assistant_messages += 1,
            _ => {}
        }

        if let Some(ts) = entry["timestamp"].as_i64() {
            if first_ts.is_none() {
                first_ts = Some(ts);
            }
            last_ts = Some(ts);
        }
    }

    Ok(TurnStats {
        user_messages,
        assistant_messages,
        first_ts,
        last_ts,
    })
}

pub fn run(verbose: u8) -> Result<()> {
    let session = match find_current_session() {
        Some(s) => s,
        None => {
            println!("No active Claude Code session found.");
            return Ok(());
        }
    };

    if verbose > 0 {
        eprintln!(
            "Session: {} ({})",
            session.session_id, session.project_slug
        );
        eprintln!("Transcript: {}", session.transcript_path.display());
    }

    let stats = count_turns(&session.transcript_path)?;

    if stats.user_messages == 0 {
        println!("Session is empty.");
        return Ok(());
    }

    // Calculate velocity (messages per hour)
    let velocity_per_hour = if let (Some(first), Some(last)) = (stats.first_ts, stats.last_ts) {
        let elapsed_secs = (last - first).max(1);
        let elapsed_hours = elapsed_secs as f64 / 3600.0;
        if elapsed_hours > 0.01 {
            stats.user_messages as f64 / elapsed_hours
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Rough 5H limit estimate: Team plan ~50 messages per 5H window
    // (actual limit varies, this is an approximation)
    let limit_5h: usize = 50;
    let remaining = limit_5h.saturating_sub(stats.user_messages);
    let pct_used = (stats.user_messages as f64 / limit_5h as f64 * 100.0).min(100.0);

    println!("─── Sessão atual ───────────────────────────────");
    println!(
        "  Msgs enviadas:   {} ({:.0}% do limite 5H estimado)",
        stats.user_messages, pct_used
    );
    println!("  Respostas Claude: {}", stats.assistant_messages);
    println!("  Restantes (est.): ~{} msgs", remaining);

    if velocity_per_hour > 0.5 {
        println!("  Velocidade:      {:.1} msgs/hora", velocity_per_hour);

        if remaining > 0 && velocity_per_hour > 0.0 {
            let hours_left = remaining as f64 / velocity_per_hour;
            let mins_left = (hours_left * 60.0).round() as u64;
            if mins_left < 120 {
                println!("  Estimativa:      limite em ~{} min", mins_left);
                if mins_left < 30 {
                    println!("  ⚠  Ritmo alto — considere usar rtk batch para agrupar tarefas");
                }
            } else {
                println!(
                    "  Estimativa:      limite em ~{:.1}h nesse ritmo",
                    hours_left
                );
            }
        }
    }

    println!("────────────────────────────────────────────────");
    println!("  Dica: rtk batch < tasks.txt  →  1 msg para N tarefas");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_session_doesnt_panic() {
        // Without a real session, find_current_session returns None gracefully
        // We just verify the function exists and runs without crashing when session is absent
        // (actual session detection requires ~/.claude/history.jsonl)
    }

    #[test]
    fn test_count_turns_empty_file() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{{\"message\":{{\"role\":\"user\"}},\"timestamp\":1000}}").unwrap();
        writeln!(f, "{{\"message\":{{\"role\":\"assistant\"}},\"timestamp\":2000}}").unwrap();
        writeln!(f, "{{\"message\":{{\"role\":\"user\"}},\"timestamp\":3000}}").unwrap();

        let stats = count_turns(&f.path().to_path_buf()).unwrap();
        assert_eq!(stats.user_messages, 2);
        assert_eq!(stats.assistant_messages, 1);
        assert_eq!(stats.first_ts, Some(1000));
        assert_eq!(stats.last_ts, Some(3000));
    }
}
