// Copyright 2026 git-next contributors. MIT license.
//! A focused terminal exists before Windows creates its console or shell.
use crate::session::{Session, SessionIo};
use anyhow::Result;
use std::{
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

pub struct SessionTask {
    pub resize: mpsc::Sender<portable_pty::PtySize>,
    pub exit: Arc<parking_lot::Mutex<Option<u32>>>,
    pub starting: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    owned: Arc<parking_lot::Mutex<Option<Session>>>,
}

struct PendingReader {
    ready: mpsc::Receiver<(Box<dyn Read + Send>, Arc<parking_lot::Mutex<Option<u32>>>)>,
    inner: Option<Box<dyn Read + Send>>,
    status: Option<Arc<parking_lot::Mutex<Option<u32>>>>,
    exit: Arc<parking_lot::Mutex<Option<u32>>>,
}
impl Read for PendingReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.inner.is_none() {
            let Ok((reader, status)) = self.ready.recv() else {
                return Ok(0);
            };
            self.inner = Some(reader);
            self.status = Some(status);
        }
        let count = self.inner.as_mut().unwrap().read(bytes)?;
        if count == 0 {
            *self.exit.lock() = *self.status.as_ref().unwrap().lock();
        }
        Ok(count)
    }
}
struct PendingWriter {
    ready: mpsc::Receiver<Box<dyn Write + Send>>,
    inner: Option<Box<dyn Write + Send>>,
}
impl Write for PendingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.inner.is_none() {
            self.inner = Some(self.ready.recv().map_err(|_| io::ErrorKind::BrokenPipe)?);
        }
        self.inner.as_mut().unwrap().write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.as_mut().map_or(Ok(()), |writer| writer.flush())
    }
}
impl SessionTask {
    pub fn start(
        start: impl FnOnce() -> Result<SessionIo> + Send + 'static,
    ) -> (
        Self,
        impl Write + Send + 'static,
        impl Read + Send + 'static,
    ) {
        let owned = Arc::new(parking_lot::Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        let starting = Arc::new(AtomicBool::new(true));
        let exit = Arc::new(parking_lot::Mutex::new(None));
        let (resize, changes) = mpsc::channel();
        let (input_tx, input_rx) = mpsc::channel();
        let (output_tx, output_rx) = mpsc::channel();
        let task = Self {
            resize,
            exit: exit.clone(),
            starting: starting.clone(),
            cancelled: cancelled.clone(),
            owned: owned.clone(),
        };
        let reader_exit = exit.clone();
        std::thread::spawn(move || {
            if cancelled.load(Ordering::Acquire) {
                return;
            }
            match start() {
                Ok((session, writer, reader)) => {
                    let resizer = session.resize.clone();
                    let status = session.exit.clone();
                    {
                        let mut slot = owned.lock();
                        if cancelled.load(Ordering::Acquire) {
                            return;
                        }
                        *slot = Some(session);
                    }
                    let _ = output_tx.send((reader, status));
                    let _ = input_tx.send(writer);
                    starting.store(false, Ordering::Release);
                    while let Ok(mut size) = changes.recv() {
                        while let Ok(latest) = changes.try_recv() {
                            size = latest;
                        }
                        if resizer.send(size).is_err() {
                            break;
                        }
                    }
                }
                Err(error) => {
                    *exit.lock() = Some(1);
                    starting.store(false, Ordering::Release);
                    let message =
                        format!("\r\n\x1b[31mCould not start terminal: {error}\x1b[0m\r\n");
                    let _ = output_tx.send((
                        Box::new(io::Cursor::new(message.into_bytes())) as Box<dyn Read + Send>,
                        exit,
                    ));
                }
            }
        });
        (
            task,
            PendingWriter {
                ready: input_rx,
                inner: None,
            },
            PendingReader {
                ready: output_rx,
                inner: None,
                status: None,
                exit: reader_exit,
            },
        )
    }
}
impl Drop for SessionTask {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let owned = self.owned.clone();
        std::thread::spawn(move || {
            drop(owned.lock().take());
        });
    }
}
