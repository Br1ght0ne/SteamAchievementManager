use anyhow::{Result, anyhow};

#[derive(Clone, Debug)]
pub(crate) struct AchievementState {
    pub(crate) name: String,
    pub(crate) achieved: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GameEntry {
    pub(crate) app_id: u32,
    pub(crate) name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StatValue {
    Int(i32),
    Float(f32),
}

pub(crate) fn parse_stat_assignment(input: &str) -> Result<(String, StatValue)> {
    let (name, value) = input
        .split_once('=')
        .ok_or_else(|| anyhow!("Expected format: STAT_NAME=value"))?;
    let stat_name = name.trim();
    let raw_value = value.trim();

    if stat_name.is_empty() {
        return Err(anyhow!("Stat name cannot be empty"));
    }
    if raw_value.is_empty() {
        return Err(anyhow!("Stat value cannot be empty"));
    }

    if raw_value.contains('.') {
        let parsed = raw_value
            .parse::<f32>()
            .map_err(|_| anyhow!("Invalid float stat value: {raw_value}"))?;
        Ok((stat_name.to_string(), StatValue::Float(parsed)))
    } else {
        let parsed = raw_value
            .parse::<i32>()
            .map_err(|_| anyhow!("Invalid integer stat value: {raw_value}"))?;
        Ok((stat_name.to_string(), StatValue::Int(parsed)))
    }
}
