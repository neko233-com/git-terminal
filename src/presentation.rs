// Copyright 2026 git-next contributors. MIT license.
use std::path::PathBuf;

pub fn profile_name(profile: &str) -> &'static str {
    match profile {
        "bash" => "Bash",
        "powershell" => "PowerShell",
        "cmd" => "CMD",
        _ => "Shell",
    }
}

pub struct TabContext {
    pub cwd: PathBuf,
    pub profile: String,
    pub label: String,
    pub branch: Option<String>,
}

impl TabContext {
    pub fn new(cwd: PathBuf, profile: String) -> Self {
        let cwd = display_directory(cwd);
        let label = cwd
            .file_name()
            .unwrap_or(cwd.as_os_str())
            .to_string_lossy()
            .into_owned();
        Self {
            cwd,
            profile,
            label,
            branch: None,
        }
    }

    pub fn update(&mut self, title: &str) {
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(2048)
            .collect();
        if title.is_empty() || title.to_ascii_lowercase().ends_with(".exe") {
            return;
        }
        let (directory, branch) = title
            .strip_suffix(']')
            .and_then(|title| title.rsplit_once(" ["))
            .map_or((title.as_str(), None), |(path, branch)| {
                (path, Some(branch.to_owned()))
            });
        let path = shell_directory(directory);
        if path.is_absolute() && path.is_dir() {
            self.label = path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned();
            self.cwd = path;
            self.branch = branch;
        } else {
            self.label = title.chars().take(48).collect();
        }
    }
}

fn shell_directory(directory: &str) -> PathBuf {
    #[cfg(windows)]
    {
        let directory = directory.strip_prefix("/cygdrive").unwrap_or(directory);
        let bytes = directory.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            return PathBuf::from(format!("{}:{}", bytes[1] as char, &directory[2..]));
        }
    }
    PathBuf::from(directory)
}

fn display_directory(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(local) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(local);
        }
    }
    path
}

#[cfg(windows)]
pub fn dark_caption() {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        Graphics::Dwm::DwmSetWindowAttribute,
        System::LibraryLoader::GetModuleHandleW,
        System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            EnumThreadWindows, GetSystemMetrics, GetWindowThreadProcessId, IMAGE_ICON, LR_SHARED,
            LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
        },
    };
    unsafe extern "system" fn apply(hwnd: HWND, _: LPARAM) -> i32 {
        let mut process = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut process);
        }
        if process == unsafe { GetCurrentProcessId() } {
            let dark = 1_i32;
            let color = 0x00261c17_u32;
            unsafe {
                DwmSetWindowAttribute(hwnd, 20, (&dark as *const i32).cast(), 4);
                DwmSetWindowAttribute(hwnd, 35, (&color as *const u32).cast(), 4);
                for (kind, width, height) in
                    [(1, SM_CXICON, SM_CYICON), (0, SM_CXSMICON, SM_CYSMICON)]
                {
                    let icon = LoadImageW(
                        GetModuleHandleW(std::ptr::null()),
                        std::ptr::without_provenance::<u16>(1),
                        IMAGE_ICON,
                        GetSystemMetrics(width),
                        GetSystemMetrics(height),
                        LR_SHARED,
                    );
                    if !icon.is_null() {
                        SendMessageW(hwnd, WM_SETICON, kind, icon as isize);
                    }
                }
            }
        }
        1
    }
    // Retain native controls and limit appearance changes to this app's UI thread.
    unsafe {
        EnumThreadWindows(GetCurrentThreadId(), Some(apply), 0);
    }
}

#[cfg(not(windows))]
pub fn dark_caption() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_tracks_directory_and_branch_without_executable_paths() {
        let cwd = std::env::current_dir().unwrap();
        let mut tab = TabContext::new(cwd.clone(), "bash".into());
        let label = tab.label.clone();
        tab.update("C:\\Windows\\System32\\cmd.exe");
        assert_eq!(tab.label, label);
        tab.update(&format!("{} [feature/中文]", cwd.display()));
        assert_eq!(tab.cwd, cwd);
        assert_eq!(tab.branch.as_deref(), Some("feature/中文"));
        tab.update(&cwd.display().to_string());
        assert_eq!(tab.branch, None);
    }

    #[test]
    fn title_is_plain_bounded_text() {
        let mut tab = TabContext::new(std::env::current_dir().unwrap(), "bash".into());
        tab.update("\x1bbuild\noutput\x07");
        assert_eq!(tab.label, "buildoutput");
        tab.update(&"x".repeat(4000));
        assert_eq!(tab.label.len(), 48);
    }
}
