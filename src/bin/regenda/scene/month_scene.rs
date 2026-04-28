use super::Scene;
use crate::caldav::Event;
use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};
use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;

fn header_height() -> u32 {
    crate::scale_u32(120)
}
const DOW_HEIGHT: u32 = 60;
const MARGIN: u32 = 40;
fn grid_top() -> u32 {
    header_height() + DOW_HEIGHT
}

pub struct MonthScene {
    pub selected_date: Option<NaiveDate>,
    pub back_pressed: bool,
    current_month: u32,
    current_year: i32,
    initial_date: NaiveDate,
    strings: &'static Strings,
    tz: chrono_tz::Tz,
    events: Vec<Event>,
    // Hitboxes
    prev_month_hitbox: mxcfb_rect,
    next_month_hitbox: mxcfb_rect,
    back_hitbox: mxcfb_rect,
    day_hitboxes: Vec<(NaiveDate, mxcfb_rect)>,
    needs_redraw: bool,
}

impl MonthScene {
    pub fn new(
        date: NaiveDate,
        events: Vec<Event>,
        strings: &'static Strings,
        tz: chrono_tz::Tz,
    ) -> Self {
        MonthScene {
            selected_date: None,
            back_pressed: false,
            current_month: date.month(),
            current_year: date.year(),
            initial_date: date,
            strings,
            tz,
            events,
            prev_month_hitbox: mxcfb_rect::default(),
            next_month_hitbox: mxcfb_rect::default(),
            back_hitbox: mxcfb_rect::default(),
            day_hitboxes: Vec::new(),
            needs_redraw: true,
        }
    }
}

impl Scene for MonthScene {
    fn on_input(&mut self, event: InputEvent) {
        if let InputEvent::MultitouchEvent {
            event: MultitouchEvent::Release { finger },
        } = event
        {
            let pos = finger.pos;

            if Canvas::is_hitting(pos, self.back_hitbox) {
                self.back_pressed = true;
            } else if Canvas::is_hitting(pos, self.prev_month_hitbox) {
                if self.current_month == 1 {
                    self.current_month = 12;
                    self.current_year -= 1;
                } else {
                    self.current_month -= 1;
                }
                self.needs_redraw = true;
            } else if Canvas::is_hitting(pos, self.next_month_hitbox) {
                if self.current_month == 12 {
                    self.current_month = 1;
                    self.current_year += 1;
                } else {
                    self.current_month += 1;
                }
                self.needs_redraw = true;
            } else {
                for (date, hitbox) in &self.day_hitboxes {
                    if Canvas::is_hitting(pos, *hitbox) {
                        self.selected_date = Some(*date);
                        break;
                    }
                }
            }
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        if !self.needs_redraw {
            return;
        }
        self.needs_redraw = false;

        canvas.clear();
        let dw = canvas.display_width();
        let today = chrono::Local::now().date_naive();

        // Collect distinct calendar colors per date (for per-event dots)
        let mut date_colors: HashMap<NaiveDate, Vec<color>> = HashMap::new();
        for event in &self.events {
            let d = event.date_in_tz(&self.tz);
            let c = event.calendar_color.unwrap_or(color::DARK_GRAY);
            let entry = date_colors.entry(d).or_insert_with(Vec::new);
            if !entry.contains(&c) {
                entry.push(c);
            }
        }

        // === Header ===
        let hdr = header_height();
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(0),
            },
            Vector2 {
                x: dw,
                y: hdr,
            },
            color::HEADER_BG,
        );

        // Back button
        let back_pad = crate::scale_u32(20);
        self.back_hitbox = canvas.draw_text_colored(
            Point2 {
                x: MARGIN as f32,
                y: crate::scale_f32(30.0),
            },
            self.strings.back,
            crate::scale_f32(42.0),
            color::WHITE,
        );
        self.back_hitbox.width += back_pad;
        self.back_hitbox.height += back_pad;

