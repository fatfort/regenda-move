use super::Scene;
use crate::caldav::{CalendarInfo, Event};
use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};
use chrono::{DateTime, Datelike, NaiveDate, Utc};

// All sizing is scaled at call time so the layout reflows for both Ferrari
// (1.0) and Move (~0.619). UNIFY's invariant: no raw u32 dimension consts.
fn header_height() -> u32 {
    crate::scale_u32(120)
}
fn stale_banner_height() -> u32 {
    crate::scale_u32(50)
}
fn nav_height() -> u32 {
    crate::scale_u32(100)
}
fn bottom_height() -> u32 {
    crate::scale_u32(100)
}
fn margin() -> u32 {
    crate::scale_u32(40)
}
fn gutter_width() -> u32 {
    crate::scale_u32(180)
}
fn event_row_height() -> u32 {
    crate::scale_u32(56)
}
fn row_pad() -> u32 {
    crate::scale_u32(8)
}

pub struct WeeklyScene {
    pub current_week_start: NaiveDate,
    /// Events that fall within the displayed week, filtered by visible
    /// calendars and sorted by (date, start). Public so `main.rs` can index
    /// into it when handling `go_to_event`.
    pub events: Vec<Event>,
    all_events: Vec<Event>,
    pub calendars: Vec<CalendarInfo>,
    pub go_to_day: Option<NaiveDate>,
    pub go_to_event: Option<usize>,
    pub go_to_settings: bool,
    pub go_to_month: bool,
    pub go_to_create: bool,
    pub refresh_pressed: bool,
    pub exit_pressed: bool,
    pub stale_since: Option<DateTime<Utc>>,
    strings: &'static Strings,
    tz: chrono_tz::Tz,
    // Hitboxes
    prev_hitbox: mxcfb_rect,
    next_hitbox: mxcfb_rect,
    this_hitbox: mxcfb_rect,
    day_view_hitbox: mxcfb_rect,
    month_hitbox: mxcfb_rect,
    settings_hitbox: mxcfb_rect,
    refresh_hitbox: mxcfb_rect,
    plus_hitbox: mxcfb_rect,
    day_header_hitboxes: Vec<(NaiveDate, mxcfb_rect)>,
    event_hitboxes: Vec<(usize, mxcfb_rect)>,
    needs_redraw: bool,
}

impl WeeklyScene {
    pub fn new(
        date: NaiveDate,
        all_events: &[Event],
        calendars: Vec<CalendarInfo>,
        strings: &'static Strings,
        tz: chrono_tz::Tz,
        stale_since: Option<DateTime<Utc>>,
    ) -> Self {
        let week_start = monday_of(date);
        let events = filter_week_events(all_events, week_start, &calendars, &tz);

        WeeklyScene {
            current_week_start: week_start,
            events,
            all_events: all_events.to_vec(),
            calendars,
            go_to_day: None,
            go_to_event: None,
            go_to_settings: false,
            go_to_month: false,
            go_to_create: false,
            refresh_pressed: false,
            exit_pressed: false,
            stale_since,
            strings,
            tz,
            prev_hitbox: mxcfb_rect::default(),
            next_hitbox: mxcfb_rect::default(),
            this_hitbox: mxcfb_rect::default(),
            day_view_hitbox: mxcfb_rect::default(),
            month_hitbox: mxcfb_rect::default(),
            settings_hitbox: mxcfb_rect::default(),
            refresh_hitbox: mxcfb_rect::default(),
            plus_hitbox: mxcfb_rect::default(),
            day_header_hitboxes: Vec::new(),
            event_hitboxes: Vec::new(),
            needs_redraw: true,
        }
    }

    pub fn apply_refresh(
        &mut self,
        all_events: Vec<Event>,
        calendars: Vec<CalendarInfo>,
        stale_since: Option<DateTime<Utc>>,
    ) {
        self.all_events = all_events;
        self.calendars = calendars;
        self.stale_since = stale_since;
        self.events =
            filter_week_events(&self.all_events, self.current_week_start, &self.calendars, &self.tz);
        self.needs_redraw = true;
    }

    pub fn events_total(&self) -> usize {
        self.all_events.len()
    }

    fn week_end(&self) -> NaiveDate {
        self.current_week_start + chrono::Duration::days(6)
    }
}

