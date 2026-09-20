use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const PLUGIN_MANIFEST: &str = include_str!("../../plugins/sanctum/.codex-plugin/plugin.json");
const MCP_CONFIG: &str = include_str!("../../plugins/sanctum/.mcp.json");
const SKILL: &str = include_str!("../../plugins/sanctum/skills/sanctum-workspace/SKILL.md");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatGptPluginStatus {
    pub installed: bool,
    pub plugin_path: String,
    pub marketplace_path: String,
    pub deep_link: String,
}

pub(crate) fn status() -> Result<ChatGptPluginStatus, String> {
    let locations = locations()?;
    let files_match = file_matches(
        &locations.plugin_root.join(".codex-plugin/plugin.json"),
        PLUGIN_MANIFEST,
    ) && file_matches(&locations.plugin_root.join(".mcp.json"), MCP_CONFIG)
        && file_matches(
            &locations
                .plugin_root
                .join("skills/sanctum-workspace/SKILL.md"),
            SKILL,
        );
    let marketplace_matches = marketplace_contains_sanctum(&locations.marketplace_path)?;
    Ok(locations.status(files_match && marketplace_matches))
}

pub(crate) fn install() -> Result<ChatGptPluginStatus, String> {
    let locations = locations()?;
    write_text(
        &locations.plugin_root.join(".codex-plugin/plugin.json"),
        PLUGIN_MANIFEST,
    )?;
    write_text(&locations.plugin_root.join(".mcp.json"), MCP_CONFIG)?;
    write_text(
        &locations
            .plugin_root
            .join("skills/sanctum-workspace/SKILL.md"),
        SKILL,
    )?;
    update_marketplace(&locations.marketplace_path)?;
    Ok(locations.status(true))
}

struct PluginLocations {
    plugin_root: PathBuf,
    marketplace_path: PathBuf,
}

impl PluginLocations {
    fn status(&self, installed: bool) -> ChatGptPluginStatus {
        ChatGptPluginStatus {
            installed,
            plugin_path: self.plugin_root.to_string_lossy().into_owned(),
            marketplace_path: self.marketplace_path.to_string_lossy().into_owned(),
            deep_link: format!(
                "codex://plugins/sanctum?marketplacePath={}",
                percent_encode(&self.marketplace_path.to_string_lossy())
            ),
        }
    }
}

fn locations() -> Result<PluginLocations, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "ユーザーフォルダを特定できない".to_owned())?;
    Ok(PluginLocations {
        plugin_root: home.join("plugins/sanctum"),
        marketplace_path: home.join(".agents/plugins/marketplace.json"),
    })
}

fn file_matches(path: &Path, expected: &str) -> bool {
    fs::read_to_string(path)
        .map(|actual| actual == expected)
        .unwrap_or(false)
}

fn marketplace_contains_sanctum(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let document: Value = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("個人用プラグイン一覧を読めない: {error}"))?,
    )
    .map_err(|error| format!("個人用プラグイン一覧のJSONが壊れている: {error}"))?;
    Ok(document
        .get("plugins")
        .and_then(Value::as_array)
        .is_some_and(|plugins| plugins.iter().any(valid_sanctum_entry)))
}

fn valid_sanctum_entry(entry: &Value) -> bool {
    entry.get("name").and_then(Value::as_str) == Some("sanctum")
        && entry.pointer("/source/source").and_then(Value::as_str) == Some("local")
        && entry.pointer("/source/path").and_then(Value::as_str) == Some("./plugins/sanctum")
}

fn update_marketplace(path: &Path) -> Result<(), String> {
    let mut document = if path.is_file() {
        serde_json::from_slice::<Value>(
            &fs::read(path)
                .map_err(|error| format!("個人用プラグイン一覧を読めない: {error}"))?,
        )
        .map_err(|error| {
            format!(
                "個人用プラグイン一覧のJSONが壊れているため、安全に更新できない: {error}"
            )
        })?
    } else {
        json!({
            "name": "personal",
            "interface": {"displayName": "Personal"},
            "plugins": []
        })
    };

    let root = document
        .as_object_mut()
        .ok_or_else(|| "個人用プラグイン一覧はJSON objectである必要がある".to_owned())?;
    root.entry("name")
        .or_insert_with(|| Value::String("personal".into()));
    root.entry("interface").or_insert_with(|| {
        let mut interface = Map::new();
        interface.insert("displayName".into(), Value::String("Personal".into()));
        Value::Object(interface)
    });
    let plugins = root
        .entry("plugins")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "個人用プラグイン一覧のpluginsが配列ではない".to_owned())?;
    plugins.retain(|entry| entry.get("name").and_then(Value::as_str) != Some("sanctum"));
    plugins.push(json!({
        "name": "sanctum",
        "source": {"source": "local", "path": "./plugins/sanctum"},
        "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"},
        "category": "Productivity"
    }));

    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("個人用プラグイン一覧を生成できない: {error}"))?;
    bytes.push(b'\n');
    replace_file(path, &bytes)
        .map_err(|error| format!("個人用プラグイン一覧を保存できない: {error}"))
}

fn write_text(path: &Path, content: &str) -> Result<(), String> {
    replace_file(path, content.as_bytes())
        .map_err(|error| format!("{}を保存できない: {error}", path.display()))
}

fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent"))?;
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".sanctum-plugin-{nonce}.partial"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    #[cfg(windows)]
    {
        let backup = parent.join(format!(".sanctum-plugin-{nonce}.backup"));
        if path.exists() {
            fs::rename(path, &backup)?;
        }
        if let Err(error) = fs::rename(&temporary, path) {
            if backup.exists() {
                let _ = fs::rename(&backup, path);
            }
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if backup.exists() {
            fs::remove_file(backup)?;
        }
    }

    #[cfg(not(windows))]
    fs::rename(&temporary, path)?;

    Ok(())
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_sanctum_without_removing_existing_plugins() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("marketplace.json");
        fs::write(
            &path,
            br#"{"name":"personal","plugins":[{"name":"other","source":{"source":"local","path":"./plugins/other"}}]}"#,
        )
        .expect("seed marketplace");

        update_marketplace(&path).expect("update marketplace");
        let document: Value =
            serde_json::from_slice(&fs::read(path).expect("read marketplace"))
                .expect("parse marketplace");
        let plugins = document["plugins"].as_array().expect("plugins");
        assert!(plugins
            .iter()
            .any(|entry| entry["name"] == Value::String("other".into())));
        assert!(plugins.iter().any(valid_sanctum_entry));
    }

    #[test]
    fn refuses_to_replace_invalid_marketplace_json() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("marketplace.json");
        fs::write(&path, b"not-json").expect("seed invalid marketplace");

        assert!(update_marketplace(&path).is_err());
        assert_eq!(fs::read(path).expect("read original"), b"not-json");
    }

    #[test]
    fn encodes_marketplace_path_for_deep_link() {
        assert_eq!(
            percent_encode(r"C:\Users\Keiya\.agents\plugins\marketplace.json"),
            "C%3A%5CUsers%5CKeiya%5C.agents%5Cplugins%5Cmarketplace.json"
        );
    }
}
