# Steam Achievement Manager

A Rust rewrite of SAM with both a CLI and a `ratatui`-based TUI.

## Requirements

- [Steam client](https://store.steampowered.com/about/) running and logged in
- Rust toolchain (for building from source)

## Building

```bash
cargo build --release
```

The binary will be at `target/release/sam`.

## Usage

```bash
# Launch the TUI
sam tui

# List all achievements for a game
sam list <app_id>

# Unlock specific achievements
sam unlock <app_id> <achievement_id> [<achievement_id>...]

# Lock (re-lock) specific achievements
sam lock <app_id> <achievement_id> [<achievement_id>...]

# Reset all unlocked achievements for a game
sam reset <app_id>
```

## TUI workflow

The TUI discovers installed games from Steam library manifests (`appmanifest_*.acf`) and shows:

- a game picker (left pane)
- achievements for the selected game (center pane)
- an operation queue (right pane)

Changes are queued first and only sent to Steam when you commit, so you can stage multiple operations before applying them.

### TUI keybindings (vim-style)

- `h` / `l`: move focus between panes
- `j` / `k`: move selection
- `g` / `G`: jump to top/bottom
- `Enter`: load selected game's achievements
- `space`: toggle selected achievement (queues lock/unlock)
- `u`: queue unlock for selected achievement
- `x`: queue lock for selected achievement (or remove queue item when queue pane focused)
- `A`: queue unlock-all for current game
- `X`: queue lock-all for current game
- `r`: queue reset-all stats+achievements for current game
- `d` / `Delete` / `Backspace`: remove selected queue item
- `c`: commit queued operations
- `q`: quit

### Examples

```bash
# List achievements for Half-Life 2 (app id 220)
sam list 220

# Unlock an achievement
sam unlock 220 HL2_HIT_CANCOP_WITHCAN

# Lock it back
sam lock 220 HL2_HIT_CANCOP_WITHCAN

# Reset all achievements
sam reset 220
```

## License

This software is provided under the [zlib license](LICENSE.txt).
