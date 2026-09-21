use crate::credentials;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub(crate) const MCP_URL: &str = "http://127.0.0.1:43991/mcp";
pub(crate) const PLATFORM_TUNNELS_URL: &str =
    "https://platform.openai.com/settings/organization/tunnels";
pub(crate) const CHATGPT_CONNECTORS_URL: &str = "https://chatgpt.com/#settings/Connectors";

const CONFIG_VERSION: u32 = 1;
const CREDENTIAL_TARGET: &str = "Sanctum/chatgpt-tunnel/runtime-api-key";
const MAX_CLIENT_BYTES: u64 = 250 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TunnelConfig {
    version: u32,
    tunnel_id: String,
    client_path: PathBuf,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatGptTunnelStatus {
    pub configured: bool,
    pub running: bool,
    pub ready: bool,
    pub tunnel_id: Option<String>,
    pub client_path: Option<String>,
    pub has_credential: bool,
    pub admin_ui_url: Option<String>,
    pub mcp_url: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub(crate) struct TunnelManager {
    process: Mutex<Option<Child>>,
    job_handle: Mutex<Option<usize>>,
    operation: Mutex<()>,
    last_error: Mutex<Option<String>>,
}

impl TunnelManager {
    pub(crate) fn status(&self) -> Result<ChatGptTunnelStatus, String> {
        let config = load_config()?;
        let running = self.refresh_process_status();
        let admin_ui_url = if running { read_admin_ui_url() } else { None };
        let ready = admin_ui_url
            .as_deref()
            .is_some_and(admin_endpoint_is_ready);
        let has_credential = credentials::has_password(CREDENTIAL_TARGET);
        let configured = config
            .as_ref()
            .is_some_and(|value| value.client_path.is_file())
            && has_credential;

        Ok(ChatGptTunnelStatus {
            configured,
            running,
            ready,
            tunnel_id: config.as_ref().map(|value| value.tunnel_id.clone()),
            client_path: config
                .as_ref()
                .map(|value| value.client_path.to_string_lossy().into_owned()),
            has_credential,
            admin_ui_url,
            mcp_url: MCP_URL.into(),
            last_error: self
                .last_error
                .lock()
                .expect("tunnel error mutex poisoned")
                .clone(),
        })
    }

    pub(crate) fn configure(
        &self,
        tunnel_id: String,
        runtime_api_key: String,
        client_path: String,
    ) -> Result<ChatGptTunnelStatus, String> {
        let _operation = self.operation.lock().expect("tunnel operation mutex poisoned");
        let tunnel_id = tunnel_id.trim().to_owned();
        validate_tunnel_id(&tunnel_id)?;

        let supplied_key = if runtime_api_key.trim().is_empty() {
            None
        } else {
            let key = Zeroizing::new(runtime_api_key);
            validate_runtime_key(key.as_str())?;
            Some(key)
        };
        if supplied_key.is_none() && !credentials::has_password(CREDENTIAL_TARGET) {
            return Err("Runtime API keyを入力して".into());
        }

        let source = PathBuf::from(client_path);
        validate_client_source(&source)?;
        self.stop_process();
        let managed_client = install_client(&source)?;
        verify_client(&managed_client)?;

        let previous_key = if supplied_key.is_some() {
            credentials::read_password(CREDENTIAL_TARGET).ok()
        } else {
            None
        };
        if let Some(key) = supplied_key.as_ref() {
            credentials::write_password(CREDENTIAL_TARGET, key.as_str())
                .map_err(|error| format!("Runtime API keyをWindowsへ保存できない: {error}"))?;
        }

        let config = TunnelConfig {
            version: CONFIG_VERSION,
            tunnel_id,
            client_path: managed_client,
            enabled: true,
        };
        if let Err(error) = save_config(&config) {
            if supplied_key.is_some() {
                if let Some(previous_key) = previous_key {
                    let _ = credentials::write_password(CREDENTIAL_TARGET, previous_key.as_str());
                } else {
                    let _ = credentials::delete_password(CREDENTIAL_TARGET);
                }
            }
            return Err(error);
        }

        self.start_from_config(&config)?;
        self.status()
    }

    pub(crate) fn start(&self) -> Result<ChatGptTunnelStatus, String> {
        let _operation = self.operation.lock().expect("tunnel operation mutex poisoned");
        if self.refresh_process_status() {
            return self.status();
        }
        let mut config = load_config()?.ok_or_else(|| "先にTunnelを設定して".to_owned())?;
        config.enabled = true;
        save_config(&config)?;
        self.start_from_config(&config)?;
        self.status()
    }

    pub(crate) fn stop(&self) -> Result<ChatGptTunnelStatus, String> {
        let _operation = self.operation.lock().expect("tunnel operation mutex poisoned");
        self.stop_process();
        if let Some(mut config) = load_config()? {
            config.enabled = false;
            save_config(&config)?;
        }
        self.status()
    }

    pub(crate) fn forget(&self) -> Result<ChatGptTunnelStatus, String> {
        let _operation = self.operation.lock().expect("tunnel operation mutex poisoned");
        self.stop_process();
        credentials::delete_password(CREDENTIAL_TARGET)
            .map_err(|error| format!("保存済みのRuntime API keyを削除できない: {error}"))?;
        let path = config_path()?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("Tunnel設定を削除できない: {error}"))?;
        }
        if let Ok(path) = managed_client_path() {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        if let Ok(path) = health_url_path() {
            let _ = fs::remove_file(path);
        }
        *self
            .last_error
            .lock()
            .expect("tunnel error mutex poisoned") = None;
        self.status()
    }

    pub(crate) fn start_if_enabled(&self) {
        let Ok(Some(config)) = load_config() else {
            return;
        };
        if !config.enabled {
            return;
        }
        if let Err(error) = self.start_from_config(&config) {
            *self
                .last_error
                .lock()
                .expect("tunnel error mutex poisoned") = Some(error);
        }
    }

    pub(crate) fn shutdown(&self) {
        self.stop_process();
    }

    fn start_from_config(&self, config: &TunnelConfig) -> Result<(), String> {
        if self.refresh_process_status() {
            return Ok(());
        }
        validate_tunnel_id(&config.tunnel_id)?;
        if !config.client_path.is_file() {
            return Err("tunnel-clientが見つからない。ChatGPT接続から選び直して".into());
        }
        let key = credentials::read_password(CREDENTIAL_TARGET)
            .map_err(|_| "Runtime API keyをWindowsから取得できない。設定し直して".to_owned())?;

        let health_path = health_url_path()?;
        if health_path.exists() {
            let _ = fs::remove_file(&health_path);
        }
        let log_path = tunnel_log_path()?;
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Tunnelログ用フォルダを作れない: {error}"))?;
        }
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&log_path)
            .map_err(|error| format!("Tunnelログを準備できない: {error}"))?;

        let mut command = Command::new(&config.client_path);
        command
            .arg("run")
            .env("CONTROL_PLANE_API_KEY", key.as_str())
            .env("CONTROL_PLANE_TUNNEL_ID", &config.tunnel_id)
            .env("MCP_SERVER_URL", MCP_URL)
            .env("MCP_STARTUP_WAIT_TIMEOUT", "15s")
            .env("HEALTH_LISTEN_ADDR", "127.0.0.1:0")
            .env("HEALTH_URL_FILE", &health_path)
            .env("LOG_LEVEL", "warn")
            .env("LOG_FORMAT", "struct-text")
            .env("LOG_FILE", &log_path)
            .env("NO_PROXY", no_proxy_value())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("tunnel-clientを起動できない: {error}"))?;
        let job_handle = match assign_kill_on_close_job(&child) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        thread::sleep(Duration::from_millis(450));
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("tunnel-clientの状態を確認できない: {error}"))?
        {
            close_job_handle(job_handle);
            let detail = read_log_tail(&log_path).unwrap_or_default();
            let detail = if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            };
            let error = format!("tunnel-clientが終了した（{status}）{detail}");
            *self
                .last_error
                .lock()
                .expect("tunnel error mutex poisoned") = Some(error.clone());
            return Err(error);
        }

        *self.process.lock().expect("tunnel process mutex poisoned") = Some(child);
        *self
            .job_handle
            .lock()
            .expect("tunnel job mutex poisoned") = job_handle;
        *self
            .last_error
            .lock()
            .expect("tunnel error mutex poisoned") = None;
        Ok(())
    }

    fn refresh_process_status(&self) -> bool {
        let mut process = self.process.lock().expect("tunnel process mutex poisoned");
        let Some(child) = process.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                let job_handle = self
                    .job_handle
                    .lock()
                    .expect("tunnel job mutex poisoned")
                    .take();
                close_job_handle(job_handle);
                let detail = tunnel_log_path()
                    .ok()
                    .and_then(|path| read_log_tail(&path).ok())
                    .filter(|value| !value.is_empty())
                    .map(|value| format!(": {value}"))
                    .unwrap_or_default();
                *self
                    .last_error
                    .lock()
                    .expect("tunnel error mutex poisoned") =
                    Some(format!("tunnel-clientが終了した（{status}）{detail}"));
                *process = None;
                false
            }
            Err(error) => {
                *self
                    .last_error
                    .lock()
                    .expect("tunnel error mutex poisoned") =
                    Some(format!("tunnel-clientの状態を確認できない: {error}"));
                false
            }
        }
    }

    fn stop_process(&self) {
        let mut process = self.process.lock().expect("tunnel process mutex poisoned");
        if let Some(mut child) = process.take() {
            let job_handle = self
                .job_handle
                .lock()
                .expect("tunnel job mutex poisoned")
                .take();
            close_job_handle(job_handle);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        if let Ok(job_handle) = self.job_handle.get_mut() {
            close_job_handle(job_handle.take());
        }
        if let Ok(process) = self.process.get_mut() {
            if let Some(mut child) = process.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn validate_tunnel_id(value: &str) -> Result<(), String> {
    let suffix = value
        .strip_prefix("tunnel_")
        .ok_or_else(|| "Tunnel IDは tunnel_ から始まる値を入力して".to_owned())?;
    if suffix.len() != 32
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Tunnel IDの形式が正しくない".into());
    }
    Ok(())
}

fn validate_runtime_key(value: &str) -> Result<(), String> {
    if value.len() < 16 || value.chars().any(char::is_whitespace) {
        return Err("Runtime API keyの形式が正しくない".into());
    }
    Ok(())
}

fn validate_client_source(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path).map_err(|_| "tunnel-client.exeが見つからない".to_owned())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CLIENT_BYTES {
        return Err("選択したtunnel-client.exeを使用できない".into());
    }
    #[cfg(windows)]
    if path.extension().and_then(|value| value.to_str()) != Some("exe") {
        return Err("Windows版のtunnel-client.exeを選んで".into());
    }
    Ok(())
}

fn install_client(source: &Path) -> Result<PathBuf, String> {
    let target = managed_client_path()?;
    if source.canonicalize().ok() == target.canonicalize().ok() && target.is_file() {
        return Ok(target);
    }
    let parent = target
        .parent()
        .ok_or_else(|| "tunnel-clientの保存先が正しくない".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("tunnel-clientの保存先を作れない: {error}"))?;
    let temporary = parent.join(format!(".tunnel-client-{}.partial", nonce()));
    fs::copy(source, &temporary)
        .map_err(|error| format!("tunnel-clientをSanctumへコピーできない: {error}"))?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("tunnel-clientをディスクへ保存できない: {error}"))?;
    replace_path(&temporary, &target)
        .map_err(|error| format!("tunnel-clientを更新できない: {error}"))?;
    Ok(target)
}

