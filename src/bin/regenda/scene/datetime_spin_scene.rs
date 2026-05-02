use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpinOutcome {
    Editing,
    Done,
    Cancelled,
}

/// Tap-spin date/time picker. Six columns: year | month | day | hour |
/// minute | am/pm. The user nudges values one click at a time. All-day mode
/// hides the last three columns.
pub struct DateTimeSpin {
    label: String,
    dt: NaiveDateTime,
    all_day: bool,
    strings: &'static Strings,
    outcome: SpinOutcome,
    needs_redraw: bool,

    year_up: mxcfb_rect,
    year_dn: mxcfb_rect,
    month_up: mxcfb_rect,
    month_dn: mxcfb_rect,
    day_up: mxcfb_rect,
    day_dn: mxcfb_rect,
    hour_up: mxcfb_rect,
    hour_dn: mxcfb_rect,
    min_up: mxcfb_rect,
    min_dn: mxcfb_rect,
    ampm_toggle: mxcfb_rect,
    done_hitbox: mxcfb_rect,
    cancel_hitbox: mxcfb_rect,
}

impl DateTimeSpin {
    pub fn new(
        label: &str,
        initial: NaiveDateTime,
        all_day: bool,
        strings: &'static Strings,
    ) -> Self {
        DateTimeSpin {
            label: label.to_string(),
            dt: initial,
            all_day,
            strings,
            outcome: SpinOutcome::Editing,
            needs_redraw: true,
            year_up: mxcfb_rect::default(),
            year_dn: mxcfb_rect::default(),
            month_up: mxcfb_rect::default(),
            month_dn: mxcfb_rect::default(),
            day_up: mxcfb_rect::default(),
            day_dn: mxcfb_rect::default(),
            hour_up: mxcfb_rect::default(),
            hour_dn: mxcfb_rect::default(),
            min_up: mxcfb_rect::default(),
            min_dn: mxcfb_rect::default(),
            ampm_toggle: mxcfb_rect::default(),
            done_hitbox: mxcfb_rect::default(),
            cancel_hitbox: mxcfb_rect::default(),
        }
    }

    pub fn value(&self) -> NaiveDateTime {
        self.dt
    }

