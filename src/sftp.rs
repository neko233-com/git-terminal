// Copyright 2026 git-next contributors. MIT license.
//! Bounded SFTP v3 framing over an owned OpenSSH subsystem; no shell filename parsing.
use crate::remote::{self, Host, OwnedProcess};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::{ChildStdin, ChildStdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 1024 * 1024;
const CHUNK: usize = 64 * 1024;
const WINDOW: usize = 16;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub permissions: u32,
    pub modified: u32,
}
impl Entry {
    pub fn directory(&self) -> bool {
        self.permissions & 0o170000 == 0o040000
    }
    pub fn symlink(&self) -> bool {
        self.permissions & 0o170000 == 0o120000
    }
}

pub struct Sftp {
    _process: OwnedProcess,
    input: ChildStdin,
    output: ChildStdout,
    id: u32,
    cancel: Arc<AtomicBool>,
    posix_rename: bool,
    cleaning_up: bool,
}

fn number(v: u32, bytes: &mut Vec<u8>) {
    bytes.extend(v.to_be_bytes());
}
fn string(v: &[u8], bytes: &mut Vec<u8>) {
    number(v.len() as u32, bytes);
    bytes.extend(v);
}

struct Packet {
    bytes: Vec<u8>,
    at: usize,
}
impl Packet {
    fn byte(&mut self) -> Result<u8> {
        let value = *self.bytes.get(self.at).context("Truncated SFTP packet")?;
        self.at += 1;
        Ok(value)
    }
    fn u32(&mut self) -> Result<u32> {
        let end = self.at.checked_add(4).context("SFTP integer overflow")?;
        let value = u32::from_be_bytes(
            self.bytes
                .get(self.at..end)
                .context("Truncated SFTP integer")?
                .try_into()?,
        );
        self.at = end;
        Ok(value)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(((self.u32()? as u64) << 32) | self.u32()? as u64)
    }
    fn string(&mut self) -> Result<Vec<u8>> {
        let len = self.u32()? as usize;
        let end = self.at.checked_add(len).context("SFTP string overflow")?;
        let value = self
            .bytes
            .get(self.at..end)
            .context("Truncated SFTP string")?
            .to_vec();
        self.at = end;
        Ok(value)
    }
    fn text(&mut self) -> Result<String> {
        String::from_utf8(self.string()?).context("Non-UTF-8 remote filename")
    }
    fn attrs(&mut self, name: String) -> Result<Entry> {
        let flags = self.u32()?;
        ensure!(flags & !0x8000000f == 0, "Unsupported SFTP attribute flags");
        let size = if flags & 1 != 0 { self.u64()? } else { 0 };
        if flags & 2 != 0 {
            self.u32()?;
            self.u32()?;
        }
        let permissions = if flags & 4 != 0 { self.u32()? } else { 0 };
        let modified = if flags & 8 != 0 {
            self.u32()?;
            self.u32()?
        } else {
            0
        };
        if flags & 0x80000000 != 0 {
            let count = self.u32()?;
            ensure!(count <= 1024, "Too many SFTP attributes");
            for _ in 0..count {
                self.string()?;
                self.string()?;
            }
        }
        Ok(Entry {
            name,
            size,
            permissions,
            modified,
        })
    }
}

