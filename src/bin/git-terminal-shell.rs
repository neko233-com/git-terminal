// Copyright 2026 git-next contributors. MIT license.
#[cfg(windows)]
fn cmd_process(script: &std::ffi::OsStr) -> std::process::Command {
    use std::{ffi::OsString, os::windows::process::CommandExt, process::Stdio};
    let shell = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("System32")
    .join("cmd.exe");
    let mut raw = OsString::from("/d /s /c \"");
    raw.push(script);
    raw.push("\"");
    let mut command = std::process::Command::new(shell);
    command
        .raw_arg(raw)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    if let Some(script) = arguments.next() {
        if script == "--version" && arguments.next().is_none() {
            println!("git-terminal-shell {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        #[cfg(windows)]
        if arguments.next().is_none() {
            match cmd_process(&script).status() {
                Ok(status) => std::process::exit(status.code().unwrap_or(1)),
                Err(error) => eprintln!("Git-Terminal CMD: {error}"),
            }
        }
    }
    std::process::exit(1);
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn cmd_quoting_expansion_unicode_path_and_exit_code() {
        use std::{
            os::windows::process::CommandExt,
            time::{SystemTime, UNIX_EPOCH},
        };
        let root = std::env::temp_dir().join(format!(
            "git-terminal-cmd-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let output = root.join("中文 output.txt");
        let source = format!("echo %SYSTEMROOT%>\"{}\" & exit /b 7", output.display());
        let status = super::cmd_process(source.as_ref())
            .creation_flags(0x08000000)
            .status()
            .unwrap();
        let contents = std::fs::read_to_string(&output).unwrap();
        std::fs::remove_file(&output).unwrap();
        std::fs::remove_dir(&root).unwrap();
        assert_eq!(status.code(), Some(7));
        assert_eq!(contents.trim(), std::env::var("SystemRoot").unwrap());
    }
}
