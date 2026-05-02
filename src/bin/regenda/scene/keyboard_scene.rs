use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};

// Layout in native (Ferrari) px; everything is scaled at draw-time so the
// same code lays out cleanly on Move (954w) and Ferrari (1620w).
const ROW_HEIGHT: u32 = 110;
const KEY_GAP: u32 = 8;
const SIDE_MARGIN: u32 = 24;
const ROW_GAP: u32 = 10;
const PREVIEW_HEIGHT: u32 = 120;
const HEADER_HEIGHT: u32 = 80;

#[derive(Clone, Debug)]
struct Key {
    label_lower: String,
    label_upper: String,
    action: KeyAction,
    /// Width as a multiple of unit width (1.0 = single letter cell).
    width_units: f32,
    rect: mxcfb_rect,
}

#[derive(Clone, Debug)]
enum KeyAction {
    Char,
    Backspace,
    Space,
    Shift,
    Caps,
    Done,
    Cancel,
    Clear,
    Enter,
}

impl Key {
    fn letter(c: &str, upper: &str) -> Self {
        Key {
            label_lower: c.to_string(),
            label_upper: upper.to_string(),
            action: KeyAction::Char,
            width_units: 1.0,
            rect: mxcfb_rect::default(),
        }
    }
    fn fixed(label: &str, action: KeyAction, width_units: f32) -> Self {
        Key {
            label_lower: label.to_string(),
            label_upper: label.to_string(),
            action,
            width_units,
            rect: mxcfb_rect::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardOutcome {
    Editing,
    Done,
    Cancelled,
}

/// On-screen QWERTY keyboard, composed by EditEventScene as an overlay.
/// Not a standalone Scene — scene transitions destroy state, and the host
/// form needs to keep its other field values while the user types into one.
pub struct Keyboard {
    buffer: String,
    shift: bool,
    caps: bool,
    rows: Vec<Vec<Key>>,
    label: String,
    /// Multi-line: when true, Enter inserts a newline; when false, Enter
    /// acts as Done. Description fields are multi-line; everything else
    /// stays single-line.
    multiline: bool,
    needs_redraw: bool,
    outcome: KeyboardOutcome,
}

impl Keyboard {
    pub fn new(initial: &str, label: &str, multiline: bool, strings: &'static Strings) -> Self {
        Keyboard {
            buffer: initial.to_string(),
            shift: false,
            caps: false,
            rows: build_layout(strings),
            label: label.to_string(),
            multiline,
            needs_redraw: true,
            outcome: KeyboardOutcome::Editing,
        }
    }

    pub fn value(&self) -> &str {
        &self.buffer
    }

    pub fn outcome(&self) -> KeyboardOutcome {
        self.outcome.clone()
    }

    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    pub fn on_input(&mut self, event: InputEvent) {
        if let InputEvent::MultitouchEvent {
            event: MultitouchEvent::Release { finger },
        } = event
        {
            let pos = finger.pos;
            for row in &self.rows {
                for key in row {
                    if Canvas::is_hitting(pos, key.rect) {
                        self.handle_press(key.clone());
                        return;
                    }
                }
            }
        }
    }

    fn handle_press(&mut self, key: Key) {
        match key.action {
            KeyAction::Char => {
                let label = if self.shift || self.caps {
                    &key.label_upper
                } else {
                    &key.label_lower
                };
                self.buffer.push_str(label);
                if self.shift {
                    self.shift = false;
                }
            }
            KeyAction::Backspace => {
                self.buffer.pop();
            }
            KeyAction::Space => {
                self.buffer.push(' ');
            }
            KeyAction::Shift => {
                self.shift = !self.shift;
            }
            KeyAction::Caps => {
                self.caps = !self.caps;
                self.shift = false;
            }
            KeyAction::Done => {
                self.outcome = KeyboardOutcome::Done;
            }
            KeyAction::Cancel => {
                self.outcome = KeyboardOutcome::Cancelled;
            }
            KeyAction::Clear => {
                self.buffer.clear();
            }
            KeyAction::Enter => {
                if self.multiline {
                    self.buffer.push('\n');
                } else {
                    self.outcome = KeyboardOutcome::Done;
                }
            }
        }
        self.needs_redraw = true;
    }

    /// Draw the full keyboard overlay (header + buffer preview + key grid).
    /// Call after `canvas.clear()` if the host form was previously visible —
    /// the keyboard owns the whole screen while up.
    pub fn draw(&mut self, canvas: &mut Canvas) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        let dw = canvas.display_width();
        let dh = crate::display_height();

        canvas.clear();

        // === Header ===
        let hdr_h = crate::scale_u32(HEADER_HEIGHT);
        canvas.fill_rect(
            Point2 { x: Some(0), y: Some(0) },
            Vector2 { x: dw, y: hdr_h },
            color::HEADER_BG,
        );
        canvas.draw_text_colored(
            Point2 {
                x: crate::scale_f32(40.0),
                y: crate::scale_f32(20.0),
            },
            &self.label,
            crate::scale_f32(40.0),
            color::WHITE,
        );

        // === Buffer preview ===
        let preview_top = hdr_h + crate::scale_u32(20);
        let preview_h = crate::scale_u32(PREVIEW_HEIGHT);
        canvas.draw_rect(
            Point2 {
                x: Some(crate::scale_u32(40) as i32),
                y: Some(preview_top as i32),
            },
            Vector2 {
                x: dw - 2 * crate::scale_u32(40),
                y: preview_h,
            },
            2,
        );
        let display = if self.buffer.is_empty() {
            "_".to_string()
        } else if self.buffer.chars().count() > 70 {
            // Right-align long buffers so the caret end is always visible.
            let take: String = self
                .buffer
                .chars()
                .rev()
                .take(70)
                .collect::<Vec<char>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{}", take)
        } else {
            self.buffer.clone()
        };
        canvas.draw_text_colored(
            Point2 {
                x: crate::scale_f32(60.0),
                y: (preview_top + crate::scale_u32(20)) as f32,
            },
            &display,
            crate::scale_f32(40.0),
            color::BLACK,
        );

        // === Keys ===
        let kb_top = compute_keyboard_top(dh, self.rows.len());
        let row_h = crate::scale_u32(ROW_HEIGHT);
        let key_gap = crate::scale_u32(KEY_GAP);
        let row_gap = crate::scale_u32(ROW_GAP);
        let side = crate::scale_u32(SIDE_MARGIN);
        let usable_w = dw.saturating_sub(2 * side);

        for (row_idx, row) in self.rows.iter_mut().enumerate() {
            let total_units: f32 = row.iter().map(|k| k.width_units).sum();
            let total_gaps = row.len().saturating_sub(1) as u32 * key_gap;
            let unit_w = if total_units > 0.0 {
                ((usable_w.saturating_sub(total_gaps)) as f32 / total_units) as u32
            } else {
                0
            };
            let row_y = kb_top + (row_idx as u32) * (row_h + row_gap);
            let mut x = side;
            for key in row.iter_mut() {
                let kw = (unit_w as f32 * key.width_units) as u32;
                let rect = mxcfb_rect {
                    top: row_y,
                    left: x,
                    width: kw,
                    height: row_h,
                };
                key.rect = rect;
                let is_active = match key.action {
                    KeyAction::Shift => self.shift,
                    KeyAction::Caps => self.caps,
                    _ => false,
                };
                let bg = if is_active { color::DARK_GRAY } else { color::WHITE };
                let fg = if is_active { color::WHITE } else { color::BLACK };
                canvas.fill_rect(
                    Point2 {
                        x: Some(rect.left as i32),
                        y: Some(rect.top as i32),
                    },
                    Vector2 { x: rect.width, y: rect.height },
                    bg,
                );
                canvas.draw_rect(
                    Point2 {
                        x: Some(rect.left as i32),
                        y: Some(rect.top as i32),
                    },
                    Vector2 { x: rect.width, y: rect.height },
                    2,
                );
                let label = if matches!(key.action, KeyAction::Char)
                    && (self.shift || self.caps)
                {
                    &key.label_upper
                } else {
                    &key.label_lower
                };
                let font = crate::scale_f32(label_font_size(&key.action));
                let m = canvas.measure_text(label, font);
                let lx = rect.left + (rect.width.saturating_sub(m.width)) / 2;
                let ly = rect.top + (rect.height.saturating_sub(m.height)) / 2;
                canvas.draw_text_colored(
                    Point2 { x: lx as f32, y: ly as f32 },
                    label,
                    font,
                    fg,
                );
                x += kw + key_gap;
            }
        }

        canvas.update_full();
    }
}

fn compute_keyboard_top(dh: u32, rows: usize) -> u32 {
    let row_h = crate::scale_u32(ROW_HEIGHT);
    let row_gap = crate::scale_u32(ROW_GAP);
    let n = rows as u32;
    let total = n * row_h + n.saturating_sub(1) * row_gap;
    let bottom_pad = crate::scale_u32(40);
    dh.saturating_sub(total + bottom_pad)
}

fn label_font_size(action: &KeyAction) -> f32 {
    match action {
        KeyAction::Space
        | KeyAction::Backspace
        | KeyAction::Shift
        | KeyAction::Caps
        | KeyAction::Done
        | KeyAction::Cancel
        | KeyAction::Clear
        | KeyAction::Enter => 28.0,
        KeyAction::Char => 40.0,
    }
}

fn build_layout(strings: &'static Strings) -> Vec<Vec<Key>> {
    let row_numbers = vec![
        Key::letter("1", "!"),
        Key::letter("2", "@"),
        Key::letter("3", "#"),
        Key::letter("4", "$"),
        Key::letter("5", "%"),
        Key::letter("6", "^"),
        Key::letter("7", "&"),
        Key::letter("8", "*"),
        Key::letter("9", "("),
        Key::letter("0", ")"),
    ];
    let row1 = vec![
        Key::letter("q", "Q"),
        Key::letter("w", "W"),
        Key::letter("e", "E"),
        Key::letter("r", "R"),
        Key::letter("t", "T"),
        Key::letter("y", "Y"),
        Key::letter("u", "U"),
        Key::letter("i", "I"),
        Key::letter("o", "O"),
        Key::letter("p", "P"),
    ];
    let row2 = vec![
        Key::letter("a", "A"),
        Key::letter("s", "S"),
        Key::letter("d", "D"),
        Key::letter("f", "F"),
        Key::letter("g", "G"),
        Key::letter("h", "H"),
        Key::letter("j", "J"),
        Key::letter("k", "K"),
        Key::letter("l", "L"),
        Key::letter(":", ";"),
    ];
    let row3 = vec![
        Key::fixed(strings.shift, KeyAction::Shift, 1.5),
        Key::letter("z", "Z"),
        Key::letter("x", "X"),
        Key::letter("c", "C"),
        Key::letter("v", "V"),
        Key::letter("b", "B"),
        Key::letter("n", "N"),
        Key::letter("m", "M"),
        Key::letter(",", "<"),
        Key::letter(".", ">"),
        Key::fixed(strings.backspace, KeyAction::Backspace, 1.5),
    ];
    let row4 = vec![
        Key::fixed(strings.caps, KeyAction::Caps, 1.5),
        Key::letter("@", "@"),
        Key::letter("-", "_"),
        Key::letter("/", "?"),
        Key::fixed(strings.space, KeyAction::Space, 4.0),
        Key::letter("'", "\""),
        Key::fixed(strings.cancel, KeyAction::Cancel, 1.5),
        Key::fixed(strings.done, KeyAction::Done, 1.5),
    ];

    vec![row_numbers, row1, row2, row3, row4]
}
