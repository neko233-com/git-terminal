// Copyright 2026 git-next contributors. MIT license.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub font_size: f32,
    pub font_family: String,
    pub scrollback: usize,
    pub shell: String,
    pub syntax_highlighting: bool,
    pub git_prompt: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            font_size: 16.,
            font_family: if cfg!(windows) {
                "Consolas"
            } else {
                "monospace"
            }
            .into(),
            scrollback: 20000,
            shell: "auto".into(),
            syntax_highlighting: true,
            git_prompt: true,
        }
    }
}
impl Settings {
    fn path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            });
        base.join("git-terminal/settings.json")
    }
    pub fn load() -> Self {
        let mut value = std::fs::read(Self::path())
            .ok()
            .and_then(|s| serde_json::from_slice::<Self>(&s).ok())
            .unwrap_or_default();
        value.font_size = value.font_size.clamp(8., 48.);
        value.scrollback = value.scrollback.clamp(1000, 100000);
        value
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!("settings-{}.tmp", std::process::id()));
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            f.write_all(&serde_json::to_vec_pretty(self)?)?;
            f.sync_all()?;
        }
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
                return Err(std::io::Error::last_os_error().into());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(temp, path)?;
        Ok(())
    }
    pub fn chinese(&self) -> bool {
        self.language == "chinese"
            || (self.language == "auto"
                && sys_locale::get_locale()
                    .unwrap_or_default()
                    .to_lowercase()
                    .starts_with("zh"))
    }
}
