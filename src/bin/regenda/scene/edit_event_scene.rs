use super::datetime_spin_scene::{DateTimeSpin, SpinOutcome};
use super::keyboard_scene::{Keyboard, KeyboardOutcome};
use super::Scene;
use crate::caldav::{CalendarInfo, Event, EventWrite};
use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};

/// Whether this scene is creating a brand-new event or editing an existing one.
#[derive(Clone, Debug)]
pub enum EditMode {
    Create,
    Edit(Event),
}

/// Which sub-overlay (if any) is currently active. Mutually exclusive with
/// the form view itself.
enum Focus {
    Form,
    Title,
    Location,
    Description,
    Start,
    End,
    CalendarPicker,
}

pub struct EditEventScene {
    pub save_request: Option<SaveRequest>,
    pub delete_request: Option<DeleteRequest>,
    pub cancel_pressed: bool,

    mode: EditMode,
    /// Google-writable calendars only — ICS sources are filtered out before
    /// the scene is constructed.
    writable_calendars: Vec<CalendarInfo>,
    /// Which calendar (path = Google calendar ID) the user picked.
    selected_calendar_idx: usize,

    title: String,
    location: String,
    description: String,
    start_dt: NaiveDateTime,
    end_dt: NaiveDateTime,
    all_day: bool,

    tz: chrono_tz::Tz,
    strings: &'static Strings,
    focus: Focus,

    keyboard: Option<Keyboard>,
    spin: Option<DateTimeSpin>,

    // Form-mode hitboxes
    back_hitbox: mxcfb_rect,
    save_hitbox: mxcfb_rect,
    title_hitbox: mxcfb_rect,
    location_hitbox: mxcfb_rect,
    description_hitbox: mxcfb_rect,
    start_hitbox: mxcfb_rect,
    end_hitbox: mxcfb_rect,
    allday_hitbox: mxcfb_rect,
    calendar_hitbox: mxcfb_rect,

    // Calendar picker hitboxes
    picker_close_hitbox: mxcfb_rect,
    picker_row_hitboxes: Vec<mxcfb_rect>,

    needs_redraw: bool,
}

/// Captured by main.rs after the user taps Save. main.rs does the actual API
/// call on a worker thread + chains a refresh.
#[derive(Clone, Debug)]
pub struct SaveRequest {
    pub mode: SaveMode,
    pub calendar_id: String,
    pub write: EventWrite,
}

#[derive(Clone, Debug)]
pub enum SaveMode {
    Insert,
    Patch { event_id: String },
}

/// User confirmed Delete on an Edit-mode form. main.rs handles the API call.
#[derive(Clone, Debug)]
pub struct DeleteRequest {
    pub calendar_id: String,
    pub event_id: String,
}

impl EditEventScene {
    pub fn new(
        mode: EditMode,
        writable_calendars: Vec<CalendarInfo>,
        initial_date: NaiveDate,
        strings: &'static Strings,
        tz: chrono_tz::Tz,
    ) -> Self {
        // Default starts at the next round half-hour today (or initial_date)
        // for a polished Create flow; for Edit pull from the event itself.
        let (title, location, description, start_dt, end_dt, all_day, selected_calendar_idx) =
            match &mode {
                EditMode::Create => {
                    let now = chrono::Local::now().naive_local();
                    let start_naive = round_to_next_half_hour(now);
                    let start_on_date = NaiveDateTime::new(initial_date, start_naive.time());
                    let end_on_date = start_on_date + chrono::Duration::hours(1);
                    (
                        String::new(),
                        String::new(),
                        String::new(),
                        start_on_date,
                        end_on_date,
                        false,
                        0,
                    )
                }
                EditMode::Edit(ev) => {
                    let start_local =
                        tz.from_utc_datetime(&ev.start.naive_utc()).naive_local();
                    let end_local = ev
                        .end
                        .map(|e| tz.from_utc_datetime(&e.naive_utc()).naive_local())
                        .unwrap_or_else(|| start_local + chrono::Duration::hours(1));
                    let idx = writable_calendars
                        .iter()
                        .position(|c| {
                            ev.source_calendar_id.as_deref() == Some(c.path.as_str())
                        })
                        .unwrap_or(0);
                    (
                        ev.summary.clone(),
                        ev.location.clone().unwrap_or_default(),
                        ev.description.clone().unwrap_or_default(),
                        start_local,
                        end_local,
                        ev.all_day,
                        idx,
                    )
                }
            };

        EditEventScene {
            save_request: None,
            delete_request: None,
            cancel_pressed: false,
            mode,
            writable_calendars,
            selected_calendar_idx,
            title,
            location,
            description,
            start_dt,
            end_dt,
            all_day,
            tz,
            strings,
            focus: Focus::Form,
            keyboard: None,
            spin: None,
            back_hitbox: mxcfb_rect::default(),
            save_hitbox: mxcfb_rect::default(),
            title_hitbox: mxcfb_rect::default(),
            location_hitbox: mxcfb_rect::default(),
            description_hitbox: mxcfb_rect::default(),
            start_hitbox: mxcfb_rect::default(),
            end_hitbox: mxcfb_rect::default(),
            allday_hitbox: mxcfb_rect::default(),
            calendar_hitbox: mxcfb_rect::default(),
            picker_close_hitbox: mxcfb_rect::default(),
            picker_row_hitboxes: Vec::new(),
            needs_redraw: true,
        }
    }

