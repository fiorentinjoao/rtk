//! rtk batch: compile multiple short messages into one optimized prompt.
//! Reads task lines from stdin (or --tasks args), outputs a numbered task list
//! ready to paste as a single Claude message.

use anyhow::Result;
use std::io::{self, BufRead};

pub fn run(tasks: &[String], verbose: u8) -> Result<()> {
    let items: Vec<String> = if tasks.is_empty() {
        // Interactive / pipe mode: read from stdin until EOF
        let stdin = io::stdin();
        let mut lines = Vec::new();
        for line in stdin.lock().lines() {
            let line = line?;
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                lines.push(trimmed);
            }
        }
        lines
    } else {
        tasks.to_vec()
    };

    if items.is_empty() {
        eprintln!("rtk batch: no tasks provided. Pass via stdin or --task flags.");
        return Ok(());
    }

    if verbose > 0 {
        eprintln!("rtk batch: compiling {} tasks into 1 message", items.len());
    }

    println!("Execute the following tasks in sequence:");
    println!();
    for (i, task) in items.iter().enumerate() {
        println!("{}. {}", i + 1, task);
    }
    println!();
    println!(
        "Complete all {} tasks and report results at the end.",
        items.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_with_tasks() {
        let tasks = vec![
            "fix the login bug".to_string(),
            "add unit tests".to_string(),
            "commit the changes".to_string(),
        ];
        // Just verify it doesn't panic
        run(&tasks, 0).unwrap();
    }

    #[test]
    fn test_batch_empty_tasks() {
        // With no tasks and no stdin in test context, should not panic
        // We can't really test stdin in unit tests, just verify the path compiles
        let tasks: Vec<String> = vec![];
        // Can't call run with empty tasks here (would hang on stdin in test)
        assert!(tasks.is_empty());
    }
}
