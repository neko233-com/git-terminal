// Copyright 2026 git-next contributors. MIT license.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn prompt() -> std::io::Result<bool> {
    use std::io::Write;
    use windows_sys::Win32::{Security::Credentials::*, UI::WindowsAndMessaging::*};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "SSH authentication".into());
    let message = wide(&text);
    let title = wide("Git-Terminal · SSH");
    if std::env::var("SSH_ASKPASS_PROMPT").as_deref() == Ok("confirm") {
        if unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND,
            )
        } == IDYES
        {
            std::io::stdout().write_all(b"yes\n")?;
            return Ok(true);
        }
        return Ok(false);
    }
    let target = wide("Git-Terminal SSH session");
    let info = CREDUI_INFOW {
        cbSize: std::mem::size_of::<CREDUI_INFOW>() as u32,
        hwndParent: std::ptr::null_mut(),
        pszMessageText: message.as_ptr(),
        pszCaptionText: title.as_ptr(),
        hbmBanner: std::ptr::null_mut(),
    };
    let mut user = vec![0u16; 513];
    user[..3].copy_from_slice(&[83, 83, 72]);
    let mut password = vec![0u16; 1025];
    let mut save = 0;
    let status = unsafe {
        CredUIPromptForCredentialsW(
            &info,
            target.as_ptr(),
            std::ptr::null(),
            0,
            user.as_mut_ptr(),
            user.len() as u32,
            password.as_mut_ptr(),
            password.len() as u32,
            &mut save,
            CREDUI_FLAGS_GENERIC_CREDENTIALS
                | CREDUI_FLAGS_ALWAYS_SHOW_UI
                | CREDUI_FLAGS_DO_NOT_PERSIST
                | CREDUI_FLAGS_PASSWORD_ONLY_OK
                | CREDUI_FLAGS_KEEP_USERNAME,
        )
    };
    let result = if status == 0 {
        let end = password
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(password.len());
        let mut bytes = String::from_utf16_lossy(&password[..end]).into_bytes();
        let result = std::io::stdout()
            .write_all(&bytes)
            .and_then(|_| std::io::stdout().write_all(b"\n"));
        for byte in &mut bytes {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        result.map(|_| true)
    } else {
        Ok(false)
    };
    for unit in &mut password {
        unsafe { std::ptr::write_volatile(unit, 0) };
    }
    result
}

fn main() {
    #[cfg(windows)]
    if prompt().unwrap_or(false) {
        return;
    }
    std::process::exit(1);
}
