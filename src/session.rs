// Copyright 2026 git-next contributors. MIT license.
use anyhow::{Context, Result, bail};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize)]
pub struct Setup {
    pub product: &'static str,
    pub version: &'static str,
    pub renderer: &'static str,
    pub cwd: PathBuf,
    pub shell: PathBuf,
    pub profile: String,
    pub git: Option<PathBuf>,
    pub no_profile: bool,
    pub integration: Option<PathBuf>,
    pub syntax_highlighting: bool,
    pub git_prompt: bool,
}

fn find_command(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}

#[cfg(windows)]
fn windows_shell_path(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let mut units: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .map(|v| if v == 47 { 92 } else { v })
        .collect();
    if units.starts_with(&[92, 92, 63, 92]) && units.get(5) == Some(&58) {
        units.drain(..4);
    } else if units.starts_with(&[92, 92, 63, 92, 85, 78, 67, 92]) {
        units.splice(..8, [92, 92]);
    }
    PathBuf::from(std::ffi::OsString::from_wide(&units))
}

impl Setup {
    pub fn discover(cwd: &Path, profile: &str, explicit_git: Option<&Path>) -> Result<Self> {
        let cwd = cwd
            .canonicalize()
            .context("Working directory is unavailable")?;
        if !cwd.is_dir() {
            bail!("Working directory must be a directory");
        }
        let executable = std::env::current_exe()?;
        let integration = executable
            .parent()
            .map(|p| p.join("shell-integration"))
            .filter(|p| p.join("bashrc").is_file());
        #[cfg(any(debug_assertions, test))]
        let integration = integration.or_else(|| {
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shell-integration");
            p.join("bashrc").is_file().then_some(p)
        });
        let sibling = executable
            .parent()
            .unwrap_or(Path::new("."))
            .join(if cfg!(windows) { "git.exe" } else { "git" });
        let platform = if cfg!(target_arch = "aarch64") {
            "windows-arm64"
        } else {
            "windows-x64"
        };
        let program_files = std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .unwrap_or_else(|| "C:\\Program Files".into());
        let installed = PathBuf::from(program_files)
            .join("git-next")
            .join(platform)
            .join("bin/git.exe");
        let candidate = explicit_git
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("GIT_NEXT_EXECUTABLE").map(PathBuf::from))
            .or_else(|| sibling.is_file().then_some(sibling))
            .or_else(|| installed.is_file().then_some(installed));
        let git = if let Some(candidate) = candidate {
            let mut identify = std::process::Command::new(&candidate);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                identify.creation_flags(0x08000000);
            }
            let output = identify
                .args(["version", "--json"])
                .output()
                .context("Cannot identify git-next")?;
            let identity: serde_json::Value =
                serde_json::from_slice(&output.stdout).context("git-next identity is invalid")?;
            if !output.status.success() || identity["product"] != "git-next" {
                bail!("The selected Git executable must identify itself as git-next");
            }
            Some(candidate.canonicalize()?)
        } else {
            None
        };
        let bash = git
            .as_ref()
            .and_then(|p| p.parent()?.parent())
            .map(|p| p.join("runtime/usr/bin/bash.exe"))
            .filter(|p| p.is_file());
        let profile = if profile == "auto" {
            if bash.is_some() {
                "bash"
            } else if cfg!(windows) {
                "powershell"
            } else {
                "shell"
            }
        } else {
            profile
        };
        let shell = match profile {
            "bash" => bash
                .or_else(|| {
                    if cfg!(windows) {
                        None
                    } else {
                        find_command("bash")
                    }
                })
                .context("The owned Bash shell is unavailable")?,
            "powershell" => find_command("pwsh.exe")
                .or_else(|| {
                    let p = PathBuf::from(
                        std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
                    )
                    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
                    p.is_file().then_some(p)
                })
                .context("PowerShell is unavailable")?,
            "cmd" if cfg!(windows) => PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
            )
            .join("System32")
            .join("cmd.exe"),
            "shell" if !cfg!(windows) => {
                PathBuf::from(std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into()))
            }
            _ => bail!("Unsupported shell profile: {profile}"),
        };
        Ok(Self {
            product: "git-terminal",
            version: env!("CARGO_PKG_VERSION"),
            renderer: "gpui-fast",
            cwd,
            shell,
            profile: profile.into(),
            git,
            no_profile: false,
            integration,
            syntax_highlighting: true,
            git_prompt: true,
        })
    }

    pub fn command(&self, command: Option<&str>) -> CommandBuilder {
        #[cfg(windows)]
        let shell = if self.profile == "cmd" && command.is_some() {
            std::env::current_exe()
                .unwrap_or_default()
                .with_file_name("git-terminal-shell.exe")
        } else {
            windows_shell_path(&self.shell)
        };
        #[cfg(not(windows))]
        let shell = self.shell.clone();
        let mut cmd = CommandBuilder::new(shell);
        #[cfg(windows)]
        let cwd = if self.profile == "cmd" {
            windows_shell_path(&self.cwd)
        } else {
            self.cwd.clone()
        };
        #[cfg(not(windows))]
        let cwd = self.cwd.clone();
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Git-Terminal");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        cmd.env(
            "GIT_TERMINAL_LOAD_PROFILE",
            if self.no_profile { "0" } else { "1" },
        );
        cmd.env(
            "GIT_TERMINAL_SYNTAX_HIGHLIGHTING",
            if self.syntax_highlighting { "1" } else { "0" },
        );
        cmd.env(
            "GIT_TERMINAL_GIT_PROMPT",
            if self.git_prompt { "1" } else { "0" },
        );
        if let Some(git) = &self.git {
            let bin = git.parent().unwrap();
            let root = bin.parent().unwrap();
            let mut paths = vec![bin.to_path_buf()];
            if cfg!(windows) {
                let prefix = if cfg!(target_arch = "aarch64") {
                    "clangarm64"
                } else {
                    "mingw64"
                };
                paths.push(root.join("runtime").join(prefix).join("bin"));
                paths.push(root.join("runtime/usr/bin"));
                cmd.env(
                    "MSYSTEM",
                    if cfg!(target_arch = "aarch64") {
                        "CLANGARM64"
                    } else {
                        "MINGW64"
                    },
                );
                cmd.env("CHERE_INVOKING", "1");
                cmd.env("MSYS2_PATH_TYPE", "inherit");
                if let Some(home) = std::env::var_os("USERPROFILE") {
                    cmd.env("HOME", home);
                }
            }
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            #[cfg(windows)]
            let paths: Vec<PathBuf> = paths.iter().map(|p| windows_shell_path(p)).collect();
            if let Ok(path) = std::env::join_paths(paths) {
                cmd.env("PATH", path);
            }
            #[cfg(windows)]
            let git = windows_shell_path(git);
            if self.profile == "bash" {
                cmd.env(
                    "GIT_NEXT_EXECUTABLE",
                    git.to_string_lossy().replace('\\', "/"),
                );
            } else {
                cmd.env("GIT_NEXT_EXECUTABLE", git);
            }
        }
        match self.profile.as_str() {
            "bash" => {
                if command.is_none()
                    && let Some(integration) = &self.integration
                {
                    cmd.args(["--noprofile", "--rcfile"]);
                    #[cfg(windows)]
                    let rcfile = windows_shell_path(&integration.join("bashrc"));
                    #[cfg(not(windows))]
                    let rcfile = integration.join("bashrc");
                    cmd.arg(rcfile.to_string_lossy().replace('\\', "/"));
                } else if self.no_profile {
                    cmd.args(["--noprofile", "--norc"]);
                } else {
                    cmd.arg("--login");
                }
                if let Some(command) = command {
                    cmd.arg("-c");
                    cmd.arg(command);
                } else {
                    cmd.arg("-i");
                }
            }
            "powershell" => {
                cmd.arg("-NoLogo");
                if self.no_profile {
                    cmd.arg("-NoProfile");
                }
                if let Some(command) = command {
                    cmd.arg("-Command");
                    cmd.arg(command);
                } else if let Some(integration) = &self.integration {
                    cmd.args(["-ExecutionPolicy", "Bypass", "-NoExit", "-File"]);
                    cmd.arg(integration.join("powershell.ps1"));
                }
            }
            "cmd" => {
                if let Some(command) = command {
                    cmd.arg(command);
                } else {
                    cmd.arg("/d");
                }
            }
            _ => {
                if let Some(command) = command {
                    cmd.arg("-c");
                    cmd.arg(command);
                } else {
                    cmd.arg("-i");
                }
            }
        }
        cmd
    }
}

