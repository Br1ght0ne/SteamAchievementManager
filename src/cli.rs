use crate::{steam, tui};
use anyhow::Result;
use clap::{Parser, Subcommand};

/// Steam Achievement Manager – CLI + TUI
#[derive(Parser)]
#[command(name = "sam", version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Launch ratatui interface
    Tui,
    /// List discovered installed games with Steam App IDs
    Games,
    /// List all achievements for a game
    List {
        /// Steam App ID
        app_id: u32,
    },
    /// Unlock one or more achievements
    Unlock {
        /// Steam App ID
        app_id: u32,
        /// Achievement API name(s) to unlock
        #[arg(required = true)]
        achievements: Vec<String>,
    },
    /// Lock (re-lock) one or more achievements
    Lock {
        /// Steam App ID
        app_id: u32,
        /// Achievement API name(s) to lock
        #[arg(required = true)]
        achievements: Vec<String>,
    },
    /// Reset all achievements and stats for a game
    Reset {
        /// Steam App ID
        app_id: u32,
    },
}

pub(crate) fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Tui => tui::run_tui(),
        Command::Games => cmd_games(),
        Command::List { app_id } => cmd_list(app_id),
        Command::Unlock {
            app_id,
            achievements,
        } => cmd_unlock(app_id, &achievements),
        Command::Lock {
            app_id,
            achievements,
        } => cmd_lock(app_id, &achievements),
        Command::Reset { app_id } => cmd_reset(app_id),
    }
}

fn cmd_list(app_id: u32) -> Result<()> {
    let achievements = steam::load_achievements(app_id)?;
    if achievements.is_empty() {
        println!("No achievements found for app {app_id}.");
        return Ok(());
    }

    println!("Achievements for app {app_id}:");
    println!("{:<50} STATUS", "NAME");
    println!("{}", "-".repeat(60));

    for item in achievements {
        let status = if item.achieved {
            "✓ Unlocked"
        } else {
            "✗ Locked"
        };
        println!("{:<50} {status}", item.name);
    }

    Ok(())
}

fn cmd_games() -> Result<()> {
    let games = steam::discover_games();
    if games.is_empty() {
        println!("No installed games found.");
        return Ok(());
    }

    println!("Installed games:");
    println!("{:<10} NAME", "APP ID");
    println!("{}", "-".repeat(70));

    for game in games {
        println!("{:<10} {}", game.app_id, game.name);
    }

    Ok(())
}

fn cmd_unlock(app_id: u32, achievements: &[String]) -> Result<()> {
    steam::apply_achievement_changes(app_id, achievements, &[])?;
    println!("Unlocked {} achievement(s).", achievements.len());
    Ok(())
}

fn cmd_lock(app_id: u32, achievements: &[String]) -> Result<()> {
    steam::apply_achievement_changes(app_id, &[], achievements)?;
    println!("Locked {} achievement(s).", achievements.len());
    Ok(())
}

fn cmd_reset(app_id: u32) -> Result<()> {
    steam::reset_all(app_id)?;
    println!("Reset all achievements and stats for app {app_id}.");
    Ok(())
}
