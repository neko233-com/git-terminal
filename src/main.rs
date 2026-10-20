// Copyright 2026 git-next contributors. MIT license.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod session;
mod settings;

use anyhow::Result;
use clap::{Parser, Subcommand};
use gpui::*;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use session::{Session, Setup};
use settings::Settings;
use std::path::PathBuf;

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
    #[arg(long,default_value="auto",value_parser=["auto","bash","powershell","cmd","shell"])]
    shell: String,
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
    Language {
        #[command(subcommand)]
        action: LanguageAction,
    },
}
#[derive(Subcommand, Clone)]
enum LanguageAction {
    Get,
    Set {
        #[arg(value_parser=["auto","chinese","english"])]
        language: String,
    },
}

struct Tab {
    id: u64,
    label: String,
    _session: Session,
    terminal: Entity<TerminalView>,
}
struct Desktop {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    setup: Setup,
    settings: Settings,
    error: Option<String>,
}

impl Desktop {
    fn new(
        setup: Setup,
        settings: Settings,
        command: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut desktop = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            setup,
            settings,
            error: None,
        };
        desktop.add_tab(command, window, cx);
        desktop
    }
    fn add_tab(&mut self, command: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let result = Session::spawn(&self.setup, command.as_deref());
        let (session, writer, reader) = match result {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let id = self.next_id;
        self.next_id += 1;
        let master = session.master.clone();
        let parent = cx.weak_entity();
        let title_parent = parent.clone();
        let config = TerminalConfig {
            cols: 110,
            rows: 30,
            font_family: self.settings.font_family.clone(),
            font_size: px(self.settings.font_size),
            scrollback: self.settings.scrollback,
            line_height_multiplier: 1.2,
            padding: Edges::all(px(12.)),
            colors: ColorPalette::builder()
                .background(0x12, 0x15, 0x1b)
                .foreground(0xd9, 0xe1, 0xeb)
                .cursor(0x91, 0xbc, 0xff)
                .build(),
        };
        let terminal = cx.new(|cx| {
            TerminalView::new(writer, reader, config, cx)
                .with_resize_callback(move |cols, rows| {
                    if let Some(master) = master.lock().as_ref() {
                        let _ = master.resize(portable_pty::PtySize {
                            cols: cols.min(1000) as u16,
                            rows: rows.min(1000) as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                })
                .with_title_callback(move |window, cx, title| {
                    let title: String =
                        title.chars().filter(|c| !c.is_control()).take(80).collect();
                    let _ = title_parent.update(cx, |app, cx| {
                        if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == id) {
                            tab.label = title;
                        }
                        window.set_window_title("Git-Terminal");
                        cx.notify();
                    });
                })
                .with_exit_callback(move |window, cx| {
                    let parent = parent.clone();
                    let handle = window.window_handle();
                    cx.spawn(async move |_, cx| {
                        let _ = parent.update(cx, |app, cx| {
                            app.tabs.retain(|t| t.id != id);
                            app.active = app.active.min(app.tabs.len().saturating_sub(1));
                            if app.tabs.is_empty() {
                                let _ = handle.update(cx, |_, window, _| window.remove_window());
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                })
        });
        terminal.read(cx).focus_handle().clone().focus(window, cx);
        self.tabs.push(Tab {
            id,
            label: self.setup.profile.clone(),
            _session: session,
            terminal,
        });
        self.active = self.tabs.len() - 1;
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
        self.tabs.retain(|t| t.id != id);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        if self.tabs.is_empty() {
            window.remove_window();
        } else {
            self.focus(window, cx);
        }
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let mut handled = true;
        if k.modifiers.control && k.modifiers.shift && k.key == "t" {
            self.add_tab(None, window, cx);
        } else if k.modifiers.control && k.modifiers.shift && k.key == "w" {
            if let Some(t) = self.tabs.get(self.active) {
                self.close(t.id, window, cx);
            }
        } else if k.modifiers.control && k.key == "tab" && !self.tabs.is_empty() {
            let n = self.tabs.len();
            self.active = (self.active + if k.modifiers.shift { n - 1 } else { 1 }) % n;
            self.focus(window, cx);
            cx.notify();
        } else if k.modifiers.control && matches!(k.key.as_str(), "=" | "+" | "-" | "0") {
            self.settings.font_size = if k.key == "0" {
                14.
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
            Ok(setup) => {
                self.setup = setup;
                self.add_tab(None, window, cx);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

impl Render for Desktop {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chinese = self.settings.chinese();
        let mut tabs = div()
            .flex()
            .items_center()
            .gap_1()
            .flex_1()
            .overflow_hidden();
        for (index, tab) in self.tabs.iter().enumerate() {
            let id = tab.id;
            let active = index == self.active;
            tabs = tabs.child(
                div()
                    .id(("tab", id))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(if active { 0x283342 } else { 0x1a202a }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.active = index;
                        app.focus(window, cx);
                        cx.notify();
                    }))
                    .child(div().max_w_64().overflow_hidden().child(tab.label.clone()))
                    .child(
                        div()
                            .id(("close", id))
                            .px_1()
                            .child("×")
                            .on_click(cx.listener(move |app, _, window, cx| {
                                cx.stop_propagation();
                                app.close(id, window, cx);
                            })),
                    ),
            );
        }
        let mut profiles = div().flex().items_center().gap_1();
        for profile in if cfg!(windows) {
            vec!["bash", "powershell", "cmd"]
        } else {
            vec!["shell", "bash"]
        } {
            profiles = profiles.child(
                div()
                    .id(SharedString::from(format!("profile-{profile}")))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x2b3545)))
                    .child(match profile {
                        "powershell" => "PowerShell",
                        "cmd" => "CMD",
                        "bash" => "Bash",
                        _ => "Shell",
                    })
                    .on_click(
                        cx.listener(move |app, _, window, cx| app.shell(profile, window, cx)),
                    ),
            );
        }
        let terminal = self.tabs.get(self.active).map(|tab| tab.terminal.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x12151b))
            .text_color(rgb(0xd9e1eb))
            .font_family("Segoe UI")
            .text_size(px(12.))
            .on_key_down(cx.listener(Self::key))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .bg(rgb(0x191f29))
                    .child(tabs)
                    .child(
                        div()
                            .id("new-tab")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .child("+")
                            .on_click(
                                cx.listener(|app, _, window, cx| app.add_tab(None, window, cx)),
                            ),
                    )
                    .child(profiles)
                    .child(
                        div()
                            .id("language")
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .child(if chinese { "中文" } else { "EN" })
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.settings.language = if app.settings.chinese() {
                                    "english"
                                } else {
                                    "chinese"
                                }
                                .into();
                                let _ = app.settings.save();
                                cx.notify();
                            })),
                    ),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|e| div().p_2().text_color(rgb(0xffb4a4)).child(e.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .children(terminal),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_3()
                    .py_1()
                    .bg(rgb(0x191f29))
                    .text_color(rgb(0x8995a7))
                    .child(if self.setup.git.is_some() {
                        "git-next · GPUI Fast"
                    } else {
                        if chinese {
                            "git-next 未安装 · GPUI Fast"
                        } else {
                            "git-next unavailable · GPUI Fast"
                        }
                    })
                    .child(if chinese {
                        "新标签 Ctrl+Shift+T · 复制/粘贴 Ctrl+Shift+C/V · 缩放 Ctrl +/-"
                    } else {
                        "New tab Ctrl+Shift+T · Copy/paste Ctrl+Shift+C/V · Zoom Ctrl +/-"
                    }),
            )
    }
}

fn run() -> Result<()> {
    let options = Options::parse();
    let mut settings = Settings::load();
    if let Some(Action::Language { action }) = &options.action {
        if let LanguageAction::Set { language } = action {
            settings.language = language.clone();
            settings.save()?;
        }
        println!(
            "{}",
            serde_json::json!({"product":"git-terminal","configured":settings.language,"language":if settings.chinese(){"chinese"}else{"english"}})
        );
        return Ok(());
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
    setup.no_profile = options.no_profile;
    if options.print_setup {
        println!("{}", serde_json::to_string_pretty(&setup)?);
        return Ok(());
    }
    gpui_platform::application().run(move |cx| {
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
                titlebar: Some(TitlebarOptions {
                    title: Some("Git-Terminal".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| Desktop::new(setup, settings, options.command, window, cx))
            },
        );
        if let Err(error) = result {
            eprintln!("Git-Terminal: {error}");
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Git-Terminal: {error:#}");
        std::process::exit(1);
    }
}