impl Sftp {
    pub fn connect(host: &Host, ssh: &Path, cancel: Arc<AtomicBool>) -> Result<Self> {
        let mut command = remote::process(host, ssh)?;
        command.args(["-T", "-s", "--", &host.destination, "sftp"]);
        let process = OwnedProcess::spawn(command, cancel.clone(), Duration::from_secs(60))?;
        let (input, output) = {
            let mut child = process.child.lock();
            (child.stdin.take().unwrap(), child.stdout.take().unwrap())
        };
        let mut client = Self {
            _process: process,
            input,
            output,
            id: 0,
            cancel,
            posix_rename: false,
            cleaning_up: false,
        };
        client.send(&[1, 0, 0, 0, 3])?;
        let mut reply = client.receive()?;
        ensure!(
            reply.byte()? == 2 && reply.u32()? == 3,
            "SFTP version 3 is required"
        );
        while reply.at < reply.bytes.len() {
            let name = reply.text()?;
            let version = reply.text()?;
            client.posix_rename |= name == "posix-rename@openssh.com" && version == "1";
        }
        Ok(client)
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            self.cleaning_up || !self.cancel.load(Ordering::Acquire),
            "Transfer cancelled"
        );
        ensure!(bytes.len() <= LIMIT, "SFTP packet is too large");
        self._process.touch();
        self.input.write_all(&(bytes.len() as u32).to_be_bytes())?;
        self.input.write_all(bytes)?;
        self.input.flush()?;
        Ok(())
    }
    fn receive(&mut self) -> Result<Packet> {
        self._process.touch();
        let mut header = [0; 4];
        self.output
            .read_exact(&mut header)
            .context("SSH/SFTP connection ended; check authentication, host key and subsystem")?;
        let size = u32::from_be_bytes(header) as usize;
        ensure!((1..=LIMIT).contains(&size), "Invalid SFTP packet size");
        let mut bytes = vec![0; size];
        self.output.read_exact(&mut bytes)?;
        Ok(Packet { bytes, at: 0 })
    }
    fn begin(&mut self, kind: u8, payload: Vec<u8>) -> Result<u32> {
        self.id = self
            .id
            .checked_add(1)
            .context("SFTP request limit reached")?;
        let mut bytes = vec![kind];
        number(self.id, &mut bytes);
        bytes.extend(payload);
        self.send(&bytes)?;
        Ok(self.id)
    }
    fn response(&mut self) -> Result<(u8, u32, Packet)> {
        let mut reply = self.receive()?;
        let kind = reply.byte()?;
        let id = reply.u32()?;
        Ok((kind, id, reply))
    }
    fn request(&mut self, kind: u8, payload: Vec<u8>) -> Result<Packet> {
        let expected = self.begin(kind, payload)?;
        let (kind, id, mut reply) = self.response()?;
        ensure!(id == expected, "Mismatched SFTP response ID");
        if kind == 101 {
            let status = reply.u32()?;
            if status != 0 && status != 1 {
                bail!("SFTP operation failed (status {status})");
            }
            reply.at = 5;
        }
        Ok(reply)
    }
    fn path_request(&mut self, kind: u8, path: &str) -> Result<Packet> {
        ensure!(!path.contains('\0'), "Invalid remote path");
        let mut bytes = Vec::new();
        string(path.as_bytes(), &mut bytes);
        self.request(kind, bytes)
    }
    fn close(&mut self, handle: &[u8]) -> Result<()> {
        let mut bytes = Vec::new();
        string(handle, &mut bytes);
        let mut reply = self.request(4, bytes)?;
        ensure!(
            reply.bytes[0] == 101 && reply.u32()? == 0,
            "SFTP close failed"
        );
        Ok(())
    }
    pub fn home(&mut self) -> Result<String> {
        let mut reply = self.path_request(16, ".")?;
        ensure!(
            reply.bytes[0] == 104 && reply.u32()? == 1,
            "SFTP realpath failed"
        );
        reply.text()
    }
    pub fn stat(&mut self, path: &str) -> Result<Entry> {
        let mut reply = self.path_request(7, path)?;
        ensure!(reply.bytes[0] == 105, "SFTP stat failed");
        reply.attrs(path.into())
    }
    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let mut reply = self.path_request(11, path)?;
        ensure!(reply.bytes[0] == 102, "SFTP directory open failed");
        let handle = reply.string()?;
        let mut entries = Vec::new();
        let result = (|| -> Result<()> {
            loop {
                let mut bytes = Vec::new();
                string(&handle, &mut bytes);
                let mut reply = self.request(12, bytes)?;
                if reply.bytes[0] == 101 {
                    ensure!(reply.u32()? == 1, "SFTP directory read failed");
                    break;
                }
                ensure!(reply.bytes[0] == 104, "Invalid SFTP directory response");
                let count = reply.u32()?;
                ensure!(count <= 16384, "Oversized SFTP directory batch");
                for _ in 0..count {
                    let name = reply.text()?;
                    reply.string()?;
                    let entry = reply.attrs(name)?;
                    if entry.name == "." || entry.name == ".." {
                        continue;
                    }
                    ensure!(component(&entry.name), "Unsafe remote filename");
                    entries.push(entry);
                    ensure!(entries.len() <= 100000, "Directory exceeds 100000 entries");
                }
            }
            Ok(())
        })();
        let closed = self.close(&handle);
        result?;
        closed?;
        entries.sort_by(|a, b| {
            b.directory()
                .cmp(&a.directory())
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }
    fn open(&mut self, path: &str, flags: u32) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        string(path.as_bytes(), &mut bytes);
        number(flags, &mut bytes);
        number(0, &mut bytes);
        let mut reply = self.request(3, bytes)?;
        ensure!(reply.bytes[0] == 102, "SFTP file open failed");
        reply.string()
    }
    pub fn download(
        &mut self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<()> {
        self.download_at(remote, local, overwrite, progress, 0)
    }
    fn download_at(
        &mut self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        progress: &mut dyn FnMut(u64),
        depth: usize,
    ) -> Result<()> {
        ensure!(depth <= 128, "Transfer tree exceeds 128 directory levels");
        if let Ok(meta) = std::fs::symlink_metadata(local) {
            ensure!(
                !meta.is_symlink(),
                "Local symbolic links are not followed during transfer"
            );
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                ensure!(
                    meta.file_attributes() & 0x400 == 0,
                    "Local reparse points are not followed during transfer"
                );
            }
        }
        ensure!(overwrite || !local.exists(), "Destination already exists");
        let attr = self.stat(remote)?;
        ensure!(
            !attr.symlink(),
            "Symbolic links are not followed during transfer"
        );
        if attr.directory() {
            std::fs::create_dir_all(local)?;
            for entry in self.list(remote)? {
                ensure!(
                    local_component(&entry.name),
                    "Remote name cannot be represented safely on this platform"
                );
                self.download_at(
                    &join(remote, &entry.name),
                    &local.join(entry.name),
                    overwrite,
                    progress,
                    depth + 1,
                )?;
            }
            return Ok(());
        }
        let temp = local.with_file_name(format!(".git-terminal-{}.part", unique()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let handle = match self.open(remote, 1) {
            Ok(handle) => handle,
            Err(error) => {
                drop(file);
                let _ = std::fs::remove_file(temp);
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            let mut offset = 0u64;
            let mut transferred = 0u64;
            while offset < attr.size {
                let mut pending = std::collections::BTreeMap::new();
                for _ in 0..WINDOW {
                    if offset >= attr.size {
                        break;
                    }
                    let length = (attr.size - offset).min(CHUNK as u64) as u32;
                    let mut bytes = Vec::new();
                    string(&handle, &mut bytes);
                    bytes.extend(offset.to_be_bytes());
                    number(length, &mut bytes);
                    pending.insert(self.begin(5, bytes)?, (offset, length));
                    offset += u64::from(length);
                }
                while !pending.is_empty() {
                    let (kind, id, mut reply) = self.response()?;
                    let (position, expected) =
                        pending.remove(&id).context("Unexpected SFTP read ID")?;
                    ensure!(kind == 103, "SFTP read failed or remote file changed");
                    let data = reply.string()?;
                    ensure!(
                        !data.is_empty() && data.len() <= expected as usize,
                        "Invalid SFTP data size"
                    );
                    file.seek(SeekFrom::Start(position))?;
                    file.write_all(&data)?;
                    transferred += data.len() as u64;
                    progress(data.len() as u64);
                    if data.len() < expected as usize {
                        let mut bytes = Vec::new();
                        string(&handle, &mut bytes);
                        let position = position + data.len() as u64;
                        let remaining = expected - data.len() as u32;
                        bytes.extend(position.to_be_bytes());
                        number(remaining, &mut bytes);
                        pending.insert(self.begin(5, bytes)?, (position, remaining));
                    }
                }
            }
            ensure!(
                transferred == attr.size && self.stat(remote)?.size == attr.size,
                "Remote file changed size during download"
            );
            file.sync_all()?;
            self.close(&handle)?;
            Ok(())
        })();
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
            return result;
        }
        let result = install_download(&temp, local, overwrite);
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
    pub fn upload(
        &mut self,
        local: &Path,
        remote: &str,
        overwrite: bool,
        progress: &mut dyn FnMut(u64),
    ) -> Result<()> {
        self.upload_at(local, remote, overwrite, progress, 0)
    }
    fn upload_at(
        &mut self,
        local: &Path,
        remote: &str,
        overwrite: bool,
        progress: &mut dyn FnMut(u64),
        depth: usize,
    ) -> Result<()> {
        ensure!(depth <= 128, "Transfer tree exceeds 128 directory levels");
        let meta = std::fs::symlink_metadata(local)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                meta.file_attributes() & 0x400 == 0,
                "Local reparse points are not followed during transfer"
            );
        }
        ensure!(
            !meta.is_symlink(),
            "Symbolic links are not followed during transfer"
        );
        if meta.is_dir() {
            let mut bytes = Vec::new();
            string(remote.as_bytes(), &mut bytes);
            number(0, &mut bytes);
            if self.request(14, bytes).is_err() {
                ensure!(
                    self.stat(remote)?.directory(),
                    "Remote target is not a directory"
                );
            }
            for entry in std::fs::read_dir(local)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("Non-UTF-8 local filename"))?;
                ensure!(component(&name), "Unsafe filename");
                self.upload_at(
                    &entry.path(),
                    &join(remote, &name),
                    overwrite,
                    progress,
                    depth + 1,
                )?;
            }
            return Ok(());
        }
        ensure!(
            meta.is_file(),
            "Only regular files and directories can be uploaded"
        );
        let temp = format!("{remote}.git-terminal-{}.part", unique());
        let handle = self.open(&temp, 2 | 8 | 32)?;
        let result = (|| -> Result<()> {
            let mut file = File::open(local)?;
            let mut offset = 0u64;
            let mut buffer = vec![0; CHUNK];
            let mut finished = false;
            while !finished {
                let mut pending = std::collections::BTreeMap::new();
                for _ in 0..WINDOW {
                    let count = file.read(&mut buffer)?;
                    if count == 0 {
                        finished = true;
                        break;
                    }
                    let mut bytes = Vec::new();
                    string(&handle, &mut bytes);
                    bytes.extend(offset.to_be_bytes());
                    string(&buffer[..count], &mut bytes);
                    pending.insert(self.begin(6, bytes)?, count as u64);
                    offset += count as u64;
                }
                let mut failed = false;
                while !pending.is_empty() {
                    let (kind, id, mut reply) = self.response()?;
                    let count = pending.remove(&id).context("Unexpected SFTP write ID")?;
                    if kind != 101 || reply.u32()? != 0 {
                        failed = true;
                    } else {
                        progress(count);
                    }
                }
                ensure!(!failed, "SFTP write failed");
            }
            ensure!(
                offset == meta.len(),
                "Local file changed size during upload"
            );
            self.close(&handle)?;
            let mut bytes = Vec::new();
            let extended = overwrite && self.posix_rename;
            if overwrite {
                ensure!(extended, "Server does not support atomic replacement");
            }
            if extended {
                string(b"posix-rename@openssh.com", &mut bytes);
            }
            string(temp.as_bytes(), &mut bytes);
            string(remote.as_bytes(), &mut bytes);
            let mut reply = self.request(if extended { 200 } else { 18 }, bytes)?;
            ensure!(
                reply.bytes[0] == 101 && reply.u32()? == 0,
                "SFTP rename failed; destination may already exist"
            );
            Ok(())
        })();
        if result.is_err() {
            self.cleaning_up = true;
            let _ = (|| -> Result<()> {
                let mut bytes = Vec::new();
                string(&handle, &mut bytes);
                let close = self.begin(4, bytes)?;
                let mut closed = false;
                for _ in 0..=WINDOW {
                    if self.response()?.1 == close {
                        closed = true;
                        break;
                    }
                }
                ensure!(closed, "SFTP cleanup could not drain pending replies");
                self.path_request(13, &temp)?;
                Ok(())
            })();
            self.cleaning_up = false;
        }
        result
    }
}

