// Copyright 2026 git-next contributors. MIT license.
use std::io::{Read, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

// ConPTY requests this protocol at startup. Its legacy UTF-8 decoder can lose
// supplementary characters; send their UTF-16 records without changing VT keys.
const REQUEST: &[u8] = b"\x1b[?9001h";

pub fn wrap(
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
    let enabled = Arc::new(AtomicBool::new(false));
    (
        Box::new(Probe {
            inner: reader,
            enabled: enabled.clone(),
            matched: 0,
            done: false,
        }),
        Box::new(Input {
            inner: writer,
            enabled,
            pending: Vec::new(),
        }),
    )
}

struct Probe {
    inner: Box<dyn Read + Send>,
    enabled: Arc<AtomicBool>,
    matched: usize,
    done: bool,
}
impl Read for Probe {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(bytes)?;
        if !self.done {
            for &byte in &bytes[..count] {
                if byte != REQUEST[self.matched] {
                    self.done = true;
                    break;
                }
                self.matched += 1;
                if self.matched == REQUEST.len() {
                    self.enabled.store(true, Ordering::Release);
                    self.done = true;
                    break;
                }
            }
        }
        Ok(count)
    }
}

struct Input {
    inner: Box<dyn Write + Send>,
    enabled: Arc<AtomicBool>,
    pending: Vec<u8>,
}
impl Write for Input {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.enabled.load(Ordering::Acquire) {
            return self.inner.write(bytes);
        }
        if self.pending.is_empty() && !bytes.iter().any(|b| (0xf0..=0xf4).contains(b)) {
            return self.inner.write(bytes);
        }
        self.pending.extend_from_slice(bytes);
        let mut output = Vec::with_capacity(self.pending.len());
        let mut offset = 0;
        while offset < self.pending.len() {
            let rest = &self.pending[offset..];
            if (0xf0..=0xf4).contains(&rest[0]) {
                if rest.len() < 4 {
                    break;
                }
                if let Ok(text) = std::str::from_utf8(&rest[..4]) {
                    let mut units = [0; 2];
                    for unit in text.chars().next().unwrap().encode_utf16(&mut units) {
                        output.extend_from_slice(format!("\x1b[0;0;{unit};1;0;1_").as_bytes());
                    }
                    offset += 4;
                    continue;
                }
            }
            output.push(rest[0]);
            offset += 1;
        }
        self.inner.write_all(&output)?;
        self.pending.drain(..offset);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.write_all(&self.pending)?;
        self.pending.clear();
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn negotiated_unicode_preserves_split_scalars_and_vt() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let (mut reader, mut writer) = wrap(
            Box::new(std::io::Cursor::new(REQUEST)),
            Box::new(Capture(output.clone())),
        );
        let mut byte = [0];
        for _ in REQUEST {
            reader.read_exact(&mut byte).unwrap();
        }
        let input = "中文🙂𠀀\x1b[A".as_bytes();
        for byte in input {
            writer.write_all(&[*byte]).unwrap();
        }
        writer.flush().unwrap();
        assert_eq!(*output.lock().unwrap(), "中文\x1b[0;0;55357;1;0;1_\x1b[0;0;56898;1;0;1_\x1b[0;0;55360;1;0;1_\x1b[0;0;56320;1;0;1_\x1b[A".as_bytes());
    }
    #[test]
    fn legacy_host_retains_utf8() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let (mut reader, mut writer) = wrap(
            Box::new(std::io::Cursor::new(b"ordinary output")),
            Box::new(Capture(output.clone())),
        );
        reader.read_exact(&mut [0]).unwrap();
        writer.write_all("🙂\x1b[A".as_bytes()).unwrap();
        assert_eq!(*output.lock().unwrap(), "🙂\x1b[A".as_bytes());
    }
}