impl Scene for WeeklyScene {
    fn on_input(&mut self, event: InputEvent) {
        if let InputEvent::MultitouchEvent {
            event: MultitouchEvent::Release { finger },
        } = event
        {
            let pos = finger.pos;

            if Canvas::is_hitting(pos, self.prev_hitbox) {
                self.current_week_start -= chrono::Duration::days(7);
                self.events = filter_week_events(
                    &self.all_events,
                    self.current_week_start,
                    &self.calendars,
                    &self.tz,
                );
                self.needs_redraw = true;
            } else if Canvas::is_hitting(pos, self.next_hitbox) {
                self.current_week_start += chrono::Duration::days(7);
                self.events = filter_week_events(
                    &self.all_events,
                    self.current_week_start,
                    &self.calendars,
                    &self.tz,
                );
                self.needs_redraw = true;
            } else if Canvas::is_hitting(pos, self.this_hitbox) {
                self.current_week_start = monday_of(chrono::Local::now().date_naive());
                self.events = filter_week_events(
                    &self.all_events,
                    self.current_week_start,
                    &self.calendars,
                    &self.tz,
                );
                self.needs_redraw = true;
            } else if Canvas::is_hitting(pos, self.day_view_hitbox) {
                self.go_to_day = Some(chrono::Local::now().date_naive());
            } else if Canvas::is_hitting(pos, self.month_hitbox) {
                self.go_to_month = true;
            } else if Canvas::is_hitting(pos, self.settings_hitbox) {
                self.go_to_settings = true;
            } else if Canvas::is_hitting(pos, self.refresh_hitbox) {
                self.refresh_pressed = true;
            } else if Canvas::is_hitting(pos, self.plus_hitbox) {
                self.go_to_create = true;
            } else {
                for (idx, hitbox) in &self.event_hitboxes {
                    if Canvas::is_hitting(pos, *hitbox) {
                        if *idx < self.events.len() {
                            self.go_to_event = Some(*idx);
                        }
                        return;
                    }
                }
                for (date, hitbox) in &self.day_header_hitboxes {
                    if Canvas::is_hitting(pos, *hitbox) {
                        self.go_to_day = Some(*date);
                        return;
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
        let dh = crate::display_height();
        let today = chrono::Local::now().date_naive();

        // === Header ===
        let hdr = header_height();
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(0),
            },
            Vector2 { x: dw, y: hdr },
            color::HEADER_BG,
        );

        let week_end = self.week_end();
        let start_month_idx = self.current_week_start.month0() as usize;
        let end_month_idx = week_end.month0() as usize;
        let start_label = format!(
            "{} {}",
            self.strings.months[start_month_idx],
            self.current_week_start.day()
        );
        let end_label = if start_month_idx == end_month_idx {
            format!("{}", week_end.day())
        } else {
            format!("{} {}", self.strings.months[end_month_idx], week_end.day())
        };
        let title = format!("{} — {}", start_label, end_label);
        canvas.draw_text_colored(
            Point2 {
                x: margin() as f32,
                y: crate::scale_f32(25.0),
            },
            &title,
            crate::scale_f32(52.0),
            color::WHITE,
        );

        // Settings + Refresh buttons (top right)
        let btn_font = crate::scale_f32(36.0);
        let btn_y = crate::scale_f32(35.0);
        let btn_pad = crate::scale_u32(20);
        let settings_text = self.strings.settings;
        let settings_rect = canvas.measure_text(settings_text, btn_font);
        let sx = dw as f32 - settings_rect.width as f32 - margin() as f32;
        self.settings_hitbox = canvas.draw_text_colored(
            Point2 { x: sx, y: btn_y },
            settings_text,
            btn_font,
            color::WHITE,
        );
        self.settings_hitbox.width += btn_pad;
        self.settings_hitbox.height += btn_pad;

        let refresh_text = self.strings.refresh;
        let refresh_rect = canvas.measure_text(refresh_text, btn_font);
        let rx = sx - refresh_rect.width as f32 - crate::scale_f32(40.0);
        self.refresh_hitbox = canvas.draw_text_colored(
            Point2 { x: rx, y: btn_y },
            refresh_text,
            btn_font,
            color::WHITE,
        );
        self.refresh_hitbox.width += btn_pad;
        self.refresh_hitbox.height += btn_pad;

        // + (new event) button
        let plus_text = self.strings.plus;
        let plus_font = crate::scale_f32(46.0);
        let plus_rect = canvas.measure_text(plus_text, plus_font);
        let px = rx - plus_rect.width as f32 - crate::scale_f32(40.0);
        self.plus_hitbox = canvas.draw_text_colored(
            Point2 { x: px, y: crate::scale_f32(28.0) },
            plus_text,
            plus_font,
            color::WHITE,
        );
        self.plus_hitbox.width += btn_pad;
        self.plus_hitbox.height += btn_pad;

        // === Stale banner ===
        let stale_h = if self.stale_since.is_some() {
            stale_banner_height()
        } else {
            0
        };
        if let Some(ts) = self.stale_since {
            use chrono::TimeZone;
            let local = self.tz.from_utc_datetime(&ts.naive_utc());
            let banner = format!(
                "{} ({})",
                self.strings.offline,
                local.format("%Y-%m-%d %H:%M")
            );
            canvas.fill_rect(
                Point2 {
                    x: Some(0),
                    y: Some(hdr as i32),
                },
                Vector2 {
                    x: dw,
                    y: stale_banner_height(),
                },
                color::LIGHT_GRAY,
            );
            canvas.draw_text_colored(
                Point2 {
                    x: margin() as f32,
                    y: (hdr + crate::scale_u32(8)) as f32,
                },
                &banner,
                crate::scale_f32(30.0),
                color::DARK_GRAY,
            );
        }

        // === 7 day rows ===
        let nav = nav_height();
        let bottom = bottom_height();
        let list_top = hdr + stale_h;
        let list_bottom = dh - nav - bottom;
        let total_list_h = list_bottom.saturating_sub(list_top);
        let row_h = total_list_h / 7;
        let gutter = gutter_width();
        let pad = row_pad();

        self.day_header_hitboxes.clear();
        self.event_hitboxes.clear();

        // Group events by date — multi-day events appear under every day they span.
        let week_end_inclusive = self.current_week_start + chrono::Duration::days(6);
        let mut events_by_date: std::collections::HashMap<NaiveDate, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, ev) in self.events.iter().enumerate() {
            let start = ev.date_in_tz(&self.tz).max(self.current_week_start);
            let end = ev.end_date_in_tz(&self.tz).min(week_end_inclusive);
            let mut d = start;
            while d <= end {
                events_by_date.entry(d).or_insert_with(Vec::new).push(i);
                d += chrono::Duration::days(1);
            }
        }

        for day_idx in 0..7u32 {
            let date = self.current_week_start + chrono::Duration::days(day_idx as i64);
            let row_y = list_top + day_idx * row_h;
            let is_today = date == today;

            // Today highlight: subtle background tint across whole row
            if is_today {
                canvas.fill_rect(
                    Point2 {
                        x: Some(0),
                        y: Some(row_y as i32),
                    },
                    Vector2 {
                        x: dw,
                        y: row_h,
                    },
                    color::TODAY_BG,
                );
            }

            // Hairline above each row except the first
            if day_idx > 0 {
                canvas.fill_rect(
                    Point2 {
                        x: Some(margin() as i32),
                        y: Some(row_y as i32),
                    },
                    Vector2 {
                        x: dw - 2 * margin(),
                        y: 1,
                    },
                    color::LIGHT_GRAY,
                );
            }

            // === Left gutter: day-of-week label + day number ===
            let dow_idx = date.weekday().num_days_from_monday() as usize;
            let dow_short = self.strings.days_of_week_short[dow_idx];
            canvas.draw_text_colored(
                Point2 {
                    x: (margin() + pad) as f32,
                    y: (row_y + pad) as f32,
                },
                dow_short,
                crate::scale_f32(32.0),
                if is_today {
                    color::ACCENT
                } else {
                    color::DARK_GRAY
                },
            );

            let day_num = date.day().to_string();
            canvas.draw_text_colored(
                Point2 {
                    x: (margin() + pad) as f32,
                    y: (row_y + pad + crate::scale_u32(40)) as f32,
                },
                &day_num,
                crate::scale_f32(56.0),
                if is_today { color::ACCENT } else { color::BLACK },
            );

            // Gutter is the tap target for jumping to DayScene
            self.day_header_hitboxes.push((
                date,
                mxcfb_rect {
                    top: row_y,
                    left: 0,
                    width: margin() + gutter,
                    height: row_h,
                },
            ));

            // === Right region: agenda list for this day ===
            let agenda_left = margin() + gutter;
            let agenda_right = dw - margin();
            let agenda_w = agenda_right.saturating_sub(agenda_left);

            let day_event_indices = events_by_date.get(&date).cloned().unwrap_or_default();
            let item_h = event_row_height();
            let max_items = ((row_h.saturating_sub(2 * pad)) / item_h).max(1) as usize;

            if day_event_indices.is_empty() {
                canvas.draw_text_colored(
                    Point2 {
                        x: agenda_left as f32,
                        y: (row_y + pad + crate::scale_u32(12)) as f32,
                    },
                    "—",
                    crate::scale_f32(32.0),
                    color::MEDIUM_GRAY,
                );
            } else {
                let visible = day_event_indices.len().min(max_items);
                let overflow = day_event_indices.len().saturating_sub(visible);
                let visible_count = if overflow > 0 && visible > 0 {
                    // Reserve last slot for "+N more"
                    visible - 1
                } else {
                    visible
                };
                let actual_overflow =
                    day_event_indices.len().saturating_sub(visible_count);

                for (slot, ev_idx) in day_event_indices
                    .iter()
                    .take(visible_count)
                    .enumerate()
                {
                    let item_y = row_y + pad + (slot as u32) * item_h;
                    let event = &self.events[*ev_idx];

                    // Color stripe at the left of the agenda region
                    let stripe_color = event.calendar_color.unwrap_or(color::DARK_GRAY);
                    canvas.fill_rect(
                        Point2 {
                            x: Some(agenda_left as i32),
                            y: Some((item_y + crate::scale_u32(6)) as i32),
                        },
                        Vector2 {
                            x: crate::scale_u32(6),
                            y: item_h.saturating_sub(crate::scale_u32(12)),
                        },
                        stripe_color,
                    );

                    // Time
                    let time_str = if event.all_day {
                        self.strings.allday.to_string()
                    } else {
                        event.start_time_str(&self.tz)
                    };
                    let time_x = agenda_left + crate::scale_u32(20);
                    canvas.draw_text_colored(
                        Point2 {
                            x: time_x as f32,
                            y: (item_y + crate::scale_u32(8)) as f32,
                        },
                        &time_str,
                        crate::scale_f32(28.0),
                        color::DARK_GRAY,
                    );

                    // Title (truncated to fit)
                    let title_x = time_x + crate::scale_u32(140);
                    let title_max_w = agenda_right.saturating_sub(title_x);
                    let title_text = truncate_to_width(canvas, &event.summary, crate::scale_f32(30.0), title_max_w);
                    canvas.draw_text_colored(
                        Point2 {
                            x: title_x as f32,
                            y: (item_y + crate::scale_u32(6)) as f32,
                        },
                        &title_text,
                        crate::scale_f32(30.0),
                        color::BLACK,
                    );

                    self.event_hitboxes.push((
                        *ev_idx,
                        mxcfb_rect {
                            top: item_y,
                            left: agenda_left,
                            width: agenda_w,
                            height: item_h,
                        },
                    ));
                }

                if actual_overflow > 0 {
                    let item_y = row_y + pad + (visible_count as u32) * item_h;
                    let more_text =
                        format!("+{} {}", actual_overflow, self.strings.more_suffix);
                    canvas.draw_text_colored(
                        Point2 {
                            x: (agenda_left + crate::scale_u32(20)) as f32,
                            y: (item_y + crate::scale_u32(8)) as f32,
                        },
                        &more_text,
                        crate::scale_f32(26.0),
                        color::MEDIUM_GRAY,
                    );
                }
            }
        }

        // === Navigation bar (Prev / This / Next week) ===
        let nav_y = (dh - nav - bottom) as i32;
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(nav_y),
            },
            Vector2 { x: dw, y: 2 },
            color::LIGHT_GRAY,
        );