pub fn join(directory: &str, name: &str) -> String {
    format!("{}/{name}", directory.trim_end_matches('/'))
}
fn component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
fn local_component(name: &str) -> bool {
    if !component(name) {
        return false;
    }
    if cfg!(windows) {
        let stem = name.split('.').next().unwrap_or("").to_uppercase();
        return !name.contains(['<', '>', ':', '"', '|', '?', '*'])
            && !name.ends_with(['.', ' '])
            && ![
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&stem.as_str())
            && !name.chars().any(char::is_control);
    }
    true
}
fn unique() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
fn install_download(temp: &Path, target: &Path, overwrite: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let flags = if overwrite {
            windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
        ensure!(
            unsafe {
                windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    flags,
                )
            } != 0,
            "Cannot install downloaded file: {}",
            std::io::Error::last_os_error()
        );
    }
    #[cfg(not(windows))]
    {
        if overwrite {
            std::fs::rename(temp, target)?;
        } else {
            std::fs::hard_link(temp, target)?;
            std::fs::remove_file(temp)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires the disposable SSH fixture and isolated transfer directory"]
    fn cancellation_preserves_destinations_and_cleans_partial_files() {
        let root =
            std::path::PathBuf::from(std::env::var_os("GIT_TERMINAL_TEST_TRANSFER_ROOT").unwrap());
        let config = std::env::var("GIT_TERMINAL_TEST_SSH_CONFIG").unwrap();
        let host = Host {
            name: "fixture".into(),
            destination: "fixture".into(),
            config,
            ..Host::default()
        };
        let ssh = remote::ssh_executable(None).unwrap();
        let connect = |cancel| Sftp::connect(&host, &ssh, cancel).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("original.bin");
        std::fs::write(&original, b"original").unwrap();
        let large = root.join("large.bin");
        let file = File::create(&large).unwrap();
        file.set_len(16 * 1024 * 1024).unwrap();
        drop(file);
        let target = "/home/terminal/cancel-protected.bin";
        connect(Arc::new(AtomicBool::new(false)))
            .upload(&original, target, true, &mut |_| {})
            .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut client = connect(cancel.clone());
        let started = std::time::Instant::now();
        assert!(
            client
                .upload(&large, target, true, &mut |_| cancel
                    .store(true, Ordering::Release))
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(client);
        let mut client = connect(Arc::new(AtomicBool::new(false)));
        assert!(!client.list("/home/terminal").unwrap().iter().any(|entry| {
            entry.name.starts_with("cancel-protected.bin.git-terminal-")
                && entry.name.ends_with(".part")
        }));
        let readback = root.join("readback.bin");
        client
            .download(target, &readback, false, &mut |_| {})
            .unwrap();
        assert_eq!(std::fs::read(readback).unwrap(), b"original");
        client
            .upload(
                &large,
                "/home/terminal/cancel-source.bin",
                true,
                &mut |_| {},
            )
            .unwrap();
        drop(client);
        let local = root.join("preserved.bin");
        std::fs::write(&local, b"local-original").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut client = connect(cancel.clone());
        assert!(
            client
                .download(
                    "/home/terminal/cancel-source.bin",
                    &local,
                    true,
                    &mut |_| cancel.store(true, Ordering::Release)
                )
                .is_err()
        );
        assert_eq!(std::fs::read(local).unwrap(), b"local-original");
        assert!(!std::fs::read_dir(root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")
        }));
    }
    #[test]
    fn remote_names_cannot_escape_local_destination() {
        for name in ["../a", "/a", "..", "a\\b", "a\0b"] {
            assert!(!component(name));
        }
        assert!(component("中文 file.txt"));
        if cfg!(windows) {
            for name in ["NUL.txt", "file:secret", "tail."] {
                assert!(!local_component(name));
            }
        }
    }
    #[test]
    fn truncated_and_oversized_fields_fail_before_allocation() {
        let mut packet = Packet {
            bytes: vec![255, 255, 255, 255],
            at: 0,
        };
        assert!(packet.string().is_err());
        let mut packet = Packet {
            bytes: vec![0, 0],
            at: 0,
        };
        assert!(packet.u32().is_err());
    }
}
