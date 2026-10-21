// Copyright 2026 git-next contributors. MIT license.
use crate::{
    Desktop,
    field::Field,
    monitor,
    remote::{self, Host},
    sftp::{self, Entry, Sftp},
};
use gpui::*;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub enum Event {
    Listed(u64, String, Result<Vec<Entry>, String>),
    Sample(u64, monitor::Snapshot),
    MonitorError(u64, String),
    TransferStarted(u64),
    Progress(u64, u64),
    Transferred(u64, Result<(), String>),
}
pub struct Transfer {
    pub id: u64,
    pub name: String,
    pub state: String,
    pub bytes: u64,
    pub cancel: Arc<AtomicBool>,
    pub started: Instant,
}
pub struct Editor {
    pub index: Option<usize>,
    pub fields: Vec<Entity<Field>>,
}
struct TransferJob {
    id: u64,
    host: Host,
    git: Option<PathBuf>,
    local: PathBuf,
    path: String,
    upload: bool,
    overwrite: bool,
    cancel: Arc<AtomicBool>,
}
pub struct Workspace {
    pub hosts: Vec<Host>,
    pub visible: bool,
    pub selected: Option<usize>,
    pub right: bool,
    pub monitoring: bool,
    show_all_cores: bool,
    pub interval: u64,
    pub directory: String,
    pub entries: Vec<Entry>,
    pub busy: bool,
    pub error: Option<String>,
    pub generation: u64,
    pub monitor_cancel: Option<Arc<AtomicBool>>,
    pub list_cancel: Option<Arc<AtomicBool>>,
    pub snapshot: Option<monitor::Snapshot>,
    pub history: VecDeque<f64>,
    pub transfers: Vec<Transfer>,
    pub context: Option<Entry>,
    pub editor: Option<Editor>,
    pub sender: flume::Sender<Event>,
    pub receiver: flume::Receiver<Event>,
    next_transfer: u64,
    jobs: flume::Sender<TransferJob>,
    config_writable: bool,
}
impl Workspace {
    pub fn new() -> Self {
        let (hosts, error) = match remote::load_hosts() {
            Ok(hosts) => (hosts, None),
            Err(error) => (Vec::new(), Some(error.to_string())),
        };
        let (sender, receiver) = flume::bounded(64);
        let config_writable = error.is_none();
        let (jobs, incoming) = flume::bounded::<TransferJob>(256);
        for _ in 0..2 {
            let incoming = incoming.clone();
            let sender = sender.clone();
            std::thread::spawn(move || {
                while let Ok(job) = incoming.recv() {
                    let result = (|| -> anyhow::Result<()> {
                        anyhow::ensure!(!job.cancel.load(Ordering::Acquire), "Transfer cancelled");
                        let _ = sender.send(Event::TransferStarted(job.id));
                        let ssh = remote::ssh_executable(job.git.as_deref())?;
                        let mut client = Sftp::connect(&job.host, &ssh, job.cancel)?;
                        let mut total = 0u64;
                        let mut last_notice = Instant::now();
                        let mut progress = |bytes| {
                            total += bytes;
                            if last_notice.elapsed().as_millis() >= 50 {
                                let _ = sender.try_send(Event::Progress(job.id, total));
                                last_notice = Instant::now();
                            }
                        };
                        if job.upload {
                            client.upload(&job.local, &job.path, job.overwrite, &mut progress)?;
                        } else {
                            client.download(&job.path, &job.local, job.overwrite, &mut progress)?;
                        }
                        let _ = sender.send(Event::Progress(job.id, total));
                        Ok(())
                    })();
                    let _ = sender.send(Event::Transferred(
                        job.id,
                        result.map_err(|e| e.to_string()),
                    ));
                }
            });
        }
        Self {
            hosts,
            visible: false,
            selected: None,
            right: false,
            monitoring: false,
            show_all_cores: false,
            interval: 2,
            directory: ".".into(),
            entries: Vec::new(),
            busy: false,
            error,
            generation: 0,
            monitor_cancel: None,
            list_cancel: None,
            snapshot: None,
            history: VecDeque::new(),
            transfers: Vec::new(),
            context: None,
            editor: None,
            sender,
            receiver,
            next_transfer: 0,
            jobs,
            config_writable,
        }
    }
    pub fn stop_monitor(&mut self) {
        if let Some(cancel) = self.monitor_cancel.take() {
            cancel.store(true, Ordering::Release);
        }
    }
    pub fn stop_listing(&mut self) {
        if let Some(cancel) = self.list_cancel.take() {
            cancel.store(true, Ordering::Release);
        }
    }
    pub fn host(&self) -> Option<Host> {
        self.selected.and_then(|i| self.hosts.get(i)).cloned()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        self.stop_monitor();
        self.stop_listing();
        for transfer in &self.transfers {
            transfer.cancel.store(true, Ordering::Release);
        }
    }
}