        let third = dw / 3;
        let nav_font = crate::scale_f32(40.0);
        let nav_text_y = nav_y + crate::scale_u32(25) as i32;

        let prev_text = self.strings.prev_week;
        let prev_r = canvas.measure_text(prev_text, nav_font);
        canvas.draw_text_colored(
            Point2 {
                x: (third as f32 - prev_r.width as f32) / 2.0,
                y: nav_text_y as f32,
            },
            prev_text,
            nav_font,
            color::BLACK,
        );
        self.prev_hitbox = mxcfb_rect {
            top: nav_y as u32,
            left: 0,
            width: third,
            height: nav,
        };

        let this_text = self.strings.this_week;
        let this_r = canvas.measure_text(this_text, nav_font);
        let viewing_current = self.current_week_start
            == monday_of(chrono::Local::now().date_naive());
        canvas.draw_text_colored(
            Point2 {
                x: third as f32 + (third as f32 - this_r.width as f32) / 2.0,
                y: nav_text_y as f32,
            },
            this_text,
            nav_font,
            if viewing_current {
                color::ACCENT
            } else {
                color::BLACK
            },
        );
        self.this_hitbox = mxcfb_rect {
            top: nav_y as u32,
            left: third,
            width: third,
            height: nav,
        };

        let next_text = self.strings.next_week;
        let next_r = canvas.measure_text(next_text, nav_font);
        canvas.draw_text_colored(
            Point2 {
                x: 2.0 * third as f32 + (third as f32 - next_r.width as f32) / 2.0,
                y: nav_text_y as f32,
            },
            next_text,
            nav_font,
            color::BLACK,
        );
        self.next_hitbox = mxcfb_rect {
            top: nav_y as u32,
            left: 2 * third,
            width: third,
            height: nav,
        };