    fn handle_form_tap(&mut self, pos: cgmath::Point2<u16>) {
        if Canvas::is_hitting(pos, self.back_hitbox) {
            self.cancel_pressed = true;
            return;
        }
        if Canvas::is_hitting(pos, self.save_hitbox) {
            self.submit();
            return;
        }
        if Canvas::is_hitting(pos, self.title_hitbox) {
            self.keyboard = Some(Keyboard::new(
                &self.title,
                self.strings.title_label,
                false,
                self.strings,
            ));
            self.focus = Focus::Title;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.location_hitbox) {
            self.keyboard = Some(Keyboard::new(
                &self.location,
                self.strings.location_label,
                false,
                self.strings,
            ));
            self.focus = Focus::Location;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.description_hitbox) {
            self.keyboard = Some(Keyboard::new(
                &self.description,
                self.strings.description_label,
                true,
                self.strings,
            ));
            self.focus = Focus::Description;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.start_hitbox) {
            self.spin = Some(DateTimeSpin::new(
                self.strings.start_label,
                self.start_dt,
                self.all_day,
                self.strings,
            ));
            self.focus = Focus::Start;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.end_hitbox) {
            self.spin = Some(DateTimeSpin::new(
                self.strings.end_label,
                self.end_dt,
                self.all_day,
                self.strings,
            ));
            self.focus = Focus::End;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.allday_hitbox) {
            self.all_day = !self.all_day;
            self.needs_redraw = true;
            return;
        }
        if Canvas::is_hitting(pos, self.calendar_hitbox)
            && matches!(self.mode, EditMode::Create)
            && self.writable_calendars.len() > 1
        {
            self.focus = Focus::CalendarPicker;
            self.needs_redraw = true;
            return;
        }
    }

    fn submit(&mut self) {
        if self.title.trim().is_empty() {
            // Don't allow saving an empty-title event.
            return;
        }
        let calendar = match self.writable_calendars.get(self.selected_calendar_idx) {
            Some(c) => c,
            None => return,
        };
        let calendar_id = calendar.path.clone();
        let tz_name = format!("{}", self.tz);

        // Convert local NaiveDateTime → UTC anchored in the configured tz.
        let start_local = self
            .tz
            .from_local_datetime(&self.start_dt)
            .earliest()
            .unwrap_or_else(|| self.tz.from_utc_datetime(&self.start_dt));
        let end_local = self
            .tz
            .from_local_datetime(&self.end_dt)
            .earliest()
            .unwrap_or_else(|| self.tz.from_utc_datetime(&self.end_dt));

        let write = EventWrite {
            summary: self.title.trim().to_string(),
            location: opt_nonblank(&self.location),
            description: opt_nonblank(&self.description),
            all_day: self.all_day,
            start: start_local.with_timezone(&Utc),
            end: Some(end_local.with_timezone(&Utc)),
            timezone: tz_name,
        };

        let mode = match &self.mode {
            EditMode::Create => SaveMode::Insert,
            EditMode::Edit(ev) => match &ev.source_event_id {
                Some(id) => SaveMode::Patch {
                    event_id: id.clone(),
                },
                None => return, // can't patch without an ID
            },
        };

        self.save_request = Some(SaveRequest {
            mode,
            calendar_id,
            write,
        });
    }

