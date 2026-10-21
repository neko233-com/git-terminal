use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/git-terminal.ico");
    let target = env::var("TARGET").unwrap();
    if !target.contains("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = root
        .join("assets/git-terminal.ico")
        .to_string_lossy()
        .replace('\\', "\\\\");
    let script = out.join("git-terminal.rc");
    fs::write(
        &script,
        format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION 0,1,0,0
PRODUCTVERSION 0,1,0,0
FILEOS 0x40004L
FILETYPE 1
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "CompanyName", "neko233-com\0"
   VALUE "FileDescription", "Git-Terminal\0"
   VALUE "ProductName", "Git-Terminal\0"
   VALUE "FileVersion", "0.1.0\0"
   VALUE "ProductVersion", "0.1.0\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
"#
        ),
    )
    .unwrap();
    let resource = out.join(if target.contains("msvc") {
        "git-terminal.res"
    } else {
        "git-terminal-resource.o"
    });
    let status = if target.contains("msvc") {
        let compiler = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
            let base = PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap())
                .join("Windows Kits/10/bin");
            let mut versions: Vec<_> = fs::read_dir(base)
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("x64/rc.exe").is_file())
                .collect();
            versions.sort();
            versions
                .pop()
                .expect("Windows SDK resource compiler is required")
                .join("x64/rc.exe")
        });
        Command::new(compiler)
            .arg("/nologo")
            .arg("/fo")
            .arg(&resource)
            .arg(&script)
            .status()
    } else {
        let compiler = env::var_os("WINDRES")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let cc = PathBuf::from(
                    env::var_os("CC_aarch64_pc_windows_gnullvm")
                        .expect("The pinned ARM64 compiler is required"),
                );
                cc.parent().unwrap().join("aarch64-w64-mingw32-windres.exe")
            });
        Command::new(compiler)
            .arg("-i")
            .arg(&script)
            .arg("-o")
            .arg(&resource)
            .args(["-O", "coff"])
            .status()
    }
    .expect("Resource compiler could not start");
    assert!(
        status.success(),
        "Git-Terminal icon resource compilation failed"
    );
    println!(
        "cargo:rustc-link-arg-bin=git-terminal={}",
        resource.display()
    );
}