        // Month/Year title
        let month_name = self.strings.months[(self.current_month - 1) as usize];
        let title = format!("< {} {} >", month_name, self.current_year);
        let title_font = crate::scale_f32(52.0);
        let tr = canvas.measure_text(&title, title_font);
        let tx = (dw as f32 - tr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: tx, y: crate::scale_f32(25.0) },
            &title,
            title_font,
            color::WHITE,
        );

        // Month navigation hitboxes (sized so prev/next chevrons line up
        // with the "<" / ">" glyphs of the title at the current scale).
        let center = dw / 2;
        let nav_offset = crate::scale_u32(250);
        let nav_offset2 = crate::scale_u32(150);
        let nav_w = crate::scale_u32(100);
        self.prev_month_hitbox = mxcfb_rect {
            top: 0,
            left: center - nav_offset,
            width: nav_w,
            height: hdr,
        };
        self.next_month_hitbox = mxcfb_rect {
            top: 0,
            left: center + nav_offset2,
            width: nav_w,
            height: hdr,
        };

        // === Day of week headers ===
        let cell_w = (dw - 2 * MARGIN) / 7;
        for (i, dow) in self.strings.days_of_week_short.iter().enumerate() {
            let x = MARGIN + i as u32 * cell_w + cell_w / 2;
            let dr = canvas.measure_text(dow, 32.0);
            canvas.draw_text_colored(
                Point2 {
                    x: (x as f32 - dr.width as f32 / 2.0),
                    y: (hdr + 15) as f32,
                },
                dow,
                32.0,
                color::DARK_GRAY,
            );
        }

        // === Calendar grid ===
        self.day_hitboxes.clear();

        let first_of_month =
            NaiveDate::from_ymd_opt(self.current_year, self.current_month, 1).unwrap();
        let days_in_month = days_in_month(self.current_year, self.current_month);
        let start_dow = first_of_month.weekday().num_days_from_monday() as usize;

        let cell_h = 120u32;

        for day in 1..=days_in_month {
            let date = NaiveDate::from_ymd_opt(self.current_year, self.current_month, day).unwrap();
            let cell_idx = start_dow + (day - 1) as usize;
            let col = cell_idx % 7;
            let row = cell_idx / 7;

            let cx = MARGIN + col as u32 * cell_w + cell_w / 2;
            let cy = grid_top() + row as u32 * cell_h + cell_h / 2;

            let hitbox = mxcfb_rect {
                top: grid_top() + row as u32 * cell_h,
                left: MARGIN + col as u32 * cell_w,
                width: cell_w,
                height: cell_h,
            };
            self.day_hitboxes.push((date, hitbox));

            // Highlight today
            if date == today {
                canvas.fill_circle(cx as i32, cy as i32, 35, color::ACCENT);
            } else if date == self.initial_date {
                canvas.fill_circle(cx as i32, cy as i32, 35, color::LIGHT_GRAY);
            }

            // Day number
            let day_str = day.to_string();
            let dr = canvas.measure_text(&day_str, 40.0);
            let text_color = if date == today {
                color::WHITE
            } else {
                color::BLACK
            };
            canvas.draw_text_colored(
                Point2 {
                    x: cx as f32 - dr.width as f32 / 2.0,
                    y: cy as f32 - dr.height as f32 / 2.0,
                },
                &day_str,
                40.0,
                text_color,
            );

            // Per-calendar event dots (one dot per distinct calendar, capped at 3)
            if date != today {
                if let Some(colors) = date_colors.get(&date) {
                    let n = colors.len().min(3) as i32;
                    let spacing: i32 = 14;
                    let total_w = (n - 1) * spacing;
                    let start_x = cx as i32 - total_w / 2;
                    for (i, c) in colors.iter().take(n as usize).enumerate() {
                        canvas.fill_circle(
                            start_x + i as i32 * spacing,
                            (cy + 30) as i32,
                            5,
                            *c,
                        );
                    }
                }
            }
        }

        canvas.update_full();
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap()
    .signed_duration_since(NaiveDate::from_ymd_opt(year, month, 1).unwrap())
    .num_days() as u32
}