    fn render_form(&mut self, canvas: &mut Canvas) {
        canvas.clear();
        let dw = canvas.display_width();
        let hdr_h = crate::scale_u32(120);

        // Header
        canvas.fill_rect(
            Point2 { x: Some(0), y: Some(0) },
            Vector2 { x: dw, y: hdr_h },
            color::HEADER_BG,
        );
        let back_pad = crate::scale_u32(20);
        self.back_hitbox = canvas.draw_text_colored(
            Point2 {
                x: 40.0,
                y: crate::scale_f32(30.0),
            },
            self.strings.cancel,
            crate::scale_f32(42.0),
            color::WHITE,
        );
        self.back_hitbox.width += back_pad;
        self.back_hitbox.height += back_pad;

        let title_text = match self.mode {
            EditMode::Create => self.strings.create_event,
            EditMode::Edit(_) => self.strings.edit_event,
        };
        let title_font = crate::scale_f32(46.0);
        let tr = canvas.measure_text(title_text, title_font);
        let tx = (dw as f32 - tr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: tx, y: crate::scale_f32(30.0) },
            title_text,
            title_font,
            color::WHITE,
        );

        // Save button (top-right)
        let save_font = crate::scale_f32(38.0);
        let save_text = self.strings.save;
        let sr = canvas.measure_text(save_text, save_font);
        let sx = dw as f32 - sr.width as f32 - crate::scale_f32(40.0);
        let mut save_rect = canvas.draw_text_colored(
            Point2 {
                x: sx,
                y: crate::scale_f32(35.0),
            },
            save_text,
            save_font,
            color::WHITE,
        );
        save_rect.left = save_rect.left.saturating_sub(back_pad / 2);
        save_rect.top = save_rect.top.saturating_sub(back_pad / 2);
        save_rect.width += back_pad;
        save_rect.height += back_pad;
        self.save_hitbox = save_rect;

        // === Form rows ===
        let margin = crate::scale_u32(40);
        let row_h = crate::scale_u32(120);
        let label_font = crate::scale_f32(28.0);
        let value_font = crate::scale_f32(36.0);
        let mut y = hdr_h + crate::scale_u32(30);

        // Title
        self.title_hitbox = self.render_field_row(
            canvas,
            margin,
            y,
            dw,
            row_h,
            label_font,
            value_font,
            self.strings.title_label,
            placeholder_or(&self.title, "(empty)"),
        );
        y += row_h + crate::scale_u32(10);

        // All-day toggle row (compact)
        let allday_row_h = crate::scale_u32(80);
        canvas.draw_rect(
            Point2 {
                x: Some(margin as i32),
                y: Some(y as i32),
            },
            Vector2 {
                x: dw - 2 * margin,
                y: allday_row_h,
            },
            2,
        );
        canvas.draw_text_colored(
            Point2 {
                x: (margin + crate::scale_u32(20)) as f32,
                y: (y + crate::scale_u32(20)) as f32,
            },
            self.strings.all_day_label,
            value_font,
            color::BLACK,
        );
        let toggle_text = if self.all_day {
            self.strings.yes
        } else {
            self.strings.no
        };
        let tr2 = canvas.measure_text(toggle_text, value_font);
        canvas.draw_text_colored(
            Point2 {
                x: (dw - margin - crate::scale_u32(20) - tr2.width) as f32,
                y: (y + crate::scale_u32(20)) as f32,
            },
            toggle_text,
            value_font,
            if self.all_day { color::ACCENT } else { color::DARK_GRAY },
        );
        self.allday_hitbox = mxcfb_rect {
            top: y,
            left: margin,
            width: dw - 2 * margin,
            height: allday_row_h,
        };
        y += allday_row_h + crate::scale_u32(10);

        // Start
        self.start_hitbox = self.render_field_row(
            canvas,
            margin,
            y,
            dw,
            row_h,
            label_font,
            value_font,
            self.strings.start_label,
            self.format_datetime(self.start_dt),
        );
        y += row_h + crate::scale_u32(10);

        // End
        self.end_hitbox = self.render_field_row(
            canvas,
            margin,
            y,
            dw,
            row_h,
            label_font,
            value_font,
            self.strings.end_label,
            self.format_datetime(self.end_dt),
        );
        y += row_h + crate::scale_u32(10);

        // Location
        self.location_hitbox = self.render_field_row(
            canvas,
            margin,
            y,
            dw,
            row_h,
            label_font,
            value_font,
            self.strings.location_label,
            placeholder_or(&self.location, "—"),
        );
        y += row_h + crate::scale_u32(10);

        // Description
        let desc_h = row_h + crate::scale_u32(60);
        self.description_hitbox = self.render_field_row(
            canvas,
            margin,
            y,
            dw,
            desc_h,
            label_font,
            value_font,
            self.strings.description_label,
            placeholder_or(&self.description, "—"),
        );
        y += desc_h + crate::scale_u32(10);