type SessionIo = (Session, Box<dyn Write + Send>, Box<dyn Read + Send>);

pub struct Session {
    pub master: Arc<parking_lot::Mutex<Option<Box<dyn MasterPty + Send>>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pub exit: Arc<parking_lot::Mutex<Option<u32>>>,
    #[cfg(windows)]
    job: Option<OwnedJob>,
}

impl Session {
    pub fn spawn(setup: &Setup, command: Option<&str>) -> Result<SessionIo> {
        Self::from_command(setup.command(command))
    }

    fn from_command(command: CommandBuilder) -> Result<SessionIo> {
        let pair = native_pty_system().openpty(PtySize {
            rows: 30,
            cols: 110,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut child = pair.slave.spawn_command(command)?;
        #[cfg(windows)]
        let job = match child
            .process_id()
            .context("PTY child identity is missing")
            .and_then(OwnedJob::new)
        {
            Ok(job) => Some(job),
            Err(_) if child.try_wait()?.is_some() => None,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let killer = child.clone_killer();
        let exit = Arc::new(parking_lot::Mutex::new(None));
        let status = exit.clone();
        let ready = Arc::new(parking_lot::Condvar::new());
        let notify_ready = ready.clone();
        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;
        drop(pair.slave);
        let master = Arc::new(parking_lot::Mutex::new(Some(pair.master)));
        let close_master = master.clone();
        std::thread::spawn(move || {
            let result = child.wait();
            *status.lock() = Some(result.map(|s| s.exit_code()).unwrap_or(1));
            notify_ready.notify_all();
            // Close ConPTY off the UI thread so its reader can drain and receive EOF.
            let owned_master = close_master.lock().take();
            drop(owned_master);
        });
        Ok((
            Self {
                master,
                killer,
                exit: exit.clone(),
                #[cfg(windows)]
                job,
            },
            writer,
            Box::new(ExitAwareReader {
                inner: reader,
                status: exit,
                ready,
            }),
        ))
    }
}

struct ExitAwareReader {
    inner: Box<dyn Read + Send>,
    status: Arc<parking_lot::Mutex<Option<u32>>>,
    ready: Arc<parking_lot::Condvar>,
}

impl Read for ExitAwareReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = match self.inner.read(buffer) {
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                ) =>
            {
                0
            }
            Err(error) => return Err(error),
        };
        if count == 0 && !buffer.is_empty() {
            let mut status = self.status.lock();
            while status.is_none() {
                self.ready.wait(&mut status);
            }
        }
        Ok(count)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        if self.exit.lock().is_none() {
            let _ = self.killer.kill();
        }
    }
}

