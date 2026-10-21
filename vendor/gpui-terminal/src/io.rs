//! Ordered PTY input without synchronous pipe writes on the UI thread.
use std::io::Write;
use std::sync::mpsc;

pub(crate) struct InputWriter(mpsc::Sender<Vec<u8>>);

impl InputWriter {
    pub(crate) fn new(mut writer: impl Write + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            while let Ok(bytes) = rx.recv() {
                if writer
                    .write_all(&bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self(tx)
    }

    pub(crate) fn send(&self, bytes: &[u8]) {
        let _ = self.0.send(bytes.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn blocked_pipe_does_not_block_input_and_keeps_order() {
        struct SlowPipe(mpsc::Receiver<()>, mpsc::Sender<Vec<u8>>);
        impl Write for SlowPipe {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.recv().unwrap();
                self.1.send(bytes.to_vec()).unwrap();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (release, gate) = mpsc::channel();
        let (written, output) = mpsc::channel();
        let writer = InputWriter::new(SlowPipe(gate, written));
        writer.send(b"command\r");
        writer.send(b"\x1b[1;1R");
        drop(writer);
        release.send(()).unwrap();
        assert_eq!(
            output.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"command\r"
        );
        release.send(()).unwrap();
        assert_eq!(
            output.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"\x1b[1;1R"
        );
        assert!(output.recv_timeout(Duration::from_secs(1)).is_err());
    }
}
