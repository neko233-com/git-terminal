// Copyright 2026 git-next contributors. MIT license.
//! OpenSSH owns authentication, host keys, config, agents and jump hosts.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Host {
    pub name: String,
    pub group: String,
    pub destination: String,
    pub user: String,
    pub port: Option<u16>,
    pub identity: String,
    pub config: String,
    pub monitor_interval: u64,
    #[serde(skip)]
    pub system: bool,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            name: String::new(),
            group: String::new(),
            destination: String::new(),
            user: String::new(),
            port: None,
            identity: String::new(),
            config: String::new(),
            monitor_interval: 2,
            system: false,
        }
    }
}

impl Host {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Name is required");
        ensure!(
            !self.destination.is_empty()
                && !self.destination.starts_with('-')
                && !self
                    .destination
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "Invalid SSH host or alias"
        );
        ensure!(
            !self.user.starts_with('-')
                && !self
                    .user
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control()),
            "Invalid SSH user"
        );
        ensure!(self.port != Some(0), "Port must be 1–65535");
        ensure!(
            self.name.len() <= 256 && self.group.len() <= 256 && self.destination.len() <= 1024,
            "Host field is too long"
        );
        Ok(())
    }

    pub fn args(&self) -> Result<Vec<String>> {
        self.validate()?;
        let mut args = vec![
            "-o".into(),
            "ConnectTimeout=8".into(),
            "-o".into(),
            "ServerAliveInterval=15".into(),
            "-o".into(),
            "ServerAliveCountMax=2".into(),
        ];
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string()]);
        }
        for (option, value) in [
            ("-l", &self.user),
            ("-i", &self.identity),
            ("-F", &self.config),
        ] {
            if !value.is_empty() {
                args.extend([option.into(), value.clone()]);
            }
        }
        Ok(args)
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("git-terminal/hosts.json")
}

pub fn load_managed_hosts() -> Result<Vec<Host>> {
    let path = config_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    ensure!(
        std::fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
        "Host configuration is too large"
    );
    let hosts: Vec<Host> = serde_json::from_slice(&std::fs::read(path)?)?;
    for host in &hosts {
        host.validate()?;
    }
    Ok(hosts)
}

pub fn write_hosts(hosts: &[Host]) -> Result<()> {
    for host in hosts {
        host.validate()?;
    }
    let path = config_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(hosts)?)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(temp);
            return Err(error.into());
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn load_hosts() -> Result<Vec<Host>> {
    crate::host_config::load()
}
pub fn save_hosts(hosts: &[Host]) -> Result<()> {
    crate::host_config::save(hosts)
}

pub fn ssh_executable(git: Option<&Path>) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let native =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
                .join("System32/OpenSSH/ssh.exe");
        if native.is_file() {
            return Ok(native);
        }
    }
    if cfg!(windows)
        && let Some(root) = git.and_then(|p| p.parent()?.parent())
    {
        let path = root.join("runtime/usr/bin/ssh.exe");
        if path.is_file() {
            return Ok(path);
        }
    }
    let name = if cfg!(windows) { "ssh.exe" } else { "ssh" };
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .context("OpenSSH is unavailable")
}

pub fn process(host: &Host, ssh: &Path) -> Result<Command> {
    let mut command = Command::new(ssh);
    command.args(host.args()?);
    // Host-key verification remains OpenSSH's normal ask/known_hosts policy.
    let helper = std::env::current_exe()?.with_file_name(if cfg!(windows) {
        "git-terminal-askpass.exe"
    } else {
        "git-terminal-askpass"
    });
    if helper.is_file() {
        command
            .env("SSH_ASKPASS", helper)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env("DISPLAY", "git-terminal");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    Ok(command)
}

pub fn terminal_command(
    host: &Host,
    ssh: &Path,
    cwd: &Path,
) -> Result<portable_pty::CommandBuilder> {
    let mut command = portable_pty::CommandBuilder::new(ssh);
    command.env_clear();
    for (key, value) in std::env::vars_os() {
        command.env(key, value);
    }
    command.args(host.args()?);
    command.args(["-tt", "--", &host.destination]);
    #[cfg(windows)]
    command.cwd(crate::session::windows_shell_path(cwd));
    #[cfg(not(windows))]
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "Git-Terminal");
    Ok(command)
}

pub struct OwnedProcess {
    pub child: Arc<parking_lot::Mutex<Child>>,
    stopped: Arc<AtomicBool>,
    activity: Arc<parking_lot::Mutex<Instant>>,
    #[cfg(windows)]
    _job: Option<crate::session::OwnedJob>,
}

impl OwnedProcess {
    pub fn spawn(mut command: Command, cancel: Arc<AtomicBool>, timeout: Duration) -> Result<Self> {
        ensure!(!cancel.load(Ordering::Acquire), "Operation cancelled");
        let child = Arc::new(parking_lot::Mutex::new(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        ));
        #[cfg(windows)]
        let job = {
            let mut child = child.lock();
            match crate::session::OwnedJob::new(child.id()) {
                Ok(job) => Some(job),
                Err(_) if child.try_wait()?.is_some() => None,
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(parking_lot::Mutex::new(Instant::now()));
        let last_activity = activity.clone();
        let watcher = child.clone();
        let done = stopped.clone();
        std::thread::spawn(move || {
            let mut cancelled_at = None;
            while !done.load(Ordering::Acquire) {
                {
                    let mut child = watcher.lock();
                    if cancel.load(Ordering::Acquire) {
                        cancelled_at.get_or_insert_with(Instant::now);
                    }
                    if cancelled_at.is_some_and(|at| {
                        Instant::now().duration_since(at) >= Duration::from_millis(500)
                    }) || (!timeout.is_zero() && last_activity.lock().elapsed() > timeout)
                    {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    if child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
        Ok(Self {
            child,
            stopped,
            activity,
            #[cfg(windows)]
            _job: job,
        })
    }
}

impl OwnedProcess {
    pub fn touch(&self) {
        *self.activity.lock() = Instant::now();
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let mut child = self.child.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn import_config(path: &Path) -> Result<Vec<Host>> {
    ensure!(
        std::fs::metadata(path)?.len() <= 2 * 1024 * 1024,
        "SSH config is too large"
    );
    let text = std::fs::read_to_string(path)?;
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|s| s.eq_ignore_ascii_case("host")) {
            continue;
        }
        for alias in words {
            if alias.contains(['*', '?', '!', '=']) {
                continue;
            }
            let host = Host {
                name: alias.into(),
                destination: alias.into(),
                group: "SSH config".into(),
                config: path.to_string_lossy().into_owned(),
                ..Host::default()
            };
            if host.validate().is_ok() && !hosts.iter().any(|h: &Host| h.name == host.name) {
                hosts.push(host);
            }
        }
    }
    if hosts.is_empty() {
        bail!("No explicit Host aliases found; wildcard blocks are not connection targets");
    }
    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn destination_cannot_be_an_option_or_control_sequence() {
        for destination in ["-oProxyCommand=bad", "host\ncommand", "host name", ""] {
            assert!(
                Host {
                    name: "test".into(),
                    destination: destination.into(),
                    ..Host::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Host {
                name: "测试".into(),
                destination: "user@[::1]".into(),
                ..Host::default()
            }
            .validate()
            .is_ok()
        );
    }
}
