// Copyright 2026 git-next contributors. MIT license.
//! Small native, IME-aware single-line field used by host configuration.
use gpui::*;
use std::ops::Range;

pub struct Field {
    pub value: String,
    focus: FocusHandle,
    selected: Range<usize>,
    marked: Option<Range<usize>>,
    bounds: Bounds<Pixels>,
    scroll_x: Pixels,
}
impl Field {
    pub fn new(value: &str, cx: &mut Context<Self>) -> Self {
        let end = value.encode_utf16().count();
        Self {
            value: value.into(),
            focus: cx.focus_handle(),
            selected: end..end,
            marked: None,
            bounds: Bounds::default(),
            scroll_x: px(0.),
        }
    }
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus.focus(window, cx);
    }
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }
    pub fn focus_handle(&self) -> FocusHandle {
        self.focus.clone()
    }
    pub fn composing(&self) -> bool {
        self.marked.is_some()
    }
    fn replace(&mut self, range: Range<usize>, text: &str) {
        let mut units: Vec<u16> = self.value.encode_utf16().collect();
        let start = range.start.min(units.len());
        let end = range.end.max(start).min(units.len());
        units.splice(
            start..end,
            text.replace(['\r', '\n', '\0'], "").encode_utf16(),
        );
        self.value = String::from_utf16_lossy(&units);
        let end = start + text.encode_utf16().count();
        let end = end.min(units.len());
        self.selected = end..end;
        self.marked = None;
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let count = self.value.encode_utf16().count();
        let mut handled = true;
        match key.key.as_str() {
            "a" if key.modifiers.control || key.modifiers.platform => self.selected = 0..count,
            "c" if key.modifiers.control || key.modifiers.platform => {
                let units: Vec<u16> = self.value.encode_utf16().collect();
                cx.write_to_clipboard(ClipboardItem::new_string(String::from_utf16_lossy(
                    &units[self.selected.clone()],
                )));
            }
            "v" if key.modifiers.control || key.modifiers.platform => {
                if let Some(text) = cx.read_from_clipboard().and_then(|s| s.text()) {
                    self.replace(self.selected.clone(), &text);
                }
            }
            "backspace" => {
                let mut range = self.selected.clone();
                if range.is_empty() {
                    let units: Vec<u16> = self.value.encode_utf16().collect();
                    range.start = range.start.saturating_sub(1);
                    if units
                        .get(range.start)
                        .is_some_and(|v| (0xdc00..=0xdfff).contains(v))
                    {
                        range.start = range.start.saturating_sub(1);
                    }
                }
                self.replace(range, "");
            }
            "delete" => {
                let mut range = self.selected.clone();
                if range.is_empty() {
                    let units: Vec<u16> = self.value.encode_utf16().collect();
                    range.end = (range.end + 1).min(count);
                    if units
                        .get(range.end.saturating_sub(1))
                        .is_some_and(|v| (0xd800..=0xdbff).contains(v))
                    {
                        range.end = (range.end + 1).min(count);
                    }
                }
                self.replace(range, "");
            }
            "home" => self.selected = 0..0,
            "end" => self.selected = count..count,
            "left" | "right" => {
                let units: Vec<u16> = self.value.encode_utf16().collect();
                let mut end = if key.key == "left" {
                    self.selected.start.saturating_sub(1)
                } else {
                    (self.selected.end + 1).min(count)
                };
                if units
                    .get(end)
                    .is_some_and(|v| (0xdc00..=0xdfff).contains(v))
                {
                    end = if key.key == "left" {
                        end.saturating_sub(1)
                    } else {
                        (end + 1).min(count)
                    };
                }
                self.selected = end..end;
            }
            _ => handled = false,
        }
        if handled {
            cx.stop_propagation();
            window.notify_text_input_changed(&self.focus);
            cx.notify();
        }
    }
}

impl Render for Field {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .id("field")
            .relative()
            .h(px(34.))
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(rgb(if focused { 0x7cb7ff } else { 0x354157 }))
            .bg(rgb(0x0e1118))
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|field, event: &MouseDownEvent, window, cx| {
                    field.focus(window, cx);
                    let shaped = shape(&field.value, window);
                    let byte = shaped.closest_index_for_x(
                        event.position.x - field.bounds.left() - px(8.) + field.scroll_x,
                    );
                    let end = field.value[..byte].encode_utf16().count();
                    field.selected = if event.click_count >= 2 {
                        0..field.value.encode_utf16().count()
                    } else {
                        end..end
                    };
                    window.notify_text_input_changed(&field.focus);
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(Self::key))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        let (text, selected, scroll) = entity.update(cx, |field, _| {
                            field.bounds = bounds;
                            let units: Vec<u16> = field.value.encode_utf16().collect();
                            let prefix = String::from_utf16_lossy(
                                &units[..field.selected.end.min(units.len())],
                            );
                            let caret_x = shape(&prefix, window).width;
                            let available = (bounds.size.width - px(16.)).max(px(1.));
                            if caret_x < field.scroll_x {
                                field.scroll_x = caret_x;
                            }
                            if caret_x > field.scroll_x + available {
                                field.scroll_x = caret_x - available;
                            }
                            (field.value.clone(), field.selected.clone(), field.scroll_x)
                        });
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, entity.clone()),
                            cx,
                        );
                        let shaped = shape(&text, window);
                        let units: Vec<u16> = text.encode_utf16().collect();
                        let prefix =
                            String::from_utf16_lossy(&units[..selected.start.min(units.len())]);
                        let through =
                            String::from_utf16_lossy(&units[..selected.end.min(units.len())]);
                        let left = bounds.left() + px(8.) - scroll;
                        let y = bounds.top() + px(7.);
                        let start = shaped.x_for_index(prefix.len());
                        let end = shaped.x_for_index(through.len());
                        if focused && !selected.is_empty() {
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(left + start, y),
                                    size: size(end - start, px(20.)),
                                },
                                rgb(0x2a476a),
                            ));
                        }
                        let _ = shaped.paint(
                            point(left, y),
                            px(20.),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        if focused {
                            window.paint_quad(fill(
                                Bounds {
                                    origin: point(left + end, y),
                                    size: size(px(1.), px(20.)),
                                },
                                rgb(0x7cb7ff),
                            ));
                        }
                    },
                )
                .size_full(),
            )
    }
}

fn shape(text: &str, window: &Window) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: Font {
            family: "Segoe UI".into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        },
        color: rgb(0xdfe7f3).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(text.to_owned().into(), px(13.), &[run], None)
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let units: Vec<u16> = self.value.encode_utf16().collect();
        let range = range.start.min(units.len())..range.end.min(units.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&units[range]))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selected.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone()
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(
            range
                .or_else(|| self.marked.clone())
                .unwrap_or(self.selected.clone()),
            text,
        );
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or(self.selected.clone());
        let start = range.start;
        self.replace(range, text);
        self.marked = Some(start..self.selected.end);
        if let Some(range) = selected {
            self.selected = start + range.start..start + range.end;
        }
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let units: Vec<u16> = self.value.encode_utf16().collect();
        let prefix = String::from_utf16_lossy(&units[..range.end.min(units.len())]);
        Some(Bounds {
            origin: point(
                self.bounds.left() + px(8.) + shape(&prefix, window).width - self.scroll_x,
                self.bounds.top() + px(7.),
            ),
            size: size(px(1.), px(20.)),
        })
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.selected.end)
    }
}