        // === Bottom bar: Day View | Month View ===
        let bottom_y = (dh - bottom) as i32;
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(bottom_y),
            },
            Vector2 { x: dw, y: 2 },
            color::LIGHT_GRAY,
        );
        let bottom_font = crate::scale_f32(36.0);
        let bottom_text_y = bottom_y + crate::scale_u32(25) as i32;
        let half = dw / 2;

        let day_text = self.strings.day_view;
        let dvr = canvas.measure_text(day_text, bottom_font);
        canvas.draw_text_colored(
            Point2 {
                x: (half as f32 - dvr.width as f32) / 2.0,
                y: bottom_text_y as f32,
            },
            day_text,
            bottom_font,
            color::BLACK,
        );
        self.day_view_hitbox = mxcfb_rect {
            top: bottom_y as u32,
            left: 0,
            width: half,
            height: bottom,
        };

        let month_text = self.strings.month_view;
        let mvr = canvas.measure_text(month_text, bottom_font);
        canvas.draw_text_colored(
            Point2 {
                x: half as f32 + (half as f32 - mvr.width as f32) / 2.0,
                y: bottom_text_y as f32,
            },
            month_text,
            bottom_font,
            color::BLACK,
        );
        self.month_hitbox = mxcfb_rect {
            top: bottom_y as u32,
            left: half,
            width: half,
            height: bottom,
        };

        // Vertical divider between the two bottom buttons
        canvas.fill_rect(
            Point2 {
                x: Some(half as i32),
                y: Some(bottom_y),
            },
            Vector2 {
                x: 1,
                y: bottom,
            },
            color::LIGHT_GRAY,
        );

        canvas.update_full();
    }
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn filter_week_events(
    all_events: &[Event],
    week_start: NaiveDate,
    calendars: &[CalendarInfo],
    tz: &chrono_tz::Tz,
) -> Vec<Event> {
    let week_end_inclusive = week_start + chrono::Duration::days(6);
    let visible_cals: Vec<&str> = calendars
        .iter()
        .filter(|c| c.visible)
        .map(|c| c.name.as_str())
        .collect();

    // Multi-day events count if they overlap any day in the week.
    let mut filtered: Vec<Event> = all_events
        .iter()
        .filter(|e| {
            let start = e.date_in_tz(tz);
            let end = e.end_date_in_tz(tz);
            let overlaps = start <= week_end_inclusive && end >= week_start;
            overlaps
                && (visible_cals.is_empty() || visible_cals.contains(&e.calendar_name.as_str()))
        })
        .cloned()
        .collect();

    filtered.sort();
    filtered
}

fn truncate_to_width(canvas: &mut Canvas, text: &str, font: f32, max_w: u32) -> String {
    let full = canvas.measure_text(text, font);
    if full.width <= max_w {
        return text.to_string();
    }
    // Binary-search-ish: progressively trim characters until it fits.
    let chars: Vec<char> = text.chars().collect();
    let mut lo = 0usize;
    let mut hi = chars.len();
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let mut candidate: String = chars[..mid].iter().collect();
        candidate.push('…');
        let m = canvas.measure_text(&candidate, font);
        if m.width <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        return "…".to_string();
    }
    let mut result: String = chars[..lo].iter().collect();
    result.push('…');
    result
}