        // Calendar (Create only — Edit can't change source)
        if matches!(self.mode, EditMode::Create) && !self.writable_calendars.is_empty() {
            let cal_name = self
                .writable_calendars
                .get(self.selected_calendar_idx)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            self.calendar_hitbox = self.render_field_row(
                canvas,
                margin,
                y,
                dw,
                row_h,
                label_font,
                value_font,
                self.strings.calendar_label,
                cal_name.to_string(),
            );
        }
    }

    fn render_field_row(
        &self,
        canvas: &mut Canvas,
        margin: u32,
        y: u32,
        dw: u32,
        row_h: u32,
        label_font: f32,
        value_font: f32,
        label: &str,
        value: String,
    ) -> mxcfb_rect {
        canvas.draw_rect(
            Point2 {
                x: Some(margin as i32),
                y: Some(y as i32),
            },
            Vector2 {
                x: dw - 2 * margin,
                y: row_h,
            },
            2,
        );
        canvas.draw_text_colored(
            Point2 {
                x: (margin + crate::scale_u32(20)) as f32,
                y: (y + crate::scale_u32(12)) as f32,
            },
            label,
            label_font,
            color::DARK_GRAY,
        );
        canvas.draw_text_colored(
            Point2 {
                x: (margin + crate::scale_u32(20)) as f32,
                y: (y + crate::scale_u32(50)) as f32,
            },
            &value,
            value_font,
            color::BLACK,
        );
        mxcfb_rect {
            top: y,
            left: margin,
            width: dw - 2 * margin,
            height: row_h,
        }
    }

    fn format_datetime(&self, dt: NaiveDateTime) -> String {
        if self.all_day {
            dt.format("%Y-%m-%d").to_string()
        } else {
            dt.format("%Y-%m-%d %H:%M").to_string()
        }
    }

    fn render_calendar_picker(&mut self, canvas: &mut Canvas) {
        canvas.clear();
        let dw = canvas.display_width();
        let hdr_h = crate::scale_u32(120);

        canvas.fill_rect(
            Point2 { x: Some(0), y: Some(0) },
            Vector2 { x: dw, y: hdr_h },
            color::HEADER_BG,
        );
        let close_pad = crate::scale_u32(20);
        self.picker_close_hitbox = canvas.draw_text_colored(
            Point2 {
                x: 40.0,
                y: crate::scale_f32(30.0),
            },
            self.strings.back,
            crate::scale_f32(42.0),
            color::WHITE,
        );
        self.picker_close_hitbox.width += close_pad;
        self.picker_close_hitbox.height += close_pad;
        let title = self.strings.calendar_label;
        let title_font = crate::scale_f32(46.0);
        let tr = canvas.measure_text(title, title_font);
        let tx = (dw as f32 - tr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: tx, y: crate::scale_f32(30.0) },
            title,
            title_font,
            color::WHITE,
        );

        let row_h = crate::scale_u32(100);
        let margin = crate::scale_u32(40);
        let mut y = hdr_h + crate::scale_u32(30);
        self.picker_row_hitboxes.clear();

        for (i, cal) in self.writable_calendars.iter().enumerate() {
            let stripe_color = cal
                .color
                .as_deref()
                .and_then(crate::caldav::types::parse_hex_color)
                .unwrap_or(color::DARK_GRAY);
            canvas.fill_rect(
                Point2 {
                    x: Some(margin as i32),
                    y: Some((y + crate::scale_u32(10)) as i32),
                },
                Vector2 {
                    x: crate::scale_u32(10),
                    y: row_h - crate::scale_u32(20),
                },
                stripe_color,
            );
            canvas.draw_text_colored(
                Point2 {
                    x: (margin + crate::scale_u32(40)) as f32,
                    y: (y + crate::scale_u32(28)) as f32,
                },
                &cal.name,
                crate::scale_f32(40.0),
                color::BLACK,
            );
            if i == self.selected_calendar_idx {
                let check_text = "✓";
                let cr = canvas.measure_text(check_text, crate::scale_f32(40.0));
                canvas.draw_text_colored(
                    Point2 {
                        x: (dw - margin - crate::scale_u32(20) - cr.width) as f32,
                        y: (y + crate::scale_u32(28)) as f32,
                    },
                    check_text,
                    crate::scale_f32(40.0),
                    color::ACCENT,
                );
            }
            canvas.fill_rect(
                Point2 {
                    x: Some(margin as i32),
                    y: Some((y + row_h - 1) as i32),
                },
                Vector2 {
                    x: dw - 2 * margin,
                    y: 1,
                },
                color::LIGHT_GRAY,
            );
            self.picker_row_hitboxes.push(mxcfb_rect {
                top: y,
                left: 0,
                width: dw,
                height: row_h,
            });
            y += row_h;
        }
    }
}

