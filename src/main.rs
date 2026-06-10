use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use steamworks::{AppId, Client};

/// Steam Achievement Manager – CLI
#[derive(Parser)]
#[command(name = "sam", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    /// Reset all achievements for a game
    Reset {
        /// Steam App ID
        app_id: u32,
    },
}

fn init_client(app_id: u32) -> Result<Client> {
    let client = Client::init_app(AppId(app_id))
        .context("Failed to initialize Steam client. Is Steam running and are you logged in?")?;
    Ok(client)
}

fn run_callbacks(client: &Client) {
    for _ in 0..50 {
        client.run_callbacks();
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn cmd_list(app_id: u32) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();

    // Give Steam time to load stats
    run_callbacks(&client);

    let names = user_stats.get_achievement_names();

    match names {
        Some(names) if !names.is_empty() => {
            println!("Achievements for app {app_id}:");
            println!("{:<50} STATUS", "NAME");
            println!("{}", "-".repeat(60));

            for name in &names {
                let achieved = user_stats.achievement(name).get().unwrap_or(false);
                let status = if achieved { "✓ Unlocked" } else { "✗ Locked" };
                println!("{:<50} {status}", name);
            }
        }
        _ => {
            println!("No achievements found for app {app_id}.");
        }
    }

    Ok(())
}

fn cmd_unlock(app_id: u32, achievements: &[String]) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();

    run_callbacks(&client);

    for name in achievements {
        let ach = user_stats.achievement(name);
        if ach.set().is_ok() {
            println!("Unlocked: {name}");
        } else {
            eprintln!("Failed to unlock: {name}");
        }
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow::anyhow!("Failed to store stats"))?;
    run_callbacks(&client);

    println!("Changes saved.");
    Ok(())
}

fn cmd_lock(app_id: u32, achievements: &[String]) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();

    run_callbacks(&client);

    for name in achievements {
        let ach = user_stats.achievement(name);
        if ach.clear().is_ok() {
            println!("Locked: {name}");
        } else {
            eprintln!("Failed to lock: {name}");
        }
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow::anyhow!("Failed to store stats"))?;
    run_callbacks(&client);

    println!("Changes saved.");
    Ok(())
}

fn cmd_reset(app_id: u32) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();

    run_callbacks(&client);

    let names = user_stats.get_achievement_names();
    let mut count = 0u32;

    if let Some(names) = names {
        for name in &names {
            let ach = user_stats.achievement(name);
            if ach.get().unwrap_or(false) && ach.clear().is_ok() {
                count += 1;
            }
        }
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow::anyhow!("Failed to store stats"))?;
    run_callbacks(&client);

    println!("Reset {count} achievement(s) for app {app_id}.");
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
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