fn label(zh: bool, chinese: &'static str, english: &'static str) -> &'static str {
    if zh { chinese } else { english }
}
fn button(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(12.))
        .hover(|s| s.bg(rgb(0x2b3545)))
        .child(text.into())
}
fn bytes(value: f64) -> String {
    if value >= 1024. * 1024. * 1024. {
        format!("{:.1} GiB", value / (1024. * 1024. * 1024.))
    } else if value >= 1024. * 1024. {
        format!("{:.1} MiB", value / (1024. * 1024.))
    } else if value >= 1024. {
        format!("{:.1} KiB", value / 1024.)
    } else {
        format!("{value:.0} B")
    }
}
fn meter(fraction: f64, color: u32) -> Div {
    div()
        .h(px(4.))
        .rounded_full()
        .bg(rgb(0x2b3545))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0., 1.) as f32))
                .bg(rgb(color)),
        )
}

impl Desktop {
    pub fn apply_remote_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let refresh_files = matches!(&event, Event::Transferred(_, Ok(())));
        match event {
            Event::Listed(generation, directory, entries)
                if generation == self.remote.generation =>
            {
                self.remote.busy = false;
                match entries {
                    Ok(entries) => {
                        self.remote.directory = directory;
                        self.remote.entries = entries;
                        self.remote.error = None;
                    }
                    Err(error) => self.remote.error = Some(error),
                }
            }
            Event::Sample(generation, sample) if generation == self.remote.generation => {
                if let Some(used) = sample.cpus.first().and_then(|c| c.used) {
                    self.remote.history.push_back(used);
                    if self.remote.history.len() > 60 {
                        self.remote.history.pop_front();
                    }
                }
                self.remote.snapshot = Some(sample);
                self.remote.error = None;
            }
            Event::MonitorError(generation, error) if generation == self.remote.generation => {
                self.remote.stop_monitor();
                self.remote.error = Some(error)
            }
            Event::TransferStarted(id) => {
                if let Some(t) = self.remote.transfers.iter_mut().find(|t| t.id == id) {
                    t.state = "running".into();
                    t.started = Instant::now();
                }
            }
            Event::Progress(id, bytes) => {
                if let Some(t) = self.remote.transfers.iter_mut().find(|t| t.id == id) {
                    t.bytes = bytes;
                }
            }
            Event::Transferred(id, result) => {
                if let Some(t) = self.remote.transfers.iter_mut().find(|t| t.id == id) {
                    t.state = match result {
                        Ok(()) => "done".into(),
                        Err(_) if t.cancel.load(Ordering::Acquire) => "cancelled".into(),
                        Err(error) => format!("failed: {error}"),
                    };
                }
            }
            _ => {}
        }
        if self.remote.transfers.len() > 460 {
            let completed = self
                .remote
                .transfers
                .iter()
                .position(|t| !matches!(t.state.as_str(), "queued" | "running"));
            if let Some(index) = completed {
                self.remote.transfers.remove(index);
            }
        }
        if refresh_files
            && self.remote.right
            && !self.remote.monitoring
            && self.remote.selected.is_some()
        {
            self.list_remote(self.remote.directory.clone(), cx);
        }
        cx.notify();
    }
    fn select_host(&mut self, index: usize, cx: &mut Context<Self>) {
        self.remote.stop_monitor();
        self.remote.stop_listing();
        self.remote.selected = Some(index);
        self.remote.interval = match self.remote.hosts[index].monitor_interval {
            0 => 2,
            value => value.clamp(1, 60),
        };
        self.remote.right = true;
        self.remote.monitoring = false;
        self.remote.directory = ".".into();
        self.remote.entries.clear();
        self.remote.snapshot = None;
        self.remote.history.clear();
        self.remote.context = None;
        self.list_remote(".".into(), cx);
    }
    fn list_remote(&mut self, directory: String, cx: &mut Context<Self>) {
        let Some(host) = self.remote.host() else {
            return;
        };
        self.remote.stop_listing();
        self.remote.generation += 1;
        let generation = self.remote.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.remote.list_cancel = Some(cancel.clone());
        self.remote.busy = true;
        self.remote.error = None;
        self.remote.context = None;
        let sender = self.remote.sender.clone();
        let git = self.setup.git.clone();
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<(String, Vec<Entry>)> {
                let ssh = remote::ssh_executable(git.as_deref())?;
                let mut client = Sftp::connect(&host, &ssh, cancel)?;
                let directory = if directory == "." {
                    client.home()?
                } else {
                    directory.clone()
                };
                Ok((directory.clone(), client.list(&directory)?))
            })();
            let event = match result {
                Ok((directory, entries)) => Event::Listed(generation, directory, Ok(entries)),
                Err(error) => Event::Listed(generation, directory, Err(error.to_string())),
            };
            let _ = sender.send(event);
        });
        cx.notify();
    }
    fn start_monitor(&mut self, cx: &mut Context<Self>) {
        let Some(mut host) = self.remote.host() else {
            return;
        };
        self.remote.stop_monitor();
        self.remote.stop_listing();
        self.remote.generation += 1;
        self.remote.monitoring = true;
        self.remote.error = None;
        self.remote.snapshot = None;
        let generation = self.remote.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.remote.monitor_cancel = Some(cancel.clone());
        host.monitor_interval = self.remote.interval;
        let sender = self.remote.sender.clone();
        let git = self.setup.git.clone();
        std::thread::spawn(move || {
            let result = remote::ssh_executable(git.as_deref()).and_then(|ssh| {
                monitor::watch(&host, &ssh, cancel.clone(), |sample| {
                    sender.send(Event::Sample(generation, sample)).is_ok()
                })
            });
            if !cancel.load(Ordering::Acquire)
                && let Err(error) = result
            {
                let _ = sender.send(Event::MonitorError(generation, error.to_string()));
            }
        });
        cx.notify();
    }
    fn queue_transfer(
        &mut self,
        local: PathBuf,
        path: String,
        upload: bool,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.remote.host() else {
            return;
        };
        self.remote.next_transfer += 1;
        let id = self.remote.next_transfer;
        let cancel = Arc::new(AtomicBool::new(false));
        let name = local
            .file_name()
            .unwrap_or(local.as_os_str())
            .to_string_lossy()
            .into_owned();
        self.remote.transfers.push(Transfer {
            id,
            name,
            state: "queued".into(),
            bytes: 0,
            cancel: cancel.clone(),
            started: Instant::now(),
        });
        let job = TransferJob {
            id,
            host,
            git: self.setup.git.clone(),
            local,
            path,
            upload,
            overwrite,
            cancel,
        };
        if self.remote.jobs.try_send(job).is_err()
            && let Some(transfer) = self.remote.transfers.last_mut()
        {
            transfer.state = "failed: transfer queue is full (256)".into();
        }
        cx.notify();
    }
    fn upload_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let paths = paths.to_vec();
        let directory = self.remote.directory.clone();
        let existing = paths.iter().any(|path| {
            path.file_name().is_some_and(|name| {
                self.remote
                    .entries
                    .iter()
                    .any(|entry| entry.name == name.to_string_lossy())
            })
        });
        let prompt = existing.then(|| {
            window.prompt(
                PromptLevel::Warning,
                "Replace existing remote files?",
                Some("Files are replaced only after a complete upload."),
                &["Cancel", "Replace"],
                cx,
            )
        });
        let host = self.remote.selected;
        cx.spawn(async move |this, cx| {
            let overwrite = if let Some(prompt) = prompt {
                if prompt.await.ok() != Some(1) {
                    return;
                }
                true
            } else {
                false
            };
            let _ = this.update(cx, |app, cx| {
                if app.remote.selected != host {
                    return;
                }
                for local in paths {
                    if let Some(name) = local.file_name().and_then(|s| s.to_str()) {
                        let target = sftp::join(&directory, name);
                        app.queue_transfer(local, target, true, overwrite, cx);
                    }
                }
            });
        })
        .detach();
    }
    fn pick_upload(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Upload via SFTP".into()),
        });
        let handle = self.window;
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |app, cx| app.upload_paths(&paths, window, cx));
                });
            }
        })
        .detach();
    }
    fn download_entry(&mut self, entry: Entry, cx: &mut Context<Self>) {
        self.remote.context = None;
        let path = sftp::join(&self.remote.directory, &entry.name);
        let host = self.remote.selected;
        let initial = self.setup.cwd.clone();
        let chosen = cx.prompt_for_new_path(&initial, Some(&entry.name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(local))) = chosen.await {
                let _ = this.update(cx, |app, cx| {
                    if app.remote.selected == host {
                        app.queue_transfer(local, path, false, true, cx);
                    }
                });
            }
        })
        .detach();
        cx.notify();
    }
    fn edit_host(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let host = index
            .and_then(|i| self.remote.hosts.get(i))
            .cloned()
            .unwrap_or_default();
        let values = [
            host.name,
            host.group,
            host.destination,
            host.user,
            host.port.map(|p| p.to_string()).unwrap_or_default(),
            host.identity,
            host.config,
        ];
        let fields: Vec<_> = values
            .iter()
            .map(|value| cx.new(|cx| Field::new(value, cx)))
            .collect();
        fields[0].read(cx).focus_handle().focus(window, cx);
        self.remote.editor = Some(Editor { index, fields });
        cx.notify();
    }
    pub fn save_host_editor(&mut self, cx: &mut Context<Self>) {
        if !self.remote.config_writable {
            self.remote.error = Some("Existing hosts.json could not be loaded. Repair it before saving; no records were overwritten.".into());
            cx.notify();
            return;
        }
        let Some(editor) = &self.remote.editor else {
            return;
        };
        let values: Vec<_> = editor
            .fields
            .iter()
            .map(|field| field.read(cx).value.trim().to_owned())
            .collect();
        let port = if values[4].is_empty() {
            None
        } else {
            match values[4].parse::<u16>() {
                Ok(port) => Some(port),
                Err(_) => {
                    self.remote.error = Some("Invalid port".into());
                    cx.notify();
                    return;
                }
            }
        };
        let host = Host {
            name: values[0].clone(),
            group: values[1].clone(),
            destination: values[2].clone(),
            user: values[3].clone(),
            port,
            identity: values[5].clone(),
            config: values[6].clone(),
            monitor_interval: self.remote.interval,
        };
        let mut hosts = self.remote.hosts.clone();
        if hosts
            .iter()
            .enumerate()
            .any(|(i, h)| Some(i) != editor.index && h.name == host.name)
        {
            self.remote.error = Some("Host name already exists".into());
            cx.notify();
            return;
        }
        if let Some(index) = editor.index {
            hosts[index] = host;
        } else {
            hosts.push(host);
        }
        match remote::save_hosts(&hosts) {
            Ok(()) => {
                self.remote.hosts = hosts;
                self.remote.editor = None;
                self.remote.error = None;
            }
            Err(error) => self.remote.error = Some(error.to_string()),
        }
        cx.notify();
    }
    fn import_hosts(&mut self, cx: &mut Context<Self>) {
        if !self.remote.config_writable {
            return;
        }
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import OpenSSH config".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await {
                let imported = remote::import_config(&paths[0]);
                let _ = this.update(cx, |app, cx| {
                    let result = imported.and_then(|hosts| {
                        let mut saved = app.remote.hosts.clone();
                        for host in hosts {
                            if !saved.iter().any(|h| h.name == host.name) {
                                saved.push(host);
                            }
                        }
                        remote::save_hosts(&saved)?;
                        app.remote.hosts = saved;
                        Ok(())
                    });
                    app.remote.error = result.err().map(|e| e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub fn render_remote_sidebar(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let zh = self.settings.chinese();
        let mut rows = div().flex().flex_col().gap_1();
        let mut groups: Vec<_> = self.remote.hosts.iter().map(|h| h.group.clone()).collect();
        groups.sort();
        groups.dedup();
        for group in groups {
            rows = rows.child(
                div()
                    .px_2()
                    .pt_3()
                    .pb_1()
                    .text_size(px(11.))
                    .text_color(rgb(0x8c9ab0))
                    .child(if group.is_empty() {
                        "SSH".into()
                    } else {
                        group.clone()
                    }),
            );
            for (index, host) in self
                .remote
                .hosts
                .iter()
                .enumerate()
                .filter(|(_, h)| h.group == group)
            {
                rows = rows.child(
                    div()
                        .id(("host", index))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_2()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(rgb(if self.remote.selected == Some(index) {
                            0x252e3f
                        } else {
                            0x171c26
                        }))
                        .on_click(cx.listener(move |app, _, _, cx| app.select_host(index, cx)))
                        .child(
                            div()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(host.name.clone()),
                        )
                        .child(
                            div()
                                .text_color(rgb(0x8c9ab0))
                                .text_size(px(11.))
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(host.destination.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    button(("connect", index), label(zh, "连接", "Connect"))
                                        .on_click(cx.listener(move |app, _, window, cx| {
                                            cx.stop_propagation();
                                            let host = app.remote.hosts[index].clone();
                                            app.open_ssh(host, window, cx);
                                        })),
                                )
                                .child(
                                    button(("edit-host", index), label(zh, "编辑", "Edit"))
                                        .on_click(cx.listener(move |app, _, window, cx| {
                                            cx.stop_propagation();
                                            app.edit_host(Some(index), window, cx);
                                        })),
                                ),
                        ),
                );
            }
        }
        div().id("ssh-hosts").w(px(216.)).min_w(px(176.)).h_full().flex_shrink_0().flex().flex_col().bg(rgb(0x171c26)).border_r_1().border_color(rgb(0x252e3f))
            .child(div().p_3().flex().items_center().justify_between().child(label(zh, "SSH 主机", "SSH hosts"))
                .child(button("add-host", "+").on_click(cx.listener(|app, _, window, cx| app.edit_host(None, window, cx)))))
            .child(div().id("host-scroll").flex_1().min_h_0().overflow_y_scroll().px_2().child(rows)
                .children(self.remote.hosts.is_empty().then(|| div().p_2().text_color(rgb(0x8c9ab0)).child(label(zh, "添加主机，或导入 SSH config。支持密钥、Agent 和跳板机。", "Add a host or import SSH config. Keys, agents and jump hosts are supported.")))))
            .child(button("import-hosts", label(zh, "导入 SSH config", "Import SSH config")).m_2().on_click(cx.listener(|app, _, _, cx| app.import_hosts(cx))))
    }
    pub fn render_remote_panel(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let zh = self.settings.chinese();
        let host = self.remote.host();
        let accepts_files = host.is_some() && !self.remote.monitoring;
        let title = host
            .as_ref()
            .map(|h| h.name.clone())
            .unwrap_or_else(|| "SSH".into());
        let mut content = div().flex().flex_col().gap_3().p_3();
        if self.remote.monitoring {
            content = content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label(zh, "只读 · 无需代理", "Read only · no agent"))
                        .child(
                            button("interval", format!("{}s", self.remote.interval)).on_click(
                                cx.listener(|app, _, _, cx| {
                                    app.remote.interval = match app.remote.interval {
                                        1 => 2,
                                        2 => 5,
                                        5 => 10,
                                        _ => 1,
                                    };
                                    if let Some(index) = app.remote.selected {
                                        app.remote.hosts[index].monitor_interval =
                                            app.remote.interval;
                                        let _ = remote::save_hosts(&app.remote.hosts);
                                    }
                                    app.start_monitor(cx);
                                }),
                            ),
                        ),
                )
                .child(
                    button(
                        "pause-monitor",
                        if self.remote.monitor_cancel.is_some() {
                            label(zh, "暂停监控", "Pause monitoring")
                        } else {
                            label(zh, "恢复监控", "Resume monitoring")
                        },
                    )
                    .on_click(cx.listener(|app, _, _, cx| {
                        if app.remote.monitor_cancel.is_some() {
                            app.remote.stop_monitor();
                        } else {
                            app.start_monitor(cx);
                        }
                        cx.notify();
                    })),
                );
            if let Some(sample) = &self.remote.snapshot {
                content = content
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(sample.os.clone())
                            .child(div().text_color(rgb(0x8c9ab0)).child(format!(
                                "{} · {:.0}h",
                                sample.timezone,
                                sample.uptime / 3600.
                            ))),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(0x2b3545))
                            .pt_3()
                            .flex()
                            .justify_between()
                            .child("CPU")
                            .child(
                                sample
                                    .cpus
                                    .first()
                                    .and_then(|cpu| cpu.used)
                                    .map(|value| format!("{value:.1}%"))
                                    .unwrap_or_else(|| "…".into()),
                            ),
                    );
                let history: Vec<_> = self.remote.history.iter().copied().collect();
                content = content.child(
                    canvas(
                        |bounds, _, _| bounds,
                        move |bounds, _, window, _| {
                            if history.len() < 2 {
                                return;
                            }
                            let mut path = PathBuilder::stroke(px(1.5));
                            for (i, value) in history.iter().enumerate() {
                                let point = point(
                                    bounds.left() + bounds.size.width * (i as f32 / 59.),
                                    bounds.bottom() - bounds.size.height * (*value as f32 / 100.),
                                );
                                if i == 0 {
                                    path.move_to(point);
                                } else {
                                    path.line_to(point);
                                }
                            }
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(0x85cbcf));
                            }
                        },
                    )
                    .h(px(38.))
                    .w_full(),
                );
                let cores: Vec<_> = sample
                    .cpus
                    .iter()
                    .skip(1)
                    .take(if self.remote.show_all_cores {
                        usize::MAX
                    } else {
                        8
                    })
                    .collect();
                let mut core_grid = div().flex().flex_col().gap_1();
                for row in cores.chunks(4) {
                    let mut cells = div().flex().gap_2();
                    for cpu in row {
                        cells = cells.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_size(px(10.)).child(format!(
                                        "{} {}",
                                        cpu.name.trim_start_matches("cpu"),
                                        cpu.used
                                            .map(|v| format!("{v:.0}%"))
                                            .unwrap_or_else(|| "…".into())
                                    )))
                                .child(meter(cpu.used.unwrap_or(0.) / 100., 0x9acfa0)),
                        );
                    }
                    core_grid = core_grid.child(cells);
                }
                content = content.child(core_grid).child(
                    button(
                        "all-cpu-cores",
                        format!(
                            "{} {} {}",
                            sample.cpus.len().saturating_sub(1),
                            label(zh, "核", "cores"),
                            if self.remote.show_all_cores {
                                "⌃"
                            } else {
                                "⌄"
                            }
                        ),
                    )
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.remote.show_all_cores = !app.remote.show_all_cores;
                        cx.notify();
                    })),
                );
                content = content
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(0x2b3545))
                            .pt_3()
                            .child(label(zh, "内存", "Memory")),
                    )
                    .child(meter(
                        sample.memory_used as f64 / sample.memory_total.max(1) as f64,
                        0xc4a5ef,
                    ))
                    .child(format!(
                        "{} / {}",
                        bytes(sample.memory_used as f64),
                        bytes(sample.memory_total as f64)
                    ))
                    .child(
                        div()
                            .text_color(rgb(0x8c9ab0))
                            .text_size(px(11.))
                            .child(format!(
                                "{} {}",
                                label(zh, "缓存", "Cache"),
                                bytes(sample.memory_cache as f64)
                            )),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(0x2b3545))
                            .pt_3()
                            .child(label(zh, "网络", "Network")),
                    )
                    .child(format!(
                        "↓ {} /s",
                        sample
                            .rx_per_second
                            .map(bytes)
                            .unwrap_or_else(|| "…".into())
                    ))
                    .child(format!(
                        "↑ {} /s",
                        sample
                            .tx_per_second
                            .map(bytes)
                            .unwrap_or_else(|| "…".into())
                    ))
                    .child(
                        div()
                            .text_color(rgb(0x8c9ab0))
                            .text_size(px(11.))
                            .child(format!(
                                "↓ {}   ↑ {}",
                                bytes(sample.rx_bytes as f64),
                                bytes(sample.tx_bytes as f64)
                            )),
                    )
                    .child(
                        div()
                            .border_t_1()
                            .border_color(rgb(0x2b3545))
                            .pt_3()
                            .child(label(zh, "磁盘", "Disk")),
                    );
                for disk in sample.disks.iter().take(16) {
                    content = content.child(div().text_size(px(11.)).child(format!(
                            "{} · R {} /s · W {} /s",
                            disk.name,
                            disk.read_per_second
                                .map(bytes)
                                .unwrap_or_else(|| "…".into()),
                            disk.write_per_second
                                .map(bytes)
                                .unwrap_or_else(|| "…".into())
                        )));
                }
                for space in sample.spaces.iter().take(12) {
                    content = content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_size(px(11.)).child(space.mount.clone()))
                            .child(meter(
                                space.used as f64 / space.total.max(1) as f64,
                                0x7cb7ff,
                            ))
                            .child(div().text_color(rgb(0x8c9ab0)).text_size(px(11.)).child(
                                format!(
                                    "{} / {}",
                                    bytes(space.used as f64),
                                    bytes(space.total as f64)
                                ),
                            )),
                    );
                }
            } else {
                content = content.child(label(zh, "连接并采样中…", "Connecting and sampling…"));
            }
        } else {
            content = content
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(button("remote-parent", "↑").on_click(cx.listener(
                            |app, _, _, cx| {
                                let path = app
                                    .remote
                                    .directory
                                    .trim_end_matches('/')
                                    .rsplit_once('/')
                                    .map(|(p, _)| if p.is_empty() { "/" } else { p })
                                    .unwrap_or("/")
                                    .to_owned();
                                app.list_remote(path, cx);
                            },
                        )))
                        .child(button("remote-refresh", "↻").on_click(cx.listener(
                            |app, _, _, cx| app.list_remote(app.remote.directory.clone(), cx),
                        )))
                        .child(
                            button("upload", label(zh, "上传…", "Upload…"))
                                .on_click(cx.listener(|app, _, _, cx| app.pick_upload(cx))),
                        ),
                )
                .child(
                    div()
                        .text_color(rgb(0x8c9ab0))
                        .text_size(px(11.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(self.remote.directory.clone()),
                )
                .child(
                    div()
                        .text_color(rgb(0x8c9ab0))
                        .text_size(px(11.))
                        .child(label(
                            zh,
                            "拖入文件上传 · 右键下载",
                            "Drop to upload · right click to download",
                        )),
                );
            if self.remote.busy {
                content = content.child(label(zh, "读取目录中…", "Reading directory…"));
            }
            content = content.child(
                uniform_list(
                    "sftp-file-list",
                    self.remote.entries.len(),
                    cx.processor(|app, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|index| {
                                let entry = &app.remote.entries[index];
                                let selected = entry.clone();
                                let download = entry.clone();
                                div()
                                    .id(("remote-file", index))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .py_1()
                                    .px_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(0x252e3f)))
                                    .child(
                                        div()
                                            .text_color(rgb(if entry.directory() {
                                                0xe9c48a
                                            } else {
                                                0x8c9ab0
                                            }))
                                            .child(if entry.directory() { "▸" } else { "·" }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(entry.name.clone()),
                                    )
                                    .child(
                                        div().text_size(px(10.)).text_color(rgb(0x8c9ab0)).child(
                                            if entry.directory() {
                                                String::new()
                                            } else {
                                                bytes(entry.size as f64)
                                            },
                                        ),
                                    )
                                    .on_click(cx.listener(move |app, _, _, cx| {
                                        if selected.directory() {
                                            app.list_remote(
                                                sftp::join(&app.remote.directory, &selected.name),
                                                cx,
                                            );
                                        }
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |app, _, _, cx| {
                                            app.remote.context = Some(download.clone());
                                            cx.notify();
                                        }),
                                    )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .h(px(320.))
                .w_full(),
            );
        }
        let mut queue = div().flex().flex_col().gap_1();
        let mut transfers: Vec<_> = self.remote.transfers.iter().rev().collect();
        transfers.sort_by_key(|t| !["running", "queued"].contains(&t.state.as_str()));
        for transfer in transfers {
            let id = transfer.id;
            let state = match transfer.state.as_str() {
                "queued" => label(zh, "排队", "Queued"),
                "running" => label(zh, "传输中", "Running"),
                "done" => label(zh, "完成", "Done"),
                "cancelled" => label(zh, "已取消", "Cancelled"),
                other => other,
            };
            queue = queue.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(format!(
                                "{} · {state} · {}",
                                transfer.name,
                                bytes(transfer.bytes as f64)
                            )),
                    )
                    .children(
                        ["queued", "running"]
                            .contains(&transfer.state.as_str())
                            .then(|| {
                                button(("cancel-transfer", id), "×").on_click(cx.listener(
                                    move |app, _, _, cx| {
                                        if let Some(t) =
                                            app.remote.transfers.iter().find(|t| t.id == id)
                                        {
                                            t.cancel.store(true, Ordering::Release);
                                        }
                                        cx.notify();
                                    },
                                ))
                            }),
                    ),
            );
        }
        let context = self.remote.context.clone().map(|entry| {
            div()
                .absolute()
                .top(px(92.))
                .left(px(10.))
                .right(px(10.))
                .p_2()
                .rounded_md()
                .bg(rgb(0x252e3f))
                .border_1()
                .border_color(rgb(0x354157))
                .shadow_lg()
                .occlude()
                .child(
                    div()
                        .text_size(px(11.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(entry.name.clone()),
                )
                .child(
                    button("download-entry", label(zh, "下载…", "Download…")).on_click(
                        cx.listener(move |app, _, _, cx| app.download_entry(entry.clone(), cx)),
                    ),
                )
                .on_mouse_down_out(cx.listener(|app, _, _, cx| {
                    app.remote.context = None;
                    cx.notify();
                }))
        });
        div()
            .id("remote-panel")
            .relative()
            .w(px(300.))
            .min_w(px(240.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(0x141923))
            .border_l_1()
            .border_color(rgb(0x252e3f))
            .can_drop(move |payload, _, _| accepts_files && payload.is::<ExternalPaths>())
            .on_drop::<ExternalPaths>(cx.listener(|app, paths: &ExternalPaths, window, cx| {
                app.upload_paths(paths.paths(), window, cx)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_2()
                    .child(div().overflow_hidden().text_ellipsis().child(title))
                    .child(button("close-remote-panel", "×").on_click(cx.listener(
                        |app, _, _, cx| {
                            app.remote.right = false;
                            app.remote.stop_monitor();
                            cx.notify();
                        },
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .px_2()
                    .child(
                        button("files-mode", label(zh, "文件", "Files")).on_click(cx.listener(
                            |app, _, _, cx| {
                                app.remote.stop_monitor();
                                app.remote.monitoring = false;
                                app.list_remote(app.remote.directory.clone(), cx);
                            },
                        )),
                    )
                    .child(
                        button("monitor-mode", label(zh, "监控", "Monitor"))
                            .on_click(cx.listener(|app, _, _, cx| app.start_monitor(cx))),
                    ),
            )
            .child(
                div()
                    .id("remote-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content)
                    .children(
                        self.remote
                            .error
                            .clone()
                            .map(|error| div().p_3().text_color(rgb(0xf08691)).child(error)),
                    ),
            )
            .children((!self.remote.transfers.is_empty()).then(|| {
                div()
                    .id("transfer-queue")
                    .p_2()
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_color(rgb(0x2b3545))
                    .child(queue)
            }))
            .children(context)
    }
    pub fn render_host_editor(&self, cx: &mut Context<Self>) -> Option<Div> {
        let editor = self.remote.editor.as_ref()?;
        let zh = self.settings.chinese();
        let labels = if zh {
            [
                "名称",
                "分组",
                "主机 / SSH 别名",
                "用户名（可选）",
                "端口（留空使用 SSH config）",
                "私钥文件（可选）",
                "SSH config 文件（可选）",
            ]
        } else {
            [
                "Name",
                "Group",
                "Host / SSH alias",
                "User (optional)",
                "Port (blank: SSH config)",
                "Identity file (optional)",
                "SSH config file (optional)",
            ]
        };
        let mut fields = div().flex().flex_col().gap_2();
        for (label, field) in labels.into_iter().zip(&editor.fields) {
            fields = fields.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(0x8c9ab0))
                            .child(label),
                    )
                    .child(field.clone()),
            );
        }
        let index = editor.index;
        Some(div().absolute().inset_0().flex().items_center().justify_center().bg(rgba(0x00000099)).occlude()
            .child(div().w(px(460.)).max_w(relative(0.95)).p_5().rounded_lg().bg(rgb(0x171c26)).border_1().border_color(rgb(0x354157)).flex().flex_col().gap_3()
                .child(div().text_size(px(18.)).child(label(zh, "SSH 主机", "SSH host"))).child(fields)
                .child(div().text_size(px(11.)).text_color(rgb(0x8c9ab0)).child(label(zh, "支持 OpenSSH Agent、ProxyJump 和 known_hosts。密码在原生登录窗口输入，不写入此配置。", "Uses OpenSSH agents, ProxyJump and known_hosts. Passwords are entered in a native authentication dialog and are never saved here.")))
                .children(self.remote.error.clone().map(|error| div().text_color(rgb(0xf08691)).child(error)))
                .child(div().flex().justify_between().gap_2()
                    .children(index.map(|index| button("delete-host", label(zh, "删除主机", "Delete host")).text_color(rgb(0xf08691)).on_click(cx.listener(move |app, _, _, cx| {
                        let mut hosts = app.remote.hosts.clone(); hosts.remove(index);
                        match remote::save_hosts(&hosts) { Ok(()) => { app.remote.stop_monitor(); app.remote.stop_listing(); app.remote.hosts = hosts; app.remote.selected = None; app.remote.right = false; app.remote.editor = None; }, Err(error) => app.remote.error = Some(error.to_string()) } cx.notify();
                    }))))
                    .child(div().flex().gap_2().child(button("cancel-host", label(zh, "取消", "Cancel")).on_click(cx.listener(|app, _, window, cx| { app.remote.editor = None; app.focus(window, cx); cx.notify(); })))
                        .child(button("save-host", label(zh, "保存", "Save")).bg(rgb(0x2a476a)).on_click(cx.listener(|app, _, window, cx| { app.save_host_editor(cx); if app.remote.editor.is_none() { app.focus(window, cx); } })))))))
    }
}