fn verify_client(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("tunnel-client.exeを確認できない: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "選択したファイルは対応するtunnel-clientではない（{status}）"
                ));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("tunnel-clientの確認がタイムアウトした".into());
            }
            Err(error) => return Err(format!("tunnel-clientを確認できない: {error}")),
        }
    }
}

fn load_config() -> Result<Option<TunnelConfig>, String> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let config: TunnelConfig = serde_json::from_slice(
        &fs::read(&path).map_err(|error| format!("Tunnel設定を読めない: {error}"))?,
    )
    .map_err(|error| format!("Tunnel設定が壊れている: {error}"))?;
    if config.version != CONFIG_VERSION {
        return Err("Tunnel設定のバージョンに対応していない".into());
    }
    Ok(Some(config))
}

fn save_config(config: &TunnelConfig) -> Result<(), String> {
    let path = config_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| "Tunnel設定の保存先が正しくない".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Tunnel設定用フォルダを作れない: {error}"))?;
    let mut bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| format!("Tunnel設定を生成できない: {error}"))?;
    bytes.push(b'\n');
    let temporary = parent.join(format!(".chatgpt-tunnel-{}.partial", nonce()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("Tunnel設定の一時ファイルを作れない: {error}"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Tunnel設定をディスクへ保存できない: {error}"))?;
    drop(file);
    replace_path(&temporary, &path).map_err(|error| format!("Tunnel設定を保存できない: {error}"))
}

fn replace_path(temporary: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let parent = target
            .parent()
            .ok_or_else(|| std::io::Error::other("path has no parent"))?;
        let backup = parent.join(format!(".sanctum-tunnel-{}.backup", nonce()));
        if target.exists() {
            fs::rename(target, &backup)?;
        }
        if let Err(error) = fs::rename(temporary, target) {
            if backup.exists() {
                let _ = fs::rename(&backup, target);
            }
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        if backup.exists() {
            fs::remove_file(backup)?;
        }
    }

    #[cfg(not(windows))]
    fs::rename(temporary, target)?;

    Ok(())
}

fn read_admin_ui_url() -> Option<String> {
    let contents = fs::read_to_string(health_url_path().ok()?).ok()?;
    let value = contents.trim();
    let without_scheme = value.strip_prefix("http://")?;
    let authority = without_scheme.split('/').next()?;
    if !(authority.starts_with("127.0.0.1:") || authority.starts_with("localhost:")) {
        return None;
    }
    Some(format!("http://{authority}/ui"))
}

fn admin_endpoint_is_ready(admin_ui_url: &str) -> bool {
    let Some(without_scheme) = admin_ui_url.strip_prefix("http://") else {
        return false;
    };
    let Some(authority) = without_scheme.split('/').next() else {
        return false;
    };
    let Ok(address) = authority.parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(350)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(350)));
    if stream
        .write_all(
            format!(
                "GET /readyz HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .is_err()
    {
        return false;
    }
    let mut response = [0_u8; 64];
    let Ok(count) = stream.read(&mut response) else {
        return false;
    };
    std::str::from_utf8(&response[..count])
        .ok()
        .is_some_and(|value| value.starts_with("HTTP/1.1 200") || value.starts_with("HTTP/1.0 200"))
}

fn read_log_tail(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let start = bytes.len().saturating_sub(4_000);
    Ok(String::from_utf8_lossy(&bytes[start..])
        .lines()
        .last()
        .unwrap_or_default()
        .chars()
        .take(600)
        .collect())
}

fn no_proxy_value() -> String {
    let existing = std::env::var("NO_PROXY").unwrap_or_default();
    if existing.is_empty() {
        "127.0.0.1,localhost".into()
    } else {
        format!("{existing},127.0.0.1,localhost")
    }
}

fn data_root() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        });

    root.map(|path| path.join("Sanctum"))
        .ok_or_else(|| "Sanctumのローカル保存先を特定できない".to_owned())
}

