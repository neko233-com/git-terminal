// Copyright 2026 git-next contributors. MIT license.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod assets;
#[cfg(windows)]
mod conpty_input;
mod field;
mod host_config;
mod i18n;
mod monitor;
mod presentation;
mod remote;
mod remote_cli;
mod remote_ui;
mod session;
mod session_task;
mod settings;
mod sftp;
#[cfg(all(test, windows))]
mod shell_acceptance;

use anyhow::Result;
use clap::{Parser, Subcommand};
use gpui::*;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use presentation::{TabContext, profile_name};
use session::{Session, Setup};
use settings::Settings;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

#[derive(Parser, Clone)]
#[command(
    name = "git-terminal",
    version,
    about = "Git-Terminal · Rust + GPUI Fast native terminal"
)]
struct Options {
    #[arg(long, alias = "cd", conflicts_with = "cd_to_home")]
    cwd: Option<PathBuf>,
    #[arg(long, conflicts_with = "no_cd")]
    cd_to_home: bool,
    #[arg(long)]
    no_cd: bool,
    #[arg(long)]
    no_profile: bool,
    #[arg(long)]
    no_shell_integration: bool,
    #[arg(long,default_value="auto",value_parser=["auto","bash","powershell","cmd","shell"])]
    shell: String,
    #[arg(long, conflicts_with = "command")]
    ssh: Option<String>,
    #[arg(long)]
    command: Option<String>,
    #[arg(long)]
    git: Option<PathBuf>,
    #[arg(long)]
    print_setup: bool,
    #[arg(long)]
    json: bool,
    #[command(subcommand)]
    action: Option<Action>,
}
#[derive(Subcommand, Clone)]
enum Action {
    Remote {
        #[command(subcommand)]
        action: remote_cli::RemoteAction,
    },
    Language {
        #[command(subcommand)]
        action: LanguageAction,
    },
}
#[derive(Subcommand, Clone)]
enum LanguageAction {
    Get,
    Set {
        /// The language to use: a locale code, or `auto` to follow the device.
        language: String,
    },
    /// List the languages that ship with this build.
    List,
}

struct Tab {
    id: u64,
    context: TabContext,
    _session: session_task::SessionTask,
    terminal: Entity<TerminalView>,
    remote: Option<remote::Host>,
}
struct Desktop {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    setup: Setup,
    settings: Settings,
    error: Option<String>,
    command_exit: Arc<AtomicU32>,
    profile_menu: bool,
    tab_scroll: ScrollHandle,
    remote: remote_ui::Workspace,
    _remote_events: Task<()>,
    window: AnyWindowHandle,
}