impl Scene for EditEventScene {
    fn on_input(&mut self, event: InputEvent) {
        match self.focus {
            Focus::Form => {
                if let InputEvent::MultitouchEvent {
                    event: MultitouchEvent::Release { finger },
                } = event
                {
                    self.handle_form_tap(finger.pos);
                }
            }
            Focus::Title | Focus::Location | Focus::Description => {
                if let Some(kb) = self.keyboard.as_mut() {
                    kb.on_input(event);
                    match kb.outcome() {
                        KeyboardOutcome::Done => {
                            let v = kb.value().to_string();
                            match self.focus {
                                Focus::Title => self.title = v,
                                Focus::Location => self.location = v,
                                Focus::Description => self.description = v,
                                _ => {}
                            }
                            self.keyboard = None;
                            self.focus = Focus::Form;
                            self.needs_redraw = true;
                        }
                        KeyboardOutcome::Cancelled => {
                            self.keyboard = None;
                            self.focus = Focus::Form;
                            self.needs_redraw = true;
                        }
                        KeyboardOutcome::Editing => {}
                    }
                }
            }
            Focus::Start | Focus::End => {
                if let Some(spin) = self.spin.as_mut() {
                    spin.on_input(event);
                    match spin.outcome() {
                        SpinOutcome::Done => {
                            let v = spin.value();
                            match self.focus {
                                Focus::Start => {
                                    // If start moves past end, push end forward
                                    let delta = v.signed_duration_since(self.start_dt);
                                    self.start_dt = v;
                                    if self.end_dt < self.start_dt {
                                        self.end_dt = self.start_dt + chrono::Duration::hours(1);
                                    } else {
                                        // Keep duration roughly stable if positive shift
                                        if delta.num_seconds() > 0 && self.end_dt < self.start_dt {
                                            self.end_dt = self.start_dt + chrono::Duration::hours(1);
                                        }
                                    }
                                }
                                Focus::End => {
                                    self.end_dt = v.max(self.start_dt + chrono::Duration::minutes(15));
                                }
                                _ => {}
                            }
                            self.spin = None;
                            self.focus = Focus::Form;
                            self.needs_redraw = true;
                        }
                        SpinOutcome::Cancelled => {
                            self.spin = None;
                            self.focus = Focus::Form;
                            self.needs_redraw = true;
                        }
                        SpinOutcome::Editing => {}
                    }
                }
            }
            Focus::CalendarPicker => {
                if let InputEvent::MultitouchEvent {
                    event: MultitouchEvent::Release { finger },
                } = event
                {
                    let pos = finger.pos;
                    if Canvas::is_hitting(pos, self.picker_close_hitbox) {
                        self.focus = Focus::Form;
                        self.needs_redraw = true;
                        return;
                    }
                    for (i, hb) in self.picker_row_hitboxes.iter().enumerate() {
                        if Canvas::is_hitting(pos, *hb) {
                            self.selected_calendar_idx = i;
                            self.focus = Focus::Form;
                            self.needs_redraw = true;
                            return;
                        }
                    }
                }
            }
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        match self.focus {
            Focus::Form => {
                if !self.needs_redraw {
                    return;
                }
                self.needs_redraw = false;
                self.render_form(canvas);
                canvas.update_full();
            }
            Focus::Title | Focus::Location | Focus::Description => {
                if let Some(kb) = self.keyboard.as_mut() {
                    kb.draw(canvas);
                }
            }
            Focus::Start | Focus::End => {
                if let Some(spin) = self.spin.as_mut() {
                    spin.draw(canvas);
                }
            }
            Focus::CalendarPicker => {
                if !self.needs_redraw {
                    return;
                }
                self.needs_redraw = false;
                self.render_calendar_picker(canvas);
                canvas.update_full();
            }
        }
    }
}

fn opt_nonblank(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn placeholder_or(s: &str, placeholder: &str) -> String {
    if s.trim().is_empty() {
        placeholder.to_string()
    } else {
        s.to_string()
    }
}

fn round_to_next_half_hour(dt: NaiveDateTime) -> NaiveDateTime {
    use chrono::Timelike;
    let m = dt.minute();
    let bumped = if m < 30 {
        dt.with_minute(30).unwrap_or(dt).with_second(0).unwrap_or(dt)
    } else {
        let next_hour = dt + chrono::Duration::hours(1);
        next_hour
            .with_minute(0)
            .unwrap_or(next_hour)
            .with_second(0)
            .unwrap_or(next_hour)
    };
    bumped.with_nanosecond(0).unwrap_or(bumped)
}