fn config_path() -> Result<PathBuf, String> {
    Ok(data_root()?.join("chatgpt-tunnel.json"))
}

fn managed_client_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let name = "tunnel-client.exe";
    #[cfg(not(windows))]
    let name = "tunnel-client";
    Ok(data_root()?.join("bin").join(name))
}

fn health_url_path() -> Result<PathBuf, String> {
    Ok(data_root()?.join("tunnel-health.url"))
}

fn tunnel_log_path() -> Result<PathBuf, String> {
    Ok(data_root()?.join("logs").join("tunnel-client.log"))
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(windows)]
fn assign_kill_on_close_job(child: &Child) -> Result<Option<usize>, String> {
    use std::mem;
    use std::os::windows::io::AsRawHandle;
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "Tunnelの安全なプロセス境界を作れない: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const information).cast(),
            u32::try_from(mem::size_of_val(&information)).expect("job information fits in u32"),
        )
    };
    if configured == 0 {
        let error = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(format!("Tunnelの終了保護を設定できない: {error}"));
    }
    let process_handle = child.as_raw_handle() as HANDLE;
    if unsafe { AssignProcessToJobObject(job, process_handle) } == 0 {
        let error = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(format!("Tunnelを安全なプロセス境界へ追加できない: {error}"));
    }
    Ok(Some(job as usize))
}