impl Desktop {
    fn new(
        setup: Setup,
        settings: Settings,
        command: Option<String>,
        remote_host: Option<remote::Host>,
        command_exit: Arc<AtomicU32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let remote = remote_ui::Workspace::new();
        let receiver = remote.receiver.clone();
        let remote_events = cx.spawn(async move |this, cx| {
            while let Ok(event) = receiver.recv_async().await {
                if this
                    .update(cx, |app: &mut Self, cx| app.apply_remote_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut desktop = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            setup,
            settings,
            error: None,
            command_exit,
            profile_menu: false,
            tab_scroll: ScrollHandle::new(),
            remote,
            _remote_events: remote_events,
            window: window.window_handle(),
        };
        presentation::dark_caption();
        desktop.add_tab_for(command, remote_host, window, cx);
        desktop
    }
    fn add_tab(&mut self, command: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let remote = if command.is_none() {
            self.tabs
                .get(self.active)
                .and_then(|tab| tab.remote.clone())
        } else {
            None
        };
        self.add_tab_for(command, remote, window, cx);
    }
    fn open_ssh(&mut self, host: remote::Host, window: &mut Window, cx: &mut Context<Self>) {
        self.add_tab_for(None, Some(host), window, cx);
    }
    fn add_tab_for(
        &mut self,
        command: Option<String>,
        remote: Option<remote::Host>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut setup = self.setup.clone();
        if command.is_none()
            && let Some(tab) = self.tabs.get(self.active)
        {
            setup.cwd = tab.context.cwd.clone();
        }
        let launch_setup = setup.clone();
        let launch_remote = remote.clone();
        let launch_command = command.clone();
        let (session, writer, reader) = session_task::SessionTask::start(move || {
            if let Some(host) = &launch_remote {
                remote::ssh_executable(launch_setup.git.as_deref())
                    .and_then(|ssh| remote::terminal_command(host, &ssh, &launch_setup.cwd))
                    .and_then(Session::from_command)
            } else {
                Session::spawn(&launch_setup, launch_command.as_deref())
            }
        });
        let id = self.next_id;
        self.next_id += 1;
        let resize = session.resize.clone();
        let shell_exit = session.exit.clone();
        let command_exit = command.as_ref().map(|_| self.command_exit.clone());
        let ssh_session = remote.is_some();
        let parent = cx.weak_entity();
        let title_parent = parent.clone();
        let config = TerminalConfig {
            cols: 110,
            rows: 30,
            font_family: self.settings.font_family.clone(),
            font_size: px(self.settings.font_size),
            scrollback: self.settings.scrollback,
            line_height_multiplier: 1.25,
            padding: Edges::all(px(18.)),
            colors: ColorPalette::builder()
                .background(0x0e, 0x11, 0x18)
                .foreground(0xdf, 0xe7, 0xf3)
                .cursor(0x7c, 0xb7, 0xff)
                .red(0xf0, 0x86, 0x91)
                .green(0x9a, 0xcf, 0xa0)
                .yellow(0xe9, 0xc4, 0x8a)
                .blue(0x7c, 0xb7, 0xff)
                .magenta(0xc4, 0xa5, 0xef)
                .cyan(0x85, 0xcb, 0xcf)
                .white(0xdf, 0xe7, 0xf3)
                .bright_black(0x8c, 0x9a, 0xb0)
                .build(),
        };
        let terminal = cx.new(|cx| {
            TerminalView::new(writer, reader, config, cx)
                .with_syntax_highlighting(
                    remote.is_none() && setup.profile == "bash" && setup.syntax_highlighting,
                )
                .with_resize_callback(move |cols, rows| {
                    let _ = resize.send(portable_pty::PtySize {
                        cols: cols.min(1000) as u16,
                        rows: rows.min(1000) as u16,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                })
                .with_title_callback(move |window, cx, title| {
                    let _ = title_parent.update(cx, |app, cx| {
                        if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == id)
                            && tab.remote.is_none()
                        {
                            tab.context.update(title);
                        }
                        window.set_window_title("Git-Terminal");
                        cx.notify();
                    });
                })
                .with_exit_callback(move |window, cx| {
                    if let Some(result) = &command_exit {
                        result.store(shell_exit.lock().unwrap_or(1), Ordering::Release);
                    }
                    if ssh_session && shell_exit.lock().is_some_and(|status| status != 0) {
                        let _ = parent.update(cx, |app, cx| {
                            app.error = Some(
                                "SSH connection ended. Review the terminal output and reconnect."
                                    .into(),
                            );
                            cx.notify();
                        });
                        return;
                    }
                    let parent = parent.clone();
                    let handle = window.window_handle();
                    cx.spawn(async move |_, cx| {
                        let _ = handle.update(cx, |_, window, cx| {
                            let _ = parent.update(cx, |app, cx| app.close(id, window, cx));
                        });
                    })
                    .detach();
                })
        });
        terminal.read(cx).focus_handle().clone().focus(window, cx);
        let mut context = TabContext::new(setup.cwd, setup.profile);
        if let Some(host) = &remote {
            context.label = host.name.clone();
            context.profile = "ssh".into();
        }
        self.tabs.push(Tab {
            id,
            context,
            _session: session,
            terminal,
            remote,
        });
        self.active = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active);
        self.profile_menu = false;
        self.error = None;
        cx.notify();
    }
    fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.terminal
                .read(cx)
                .focus_handle()
                .clone()
                .focus(window, cx);
        }
    }
    fn close(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.remove_tab(id);
        if self.tabs.is_empty()
            && self
                .remote
                .transfers
                .iter()
                .any(|t| matches!(t.state.as_str(), "queued" | "running"))
        {
            self.remote.right = true;
        } else if self.tabs.is_empty() {
            window.remove_window();
        } else {
            self.focus(window, cx);
        }
        cx.notify();
    }
    fn remove_tab(&mut self, id: u64) {
        let selected = self.tabs.get(self.active).map(|tab| tab.id);
        self.tabs.retain(|tab| tab.id != id);
        self.active = selected
            .and_then(|id| self.tabs.iter().position(|tab| tab.id == id))
            .unwrap_or_else(|| self.active.min(self.tabs.len().saturating_sub(1)));
        self.tab_scroll.scroll_to_item(self.active);
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if let Some(editor) = &self.remote.editor {
            if matches!(k.key.as_str(), "enter" | "escape")
                && editor.fields.iter().any(|field| field.read(cx).composing())
            {
                return;
            }
            if k.key == "escape" {
                self.remote.editor = None;
                self.focus(window, cx);
                cx.stop_propagation();
                cx.notify();
            } else if k.key == "tab" {
                let n = editor.fields.len();
                let selected = editor
                    .fields
                    .iter()
                    .position(|field| field.read(cx).is_focused(window))
                    .unwrap_or(0);
                let next = (selected + if k.modifiers.shift { n - 1 } else { 1 }) % n;
                editor.fields[next]
                    .read(cx)
                    .focus_handle()
                    .focus(window, cx);
                cx.stop_propagation();
            } else if k.key == "enter" {
                self.save_host_editor(cx);
                cx.stop_propagation();
                if self.remote.editor.is_none() {
                    self.focus(window, cx);
                }
            }
            return;
        }
        let mut handled = true;
        if k.key == "escape" && self.profile_menu {
            self.profile_menu = false;
            self.focus(window, cx);
            cx.notify();
        } else if k.modifiers.control && k.key == "t" {
            self.add_tab(None, window, cx);
        } else if k.modifiers.control && k.key == "w" {
            if let Some(t) = self.tabs.get(self.active) {
                self.close(t.id, window, cx);
            }
        } else if k.modifiers.control && k.modifiers.shift && k.key == "v" {
            if let Some(tab) = self.tabs.get(self.active) {
                tab.terminal
                    .update(cx, |terminal, cx| terminal.paste_clipboard(cx));
            }
        } else if k.modifiers.control && k.key == "tab" && !self.tabs.is_empty() {
            let n = self.tabs.len();
            self.active = (self.active + if k.modifiers.shift { n - 1 } else { 1 }) % n;
            self.tab_scroll.scroll_to_item(self.active);
            self.focus(window, cx);
            cx.notify();
        } else if k.modifiers.control && matches!(k.key.as_str(), "=" | "+" | "-" | "0") {
            self.settings.font_size = if k.key == "0" {
                16.
            } else {
                (self.settings.font_size + if k.key == "-" { -1. } else { 1. }).clamp(8., 48.)
            };
            for tab in &self.tabs {
                tab.terminal.update(cx, |terminal, cx| {
                    let mut config = terminal.config().clone();
                    config.font_size = px(self.settings.font_size);
                    terminal.update_config(config, cx);
                });
            }
            let _ = self.settings.save();
        } else {
            handled = false;
        }
        if handled {
            cx.stop_propagation();
        }
    }
    fn shell(&mut self, profile: &str, window: &mut Window, cx: &mut Context<Self>) {
        match Setup::discover(&self.setup.cwd, profile, self.setup.git.as_deref()) {
            Ok(mut setup) => {
                setup.no_profile = self.setup.no_profile;
                setup.integration = self.setup.integration.clone();
                setup.syntax_highlighting = self.settings.syntax_highlighting;
                setup.git_prompt = self.settings.git_prompt;
                self.setup = setup;
                self.settings.shell = profile.into();
                let _ = self.settings.save();
                self.add_tab_for(None, None, window, cx);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.settings.active_language();
        let show_hosts = self.remote.visible
            && (!self.remote.right || window.viewport_size().width >= px(1050.));
        let host_editor = self.render_host_editor(cx);
        let mut tabs = div()
            .id("tabs")
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll);
        for (index, tab) in self.tabs.iter().enumerate() {
            let id = tab.id;
            let active = index == self.active;
            tabs = tabs.child(
                div()
                    .id(("tab", id))
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .h(px(40.))
                    .min_w(px(148.))
                    .max_w(px(300.))
                    .px_3()
                    .rounded_t_md()
                    .border_b_2()
                    .border_color(rgb(if active { 0x7cb7ff } else { 0x171c26 }))
                    .bg(rgb(if active { 0x252e3f } else { 0x171c26 }))
                    .text_color(rgb(if active { 0xdfe7f3 } else { 0x8c9ab0 }))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x252e3f)).text_color(rgb(0xdfe7f3)))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.active = index;
                        app.focus(window, cx);
                        cx.notify();
                    }))
                    .child(div().text_color(rgb(0x7cb7ff)).child(
                        if tab._session.starting.load(Ordering::Acquire) {
                            "·"
                        } else {
                            "›_"
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(tab.context.label.clone()),
                    )
                    .children(tab.context.branch.as_ref().map(|branch| {
                        div()
                            .max_w(px(110.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(11.))
                            .text_color(rgb(0xc4a5ef))
                            .child(branch.clone())
                    }))
                    .child(
                        div()
                            .id(("close", id))
                            .w(px(24.))
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_size(px(17.))
                            .text_color(rgb(0x8c9ab0))
                            .hover(|s| s.bg(rgb(0x3a455a)).text_color(rgb(0xdfe7f3)))
                            .child("×")
                            .on_click(cx.listener(move |app, _, window, cx| {
                                cx.stop_propagation();
                                app.close(id, window, cx);
                            })),
                    ),
            );
        }
        let mut profiles = div().flex().flex_col().gap_1();
        for profile in if cfg!(windows) {
            vec!["bash", "powershell", "cmd"]
        } else {
            vec!["shell", "bash"]
        } {
            profiles = profiles.child(
                div()
                    .id(SharedString::from(format!("profile-{profile}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x2b3545)))
                    .child(profile_name(profile))
                    .child(if self.setup.profile == profile {
                        "✓"
                    } else {
                        ""
                    })
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.shell(profile, window, cx)),
                    ),
            );
        }
        let terminal = self.tabs.get(self.active).map(|tab| tab.terminal.clone());
        let directory = self
            .tabs
            .get(self.active)
            .map(|tab| {
                tab.remote
                    .as_ref()
                    .map(|host| {
                        format!(
                            "ssh://{}{}",
                            if host.user.is_empty() {
                                String::new()
                            } else {
                                format!("{}@", host.user)
                            },
                            host.destination
                        )
                    })
                    .unwrap_or_else(|| tab.context.cwd.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let profile = self
            .tabs
            .get(self.active)
            .map(|tab| profile_name(&tab.context.profile))
            .unwrap_or("Shell");
        let menu = self.profile_menu.then(|| {
            deferred(
                div()
                    .id("profile-menu")
                    .absolute()
                    .top(px(49.))
                    .right(px(if cfg!(windows) { 148. } else { 10. }))
                    .w(px(240.))
                    .p_2()
                    .rounded_lg()
                    .bg(rgb(0x202838))
                    .border_1()
                    .border_color(rgb(0x354157))
                    .shadow_lg()
                    .occlude()
                    .on_mouse_down_out(cx.listener(|app, _, _, cx| {
                        app.profile_menu = false;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .text_color(rgb(0x8c9ab0))
                            .text_size(px(12.))
                            .child(crate::i18n::translate(
                                &language,
                                "Open tab and set default",
                            )),
                    )
                    .child(profiles)
                    .child(div().h(px(1.)).my_2().bg(rgb(0x354157)))
                    .child(
                        div()
                            .id("language")
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(0x2b3545)))
                            .child(format!(
                                "{}　{}",
                                crate::i18n::translate(&language, "Language"),
                                // The active language is named in its own
                                // language, so the entry stays readable no
                                // matter which language the menu is shown in.
                                language.name()
                            ))
                            .on_click(cx.listener(|app, _, window, cx| {
                                // The menu cycles through the shipped languages.
                                // The list comes from the locale documents, so
                                // it grows by itself when a language is added.
                                let available = crate::i18n::available();
                                let current = app.settings.active_language().code().to_owned();
                                let next = available
                                    .iter()
                                    .find(|entry| entry.code() != current)
                                    .map(|entry| entry.code().to_owned());
                                if let Some(next) = next {
                                    app.settings.language = next;
                                    let _ = app.settings.save();
                                }
                                app.profile_menu = false;
                                app.focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .priority(1)
        });
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(0x0e1118))
            .text_color(rgb(0xdfe7f3))
            .font_family("Segoe UI")
            .text_size(px(13.))
            .capture_key_down(cx.listener(Self::key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .h(px(48.))
                    .flex_shrink_0()
                    .bg(rgb(0x171c26))
                    .child(
                        div()
                            .id("app-icon")
                            .w(px(32.))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .window_control_area(WindowControlArea::Drag)
                            .child(img("git-terminal.png").size(px(22.))),
                    )
                    .child(tabs)
                    .child(
                        div()
                            .id("ssh-toggle")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(rgb(0x85cbcf))
                            .hover(|s| s.bg(rgb(0x252e3f)))
                            .child("SSH")
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.remote.visible = !app.remote.visible;
                                if app.remote.visible && window.viewport_size().width < px(1050.) {
                                    app.remote.right = false;
                                    app.remote.stop_monitor();
                                }
                                cx.notify();
                            })),
                    )
                    .children(
                        (self.remote.selected.is_some() || !self.remote.transfers.is_empty()).then(
                            || {
                                div()
                                    .id("remote-toggle")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .child("⇄")
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        app.remote.right = !app.remote.right;
                                        if !app.remote.right {
                                            app.remote.stop_monitor();
                                        }
                                        cx.notify();
                                    }))
                            },
                        ),
                    )
                    .children(
                        self.tabs
                            .get(self.active)
                            .filter(|tab| {
                                tab.remote.is_some() && tab._session.exit.lock().is_some()
                            })
                            .map(|tab| {
                                let host = tab.remote.clone().unwrap();
                                let id = tab.id;
                                div()
                                    .id("ssh-reconnect")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_color(rgb(0x85cbcf))
                                    .child(crate::i18n::translate(&language, "Reconnect"))
                                    .on_click(cx.listener(move |app, _, window, cx| {
                                        app.open_ssh(host.clone(), window, cx);
                                        app.close(id, window, cx);
                                    }))
                            }),
                    )
                    .child(
                        div()
                            .id("new-tab")
                            .w(px(36.))
                            .h(px(36.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(23.))
                            .text_color(rgb(0x8c9ab0))
                            .rounded_md()
                            .hover(|s| s.bg(rgb(0x252e3f)).text_color(rgb(0xdfe7f3)))
                            .cursor_pointer()
                            .child("+")
                            .on_click(
                                cx.listener(|app, _, window, cx| app.add_tab(None, window, cx)),
                            ),
                    )
                    .child(
                        div()
                            .id("profiles")
                            .w(px(28.))
                            .h(px(36.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .text_color(rgb(0x8c9ab0))
                            .hover(|s| s.bg(rgb(0x252e3f)).text_color(rgb(0xdfe7f3)))
                            .cursor_pointer()
                            .child("⌄")
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.profile_menu = !app.profile_menu;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("window-drag")
                            .w(px(32.))
                            .min_w(px(32.))
                            .h_full()
                            .flex_1()
                            .flex_shrink_0()
                            .window_control_area(WindowControlArea::Drag),
                    )
                    .children(cfg!(windows).then(|| {
                        div()
                            .flex()
                            .h_full()
                            .flex_shrink_0()
                            .child(window_control(
                                "window-minimize",
                                "\u{e921}",
                                WindowControlArea::Min,
                            ))
                            .child(window_control(
                                "window-maximize",
                                if window.is_maximized() {
                                    "\u{e923}"
                                } else {
                                    "\u{e922}"
                                },
                                WindowControlArea::Max,
                            ))
                            .child(window_control(
                                "window-close",
                                "\u{e8bb}",
                                WindowControlArea::Close,
                            ))
                    })),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|e| div().p_2().text_color(rgb(0xffb4a4)).child(e.clone())),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .children(show_hosts.then(|| self.render_remote_sidebar(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .children(terminal),
                    )
                    .children(self.remote.right.then(|| self.render_remote_panel(cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_4()
                    .h(px(30.))
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(rgb(0x202838))
                    .text_size(px(11.))
                    .text_color(rgb(0x8c9ab0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(directory),
                    )
                    .child(div().flex_shrink_0().child(crate::i18n::translate(
                        &language,
                        "New Ctrl+T    Close Ctrl+W",
                    )))
                    .child(div().flex_shrink_0().child(profile)),
            )
            .children(menu)
            .children(host_editor)
    }
}

fn window_control(id: &'static str, glyph: &'static str, area: WindowControlArea) -> Stateful<Div> {
    let close = matches!(area, WindowControlArea::Close);
    div()
        .id(id)
        .w(px(46.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family("Segoe MDL2 Assets")
        .text_size(px(10.))
        .text_color(rgb(0xdfe7f3))
        .window_control_area(area)
        .hover(move |s| s.bg(rgb(if close { 0xc42b1c } else { 0x303a4b })))
        .child(glyph)
}

fn run() -> Result<u32> {
    let options = Options::parse();
    let mut settings = Settings::load();
    if let Some(Action::Remote { action }) = &options.action {
        remote_cli::run(action, options.git.as_deref())?;
        return Ok(0);
    }
    if let Some(Action::Language { action }) = &options.action {
        match action {
            LanguageAction::Get => {
                let language = settings.active_language();
                println!(
                    "{}",
                    serde_json::json!({
                        "product":"git-terminal",
                        "configured":settings.language,
                        "language":language.code(),
                        "name":language.name(),
                        "english_name":language.english_name(),
                        "supported":crate::i18n::available()
                            .iter()
                            .map(|entry| entry.code().to_owned())
                            .collect::<Vec<_>>(),
                    })
                );
            }
            LanguageAction::Set { language } => {
                // The name is validated before it is stored, so a typo cannot
                // leave the configuration pointing at a language that does not
                // exist.
                let resolved = crate::i18n::resolve(language).ok_or_else(|| {
                    anyhow::anyhow!("unsupported language; use one of the listed languages or auto")
                })?;
                settings.language = crate::i18n::canonical(language);
                settings.save()?;
                println!(
                    "{}",
                    serde_json::json!({
                        "product":"git-terminal",
                        "configured":settings.language,
                        "language":resolved.code(),
                        "name":resolved.name(),
                    })
                );
            }
            LanguageAction::List => {
                for entry in crate::i18n::available() {
                    println!(
                        "{}\t{}\t{}",
                        entry.code(),
                        entry.name(),
                        entry.english_name()
                    );
                }
                println!(
                    "auto\t{}",
                    crate::i18n::translate(
                        &settings.active_language(),
                        "Follow the device language"
                    )
                );
            }
        }
        return Ok(0);
    }
    let cwd = if options.cd_to_home {
        PathBuf::from(
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .ok_or_else(|| anyhow::anyhow!("Home directory is unavailable"))?,
        )
    } else {
        options.cwd.clone().unwrap_or(std::env::current_dir()?)
    };
    let profile = if options.shell == "auto" {
        &settings.shell
    } else {
        &options.shell
    };
    let mut setup = Setup::discover(&cwd, profile, options.git.as_deref())?;
    let remote_host = options
        .ssh
        .as_deref()
        .map(remote_cli::resolve)
        .transpose()?;
    setup.no_profile = options.no_profile;
    if options.no_shell_integration {
        setup.integration = None;
    }
    let supported =
        setup.integration.is_some() && matches!(setup.profile.as_str(), "bash" | "powershell");
    setup.syntax_highlighting = settings.syntax_highlighting && supported;
    setup.git_prompt = settings.git_prompt && supported;
    if options.print_setup {
        println!("{}", serde_json::to_string_pretty(&setup)?);
        return Ok(0);
    }
    let command_exit = Arc::new(AtomicU32::new(0));
    let result_exit = command_exit.clone();
    let view_exit = command_exit.clone();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            cx.set_window_appearance(Some(WindowAppearance::Dark));
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1120.), px(720.)), cx);
            let result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(560.), px(340.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Git-Terminal".into()),
                        appears_transparent: cfg!(windows),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let command_exit = view_exit.clone();
                    cx.new(|cx| {
                        Desktop::new(
                            setup,
                            settings,
                            options.command,
                            remote_host,
                            command_exit,
                            window,
                            cx,
                        )
                    })
                },
            );
            if let Err(error) = result {
                result_exit.store(1, Ordering::Release);
                eprintln!("Git-Terminal: {error}");
                cx.quit();
            } else {
                cx.activate(true);
            }
        });
    Ok(command_exit.load(Ordering::Acquire))
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code as i32),
        Err(error) => {
            eprintln!("Git-Terminal: {error:#}");
            std::process::exit(1);
        }
    }
}