    pub fn outcome(&self) -> SpinOutcome {
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
            if Canvas::is_hitting(pos, self.done_hitbox) {
                self.outcome = SpinOutcome::Done;
                return;
            }
            if Canvas::is_hitting(pos, self.cancel_hitbox) {
                self.outcome = SpinOutcome::Cancelled;
                return;
            }
            if Canvas::is_hitting(pos, self.year_up) {
                self.adjust_year(1);
            } else if Canvas::is_hitting(pos, self.year_dn) {
                self.adjust_year(-1);
            } else if Canvas::is_hitting(pos, self.month_up) {
                self.adjust_month(1);
            } else if Canvas::is_hitting(pos, self.month_dn) {
                self.adjust_month(-1);
            } else if Canvas::is_hitting(pos, self.day_up) {
                self.adjust_day(1);
            } else if Canvas::is_hitting(pos, self.day_dn) {
                self.adjust_day(-1);
            } else if !self.all_day {
                if Canvas::is_hitting(pos, self.hour_up) {
                    self.adjust_hour(1);
                } else if Canvas::is_hitting(pos, self.hour_dn) {
                    self.adjust_hour(-1);
                } else if Canvas::is_hitting(pos, self.min_up) {
                    self.adjust_minute(5);
                } else if Canvas::is_hitting(pos, self.min_dn) {
                    self.adjust_minute(-5);
                } else if Canvas::is_hitting(pos, self.ampm_toggle) {
                    self.adjust_hour(12);
                }
            }
        }
    }

    fn adjust_year(&mut self, delta: i32) {
        let new_year = self.dt.year() + delta;
        if !(1970..=2100).contains(&new_year) {
            return;
        }
        if let Some(new) = self.dt.with_year(new_year) {
            self.dt = new;
        } else {
            // Feb 29 in a non-leap year — fall back to Feb 28.
            if let Some(date) = NaiveDate::from_ymd_opt(new_year, self.dt.month(), 28) {
                self.dt = date.and_time(self.dt.time());
            }
        }
        self.needs_redraw = true;
    }

    fn adjust_month(&mut self, delta: i32) {
        let total = self.dt.month() as i32 + delta;
        let (new_year, new_month) = if total < 1 {
            (self.dt.year() - 1, 12)
        } else if total > 12 {
            (self.dt.year() + 1, 1)
        } else {
            (self.dt.year(), total as u32)
        };
        let day = self.dt.day().min(days_in_month(new_year, new_month));
        if let Some(date) = NaiveDate::from_ymd_opt(new_year, new_month, day) {
            self.dt = date.and_time(self.dt.time());
            self.needs_redraw = true;
        }
    }

    fn adjust_day(&mut self, delta: i64) {
        if let Some(new) = self.dt.checked_add_signed(chrono::Duration::days(delta)) {
            self.dt = new;
            self.needs_redraw = true;
        }
    }

    fn adjust_hour(&mut self, delta: i64) {
        if let Some(new) = self.dt.checked_add_signed(chrono::Duration::hours(delta)) {
            self.dt = new;
            self.needs_redraw = true;
        }
    }

    fn adjust_minute(&mut self, delta: i64) {
        if let Some(new) = self.dt.checked_add_signed(chrono::Duration::minutes(delta)) {
            self.dt = new;
            self.needs_redraw = true;
        }
    }

    pub fn draw(&mut self, canvas: &mut Canvas) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        canvas.clear();
        let dw = canvas.display_width();
        let dh = crate::display_height();

        // === Header ===
        let hdr_h = crate::scale_u32(120);
        canvas.fill_rect(
            Point2 { x: Some(0), y: Some(0) },
            Vector2 { x: dw, y: hdr_h },
            color::HEADER_BG,
        );
        canvas.draw_text_colored(
            Point2 {
                x: crate::scale_f32(40.0),
                y: crate::scale_f32(30.0),
            },
            &self.label,
            crate::scale_f32(46.0),
            color::WHITE,
        );

        // === Columns ===
        let cols: u32 = if self.all_day { 3 } else { 6 };
        let col_w = dw / cols;
        let center_y = (dh / 2) as i32;
        let arrow_size = crate::scale_u32(80);
        let arrow_gap = crate::scale_u32(60);
        let value_font = crate::scale_f32(64.0);
        let label_font = crate::scale_f32(28.0);

        // Build per-column data first — labels and current values.
        let month_idx = (self.dt.month() - 1) as usize;
        let month_full = self.strings.months[month_idx];
        let month_short_chars = month_full.chars().take(3).collect::<String>();

        let h24 = self.dt.hour();
        let (h12, is_pm) = if h24 == 0 {
            (12u32, false)
        } else if h24 < 12 {
            (h24, false)
        } else if h24 == 12 {
            (12, true)
        } else {
            (h24 - 12, true)
        };

        let mut columns: Vec<(&str, String)> = vec![
            (self.strings.year, format!("{}", self.dt.year())),
            (self.strings.month, month_short_chars),
            (self.strings.day, format!("{:02}", self.dt.day())),
        ];
        if !self.all_day {
            columns.push((self.strings.hour, format!("{:02}", h12)));
            columns.push((self.strings.minute, format!("{:02}", self.dt.minute())));
            columns.push((
                if is_pm { self.strings.pm } else { self.strings.am },
                String::new(),
            ));
        }

        // Render and capture rects per column.
        let mut up_rects: Vec<mxcfb_rect> = Vec::new();
        let mut dn_rects: Vec<mxcfb_rect> = Vec::new();

        for (idx, (lab, val)) in columns.iter().enumerate() {
            let cx = (idx as u32) * col_w + col_w / 2;
            let arrow_left = cx.saturating_sub(arrow_size / 2);
            let up_rect = mxcfb_rect {
                top: (center_y - arrow_gap as i32 - arrow_size as i32) as u32,
                left: arrow_left,
                width: arrow_size,
                height: arrow_size,
            };
            let dn_rect = mxcfb_rect {
                top: (center_y + arrow_gap as i32) as u32,
                left: arrow_left,
                width: arrow_size,
                height: arrow_size,
            };

            // Column label
            let lr = canvas.measure_text(lab, label_font);
            canvas.draw_text_colored(
                Point2 {
                    x: (cx as f32) - (lr.width as f32) / 2.0,
                    y: (hdr_h + crate::scale_u32(40)) as f32,
                },
                lab,
                label_font,
                color::DARK_GRAY,
            );

            // AM/PM has a single tappable toggle in place of the up/down pair.
            if !self.all_day && idx == 5 {
                let toggle = mxcfb_rect {
                    top: up_rect.top,
                    left: up_rect.left,
                    width: up_rect.width,
                    height: dn_rect.top + dn_rect.height - up_rect.top,
                };
                canvas.draw_rect(
                    Point2 {
                        x: Some(toggle.left as i32),
                        y: Some(toggle.top as i32),
                    },
                    Vector2 { x: toggle.width, y: toggle.height },
                    3,
                );
                let vr = canvas.measure_text(lab, value_font);
                canvas.draw_text_colored(
                    Point2 {
                        x: (cx as f32) - (vr.width as f32) / 2.0,
                        y: (toggle.top + toggle.height / 2 - crate::scale_u32(30)) as f32,
                    },
                    lab,
                    value_font,
                    color::BLACK,
                );
                up_rects.push(toggle);
                dn_rects.push(mxcfb_rect::default());
                continue;
            }

            // Up arrow
            canvas.draw_rect(
                Point2 {
                    x: Some(up_rect.left as i32),
                    y: Some(up_rect.top as i32),
                },
                Vector2 { x: up_rect.width, y: up_rect.height },
                3,
            );
            let up_label = "^";
            let ur = canvas.measure_text(up_label, value_font * 0.8);
            canvas.draw_text_colored(
                Point2 {
                    x: (cx as f32) - (ur.width as f32) / 2.0,
                    y: (up_rect.top + up_rect.height / 2) as f32 - (ur.height as f32) / 2.0,
                },
                up_label,
                value_font * 0.8,
                color::BLACK,
            );

            // Value
            let vr = canvas.measure_text(val, value_font);
            canvas.draw_text_colored(
                Point2 {
                    x: (cx as f32) - (vr.width as f32) / 2.0,
                    y: (center_y as f32) - (vr.height as f32) / 2.0,
                },
                val,
                value_font,
                color::BLACK,
            );

            // Down arrow
            canvas.draw_rect(
                Point2 {
                    x: Some(dn_rect.left as i32),
                    y: Some(dn_rect.top as i32),
                },
                Vector2 { x: dn_rect.width, y: dn_rect.height },
                3,
            );
            let dn_label = "v";
            let dr = canvas.measure_text(dn_label, value_font * 0.8);
            canvas.draw_text_colored(
                Point2 {
                    x: (cx as f32) - (dr.width as f32) / 2.0,
                    y: (dn_rect.top + dn_rect.height / 2) as f32 - (dr.height as f32) / 2.0,
                },
                dn_label,
                value_font * 0.8,
                color::BLACK,
            );

            up_rects.push(up_rect);
            dn_rects.push(dn_rect);
        }

        // Persist hitboxes from the locally-captured rects.
        if up_rects.len() >= 1 {
            self.year_up = up_rects[0];
            self.year_dn = dn_rects[0];
        }
        if up_rects.len() >= 2 {
            self.month_up = up_rects[1];
            self.month_dn = dn_rects[1];
        }
        if up_rects.len() >= 3 {
            self.day_up = up_rects[2];
            self.day_dn = dn_rects[2];
        }
        if up_rects.len() >= 5 {
            self.hour_up = up_rects[3];
            self.hour_dn = dn_rects[3];
            self.min_up = up_rects[4];
            self.min_dn = dn_rects[4];
        }
        if up_rects.len() >= 6 {
            self.ampm_toggle = up_rects[5];
        }

        // === Done / Cancel ===
        let btn_y = dh as i32 - crate::scale_u32(160) as i32;
        let btn_font = crate::scale_f32(46.0);

        let cancel_text = self.strings.cancel;
        let cx = crate::scale_u32(120);
        let mut cancel_rect = canvas.draw_text_colored(
            Point2 {
                x: cx as f32,
                y: btn_y as f32,
            },
            cancel_text,
            btn_font,
            color::BLACK,
        );
        cancel_rect.left = cancel_rect.left.saturating_sub(crate::scale_u32(20));
        cancel_rect.top = cancel_rect.top.saturating_sub(crate::scale_u32(20));
        cancel_rect.width += crate::scale_u32(40);
        cancel_rect.height += crate::scale_u32(40);
        canvas.draw_rect(
            Point2 {
                x: Some(cancel_rect.left as i32),
                y: Some(cancel_rect.top as i32),
            },
            Vector2 {
                x: cancel_rect.width,
                y: cancel_rect.height,
            },
            3,
        );
        self.cancel_hitbox = cancel_rect;

        let done_text = self.strings.done;
        let dr = canvas.measure_text(done_text, btn_font);
        let dx = (dw as i32) - crate::scale_u32(120) as i32 - dr.width as i32;
        let mut done_rect = canvas.draw_text_colored(
            Point2 {
                x: dx as f32,
                y: btn_y as f32,
            },
            done_text,
            btn_font,
            color::BLACK,
        );
        done_rect.left = done_rect.left.saturating_sub(crate::scale_u32(20));
        done_rect.top = done_rect.top.saturating_sub(crate::scale_u32(20));
        done_rect.width += crate::scale_u32(40);
        done_rect.height += crate::scale_u32(40);
        canvas.draw_rect(
            Point2 {
                x: Some(done_rect.left as i32),
                y: Some(done_rect.top as i32),
            },
            Vector2 {
                x: done_rect.width,
                y: done_rect.height,
            },
            3,
        );
        self.done_hitbox = done_rect;

        canvas.update_full();
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    let this = NaiveDate::from_ymd_opt(year, month, 1);
    match (this, next) {
        (Some(t), Some(n)) => (n.signed_duration_since(t).num_days()) as u32,
        _ => 30,
    }
}