#[cfg(not(windows))]
fn assign_kill_on_close_job(_child: &Child) -> Result<Option<usize>, String> {
    Ok(None)
}

#[cfg(windows)]
fn close_job_handle(handle: Option<usize>) {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    if let Some(handle) = handle {
        unsafe { CloseHandle(handle as HANDLE) };
    }
}

#[cfg(not(windows))]
fn close_job_handle(_handle: Option<usize>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_tunnel_ids() {
        assert!(validate_tunnel_id("tunnel_0123456789abcdef0123456789abcdef").is_ok());
        assert!(validate_tunnel_id("0123456789abcdef0123456789abcdef").is_err());
        assert!(validate_tunnel_id("tunnel_0123456789ABCDEF0123456789ABCDEF").is_err());
        assert!(validate_tunnel_id("tunnel_short").is_err());
    }

    #[test]
    fn serialized_config_never_contains_a_runtime_key() {
        let config = TunnelConfig {
            version: CONFIG_VERSION,
            tunnel_id: "tunnel_0123456789abcdef0123456789abcdef".into(),
            client_path: PathBuf::from("tunnel-client.exe"),
            enabled: true,
        };
        let encoded = serde_json::to_string(&config).expect("serialize tunnel config");
        assert!(encoded.contains("tunnel_0123456789abcdef0123456789abcdef"));
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("runtime_api_key"));
    }

    #[test]
    fn rejects_empty_or_oversized_client_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let empty = directory.path().join("tunnel-client");
        fs::write(&empty, []).expect("empty file");
        assert!(validate_client_source(&empty).is_err());
        assert!(validate_client_source(directory.path()).is_err());
    }
}
