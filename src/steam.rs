use crate::common::{AchievementState, GameEntry, StatValue};
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use steamworks::{AppId, Client};

fn init_client(app_id: u32) -> Result<Client> {
    Client::init_app(AppId(app_id))
        .context("Failed to initialize Steam client. Is Steam running and are you logged in?")
}

fn run_callbacks(client: &Client) {
    for _ in 0..50 {
        client.run_callbacks();
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn load_achievements(app_id: u32) -> Result<Vec<AchievementState>> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    let names = user_stats.get_achievement_names().unwrap_or_default();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let achieved = user_stats.achievement(&name).get().unwrap_or(false);
        out.push(AchievementState { name, achieved });
    }
    Ok(out)
}

pub(crate) fn apply_achievement_changes(app_id: u32, unlocks: &[String], locks: &[String]) -> Result<()> {
    if unlocks.is_empty() && locks.is_empty() {
        return Ok(());
    }

    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    for achievement in unlocks {
        user_stats
            .achievement(achievement)
            .set()
            .map_err(|()| anyhow!("Failed to unlock achievement: {achievement}"))?;
    }

    for achievement in locks {
        user_stats
            .achievement(achievement)
            .clear()
            .map_err(|()| anyhow!("Failed to lock achievement: {achievement}"))?;
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow!("Failed to store stats"))?;
    run_callbacks(&client);

    Ok(())
}

pub(crate) fn apply_stat_changes(app_id: u32, stats: &[(String, StatValue)]) -> Result<()> {
    if stats.is_empty() {
        return Ok(());
    }

    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    for (name, value) in stats {
        match value {
            StatValue::Int(v) => user_stats
                .set_stat_i32(name, *v)
                .map_err(|()| anyhow!("Failed to set i32 stat {name}={v}"))?,
            StatValue::Float(v) => user_stats
                .set_stat_f32(name, *v)
                .map_err(|()| anyhow!("Failed to set f32 stat {name}={v}"))?,
        }
    }

    user_stats
        .store_stats()
        .map_err(|()| anyhow!("Failed to store stats"))?;
    run_callbacks(&client);
    Ok(())
}

pub(crate) fn reset_all(app_id: u32) -> Result<()> {
    let client = init_client(app_id)?;
    let user_stats = client.user_stats();
    run_callbacks(&client);

    user_stats
        .reset_all_stats(true)
        .map_err(|()| anyhow!("Failed to reset stats/achievements"))?;
    user_stats
        .store_stats()
        .map_err(|()| anyhow!("Failed to store stats"))?;
    run_callbacks(&client);
    Ok(())
}

pub(crate) fn discover_games() -> Vec<GameEntry> {
    let mut all_games = BTreeMap::<u32, String>::new();
    for path in discover_steam_libraries() {
        for game in discover_games_in_library(&path) {
            all_games.entry(game.app_id).or_insert(game.name);
        }
    }

    all_games
        .into_iter()
        .map(|(app_id, name)| GameEntry { app_id, name })
        .collect()
}

fn discover_steam_libraries() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(&home).join(".steam/steam"));
        roots.push(PathBuf::from(home).join(".local/share/Steam"));
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&profile).join("AppData/Local/Steam"));
    }

    if let Ok(pfx86) = std::env::var("PROGRAMFILES(X86)") {
        roots.push(PathBuf::from(pfx86).join("Steam"));
    }

    let mut libraries = HashSet::<PathBuf>::new();
    for root in roots {
        let steamapps = root.join("steamapps");
        if !steamapps.exists() {
            continue;
        }

        libraries.insert(root.clone());

        let library_folders = steamapps.join("libraryfolders.vdf");
        if library_folders.exists() {
            for path in parse_library_paths(&library_folders) {
                libraries.insert(path);
            }
        }
    }

    let mut out: Vec<_> = libraries.into_iter().collect();
    out.sort();
    out
}

fn parse_library_paths(path: &Path) -> Vec<PathBuf> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };

    let path_re = Regex::new(r#"\"path\"\s*\"([^\"]+)\""#).expect("regex compile");
    path_re
        .captures_iter(&content)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().replace("\\\\", "\\")))
        .map(PathBuf::from)
        .collect()
}

fn discover_games_in_library(library_root: &Path) -> Vec<GameEntry> {
    let steamapps = library_root.join("steamapps");
    let Ok(entries) = fs::read_dir(&steamapps) else {
        return Vec::new();
    };

    let mut games = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
            continue;
        }

        if let Some(game) = parse_app_manifest(&path) {
            games.push(game);
        }
    }

    games
}

fn parse_app_manifest(path: &Path) -> Option<GameEntry> {
    let content = fs::read_to_string(path).ok()?;
    let app_id_re = Regex::new(r#"\"appid\"\s*\"(\d+)\""#).ok()?;
    let name_re = Regex::new(r#"\"name\"\s*\"([^\"]+)\""#).ok()?;

    let app_id = app_id_re
        .captures(&content)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u32>().ok())?;

    let name = name_re
        .captures(&content)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| format!("App {app_id}"));

    Some(GameEntry { app_id, name })
}