#[cfg(windows)]
struct OwnedJob(usize);
#[cfg(windows)]
impl OwnedJob {
    fn new(pid: u32) -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::{JobObjects::*, Threading::*},
        };
        // The job owns only this newly created PTY shell and its descendants.
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let job = Self(handle as usize);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            let assigned = AssignProcessToJobObject(handle, process);
            let error = std::io::Error::last_os_error();
            CloseHandle(process);
            if assigned == 0 {
                return Err(error.into());
            }
            Ok(job)
        }
    }
}
#[cfg(windows)]
impl Drop for OwnedJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as *mut std::ffi::c_void);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pty_unicode_resize_exit_and_cleanup() {
        let shell = if cfg!(windows) {
            "C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
        } else {
            "/bin/sh"
        };
        let mut command = CommandBuilder::new(shell);
        if cfg!(windows) {
            command.args(["-NoLogo","-NoProfile","-NonInteractive","-Command","[Console]::OutputEncoding=[Text.UTF8Encoding]::new(); Write-Output 'Git-Terminal 中文'; Start-Sleep -Milliseconds 100; exit 7"]);
        } else {
            command.args(["-c", "printf 'Git-Terminal 中文'; exit 7"]);
        }
        let (session, writer, mut reader) = Session::from_command(command).unwrap();
        session
            .master
            .lock()
            .as_ref()
            .unwrap()
            .resize(PtySize {
                rows: 42,
                cols: 132,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = reader.by_ref().take(1024 * 1024).read_to_end(&mut bytes);
            let _ = tx.send(bytes);
        });
        let bytes = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("Git-Terminal 中文"));
        for _ in 0..100 {
            if session.exit.lock().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*session.exit.lock(), Some(7));
        drop(writer);
        drop(session);
    }
}
