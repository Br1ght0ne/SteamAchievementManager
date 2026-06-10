# Steam Achievement Manager

A command-line tool to manage Steam achievements, rewritten in Rust.

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
# List all achievements for a game
sam list <app_id>

# Unlock specific achievements
sam unlock <app_id> <achievement_id> [<achievement_id>...]

# Lock (re-lock) specific achievements
sam lock <app_id> <achievement_id> [<achievement_id>...]

# Reset all unlocked achievements for a game
sam reset <app_id>
```

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
