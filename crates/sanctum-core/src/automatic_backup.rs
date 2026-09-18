use crate::domain::AutomaticBackupConfig;
use crate::error::{Result, SanctumError};
use crate::{atomic_write, utc_now, Vault};
use chrono::{DateTime, Duration, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const SETTINGS_DIRECTORY: &str = "settings/automatic-backup";
const DEFAULT_INTERVAL_HOURS: u32 = 24;

impl Vault {
    pub fn automatic_backup_config(&self) -> Result<AutomaticBackupConfig> {
        let directory = self.root.join(SETTINGS_DIRECTORY);
        if !directory.is_dir() {
            return Ok(disabled_config());
        }
        let mut records = fs::read_dir(&directory)?
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        records.sort_by_key(|entry| entry.file_name());
        let Some(latest) = records.last() else {
            return Ok(disabled_config());
        };
        let config: AutomaticBackupConfig = serde_json::from_slice(&fs::read(latest.path())?)?;
        validate_config_shape(&config)?;
        Ok(config)
    }

    pub fn save_automatic_backup_config(
        &self,
        enabled: bool,
        destination_directory: impl AsRef<Path>,
        last_success_at: Option<String>,
    ) -> Result<AutomaticBackupConfig> {
        let destination_directory = destination_directory.as_ref();
        if enabled {
            if !destination_directory.is_dir() {
                return Err(SanctumError::InvalidInput(
                    "automatic backup destination must be an existing directory".into(),
                ));
            }
            let canonical_destination = destination_directory.canonicalize()?;
            let canonical_root = self.root.canonicalize()?;
            if canonical_destination.starts_with(&canonical_root) {
                return Err(SanctumError::InvalidInput(
                    "automatic backup destination must be outside the live Vault".into(),
                ));
            }
        }
        if let Some(value) = last_success_at.as_deref() {
            DateTime::parse_from_rfc3339(value).map_err(|_| {
                SanctumError::InvalidInput("last automatic backup time is invalid".into())
            })?;
        }
        let config = AutomaticBackupConfig {
            enabled,
            destination_directory: if destination_directory.as_os_str().is_empty() {
                String::new()
            } else {
                destination_directory.to_string_lossy().into_owned()
            },
            interval_hours: DEFAULT_INTERVAL_HOURS,
            last_success_at,
            updated_at: utc_now(),
        };
        let directory = self.root.join(SETTINGS_DIRECTORY);
        fs::create_dir_all(&directory)?;
        let sequence = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        atomic_write(
            &directory.join(format!("{sequence:019}-{}.json", Uuid::new_v4())),
            serde_json::to_string_pretty(&config)?.as_bytes(),
        )?;
        Ok(config)
    }

    pub fn automatic_backup_due(&self, config: &AutomaticBackupConfig) -> Result<bool> {
        if !config.enabled {
            return Ok(false);
        }
        validate_config_shape(config)?;
        let Some(last_success_at) = config.last_success_at.as_deref() else {
            return Ok(true);
        };
        let last = DateTime::parse_from_rfc3339(last_success_at)
            .map_err(|_| SanctumError::Integrity("automatic backup time is invalid".into()))?
            .with_timezone(&Utc);
        Ok(Utc::now() >= last + Duration::hours(i64::from(config.interval_hours)))
    }

    pub fn next_automatic_backup_path(
        &self,
        config: &AutomaticBackupConfig,
    ) -> Result<PathBuf> {
        validate_config_shape(config)?;
        let destination = PathBuf::from(&config.destination_directory);
        if !destination.is_dir() {
            return Err(SanctumError::InvalidInput(
                "automatic backup destination is unavailable".into(),
            ));
        }
        let safe_name = safe_component(&self.manifest.name);
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let unique = Uuid::new_v4().simple().to_string();
        Ok(destination.join(format!(
            "Sanctum-{safe_name}-{timestamp}-{}.sanctum-backup",
            &unique[..8]
        )))
    }
}

fn disabled_config() -> AutomaticBackupConfig {
    AutomaticBackupConfig {
        enabled: false,
        destination_directory: String::new(),
        interval_hours: DEFAULT_INTERVAL_HOURS,
        last_success_at: None,
        updated_at: utc_now(),
    }
}

fn validate_config_shape(config: &AutomaticBackupConfig) -> Result<()> {
    if !(1..=24 * 31).contains(&config.interval_hours) {
        return Err(SanctumError::Integrity(
            "automatic backup interval is outside the supported range".into(),
        ));
    }
    if config.enabled && config.destination_directory.trim().is_empty() {
        return Err(SanctumError::Integrity(
            "automatic backup destination is missing".into(),
        ));
    }
    Ok(())
}

fn safe_component(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            character if character.is_control() => '-',
            character => character,
        })
        .collect::<String>();
    let value = value.trim_matches(|character: char| character == ' ' || character == '.');
    if value.is_empty() {
        "Vault".into()
    } else {
        value.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_component_removes_windows_path_characters() {
        assert_eq!(safe_component("Model: a/b?"), "Model- a-b-");
        assert_eq!(safe_component(" .. "), "Vault");
    }
}
