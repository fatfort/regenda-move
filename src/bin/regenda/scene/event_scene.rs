use super::Scene;
use crate::caldav::Event;
use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};

fn header_height() -> u32 {
    crate::scale_u32(120)
}
const MARGIN: u32 = 60;
const CONTENT_TOP: u32 = 160;

pub struct EventScene {
    pub back_pressed: bool,
    pub edit_pressed: bool,
    pub delete_confirmed: bool,
    pub event: Event,
    strings: &'static Strings,
    tz: chrono_tz::Tz,
    back_hitbox: mxcfb_rect,
    edit_hitbox: mxcfb_rect,
    delete_hitbox: mxcfb_rect,
    confirm_yes_hitbox: mxcfb_rect,
    confirm_no_hitbox: mxcfb_rect,
    delete_modal: bool,
    needs_redraw: bool,
}

impl EventScene {
    pub fn new(event: Event, strings: &'static Strings, tz: chrono_tz::Tz) -> Self {
        EventScene {
            back_pressed: false,
            edit_pressed: false,
            delete_confirmed: false,
            event,
            strings,
            tz,
            back_hitbox: mxcfb_rect::default(),
            edit_hitbox: mxcfb_rect::default(),
            delete_hitbox: mxcfb_rect::default(),
            confirm_yes_hitbox: mxcfb_rect::default(),
            confirm_no_hitbox: mxcfb_rect::default(),
            delete_modal: false,
            needs_redraw: true,
        }
    }

    fn writable(&self) -> bool {
        self.event.source_calendar_id.is_some() && self.event.source_event_id.is_some()
    }
}

impl Scene for EventScene {
    fn on_input(&mut self, event: InputEvent) {
        if let InputEvent::MultitouchEvent {
            event: MultitouchEvent::Release { finger },
        } = event
        {
            let pos = finger.pos;
            if self.delete_modal {
                if Canvas::is_hitting(pos, self.confirm_yes_hitbox) {
                    self.delete_confirmed = true;
                    return;
                }
                if Canvas::is_hitting(pos, self.confirm_no_hitbox) {
                    self.delete_modal = false;
                    self.needs_redraw = true;
                    return;
                }
                return;
            }
            if Canvas::is_hitting(pos, self.back_hitbox) {
                self.back_pressed = true;
                return;
            }
            if self.writable() && Canvas::is_hitting(pos, self.edit_hitbox) {
                self.edit_pressed = true;
                return;
            }
            if self.writable() && Canvas::is_hitting(pos, self.delete_hitbox) {
                self.delete_modal = true;
                self.needs_redraw = true;
                return;
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

        // === Header ===
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(0),
            },
            Vector2 {
                x: dw,
                y: header_height(),
            },
            color::HEADER_BG,
        );

        // Back button
        let back_pad = crate::scale_u32(20);
        self.back_hitbox = canvas.draw_text_colored(
            Point2 {
                x: 40.0,
                y: crate::scale_f32(30.0),
            },
            self.strings.back,
            crate::scale_f32(42.0),
            color::WHITE,
        );
        self.back_hitbox.width += back_pad;
        self.back_hitbox.height += back_pad;

        // Title text
        let title = self.strings.event_details;
        let title_font = crate::scale_f32(46.0);
        let tr = canvas.measure_text(title, title_font);
        let tx = (dw as f32 - tr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: tx, y: crate::scale_f32(30.0) },
            title,
            title_font,
            color::WHITE,
        );

        // Per-calendar color accent strip (sits in the gap below the header)
        let accent_color = self.event.calendar_color.unwrap_or(color::DARK_GRAY);
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(header_height() as i32),
            },
            Vector2 {
                x: dw,
                y: 14,
            },
            accent_color,
        );

        // === Content ===
        let mut y = CONTENT_TOP as f32;

        // Event title
        canvas.draw_text_colored(
            Point2 {
                x: MARGIN as f32,
                y,
            },
            &self.event.summary,
            crate::scale_f32(60.0),
            color::BLACK,
        );
        y += 90.0;

        // Divider
        canvas.fill_rect(
            Point2 {
                x: Some(MARGIN as i32),
                y: Some(y as i32),
            },
            Vector2 {
                x: dw - 2 * MARGIN,
                y: 2,
            },
            color::LIGHT_GRAY,
        );
        y += 30.0;

        // Time
        if self.event.all_day {
            canvas.draw_text_colored(
                Point2 {
                    x: MARGIN as f32,
                    y,
                },
                self.strings.allday,
                42.0,
                color::BLACK,
            );
            y += 60.0;
        } else {
            let start_str = format!(
                "{}{}",
                self.strings.start_label,
                self.event.start_datetime_str(&self.tz)
            );
            canvas.draw_text_colored(
                Point2 {
                    x: MARGIN as f32,
                    y,
                },
                &start_str,
                38.0,
                color::BLACK,
            );
            y += 55.0;

            if let Some(end_str) = self.event.end_datetime_str(&self.tz) {
                let end_display = format!("{}{}", self.strings.end_label, end_str);
                canvas.draw_text_colored(
                    Point2 {
                        x: MARGIN as f32,
                        y,
                    },
                    &end_display,
                    38.0,
                    color::BLACK,
                );
                y += 55.0;
            }
        }

        y += 10.0;

        // Location
        if let Some(ref loc) = self.event.location {
            let loc_str = format!("{}{}", self.strings.location_label, loc);
            canvas.draw_text_colored(
                Point2 {
                    x: MARGIN as f32,
                    y,
                },
                &loc_str,
                38.0,
                color::BLACK,
            );
            y += 55.0;
        }

        // Calendar name (use calendar's own color if available)
        let cal_color = self.event.calendar_color.unwrap_or(color::ACCENT);
        let cal_str = format!("{}{}", self.strings.calendar_label, self.event.calendar_name);
        canvas.draw_text_colored(
            Point2 {
                x: MARGIN as f32,
                y,
            },
            &cal_str,
            34.0,
            cal_color,
        );
        y += 55.0;

        // Divider
        canvas.fill_rect(
            Point2 {
                x: Some(MARGIN as i32),
                y: Some(y as i32),
            },
            Vector2 {
                x: dw - 2 * MARGIN,
                y: 2,
            },
            color::LIGHT_GRAY,
        );
        y += 30.0;

        // Description
        if let Some(ref desc) = self.event.description {
            canvas.draw_multi_line_text(
                Some(MARGIN as i32),
                y as i32,
                desc,
                55,
                30,
                34.0,
                0.3,
                color::BLACK,
            );
        }

        // === Edit / Delete buttons (bottom bar) ===
        let dh = crate::display_height();
        let bottom_h = crate::scale_u32(120);
        let bottom_y = (dh - bottom_h) as i32;
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(bottom_y),
            },
            Vector2 { x: dw, y: 2 },
            color::LIGHT_GRAY,
        );

        let writable = self.writable();
        let btn_font = crate::scale_f32(40.0);
        let btn_y = (bottom_y + crate::scale_u32(35) as i32) as f32;

        let half = (dw / 2) as i32;

        // Edit button (left half)
        let edit_color = if writable { color::BLACK } else { color::MEDIUM_GRAY };
        let edit_text = self.strings.edit_event;
        let er = canvas.measure_text(edit_text, btn_font);
        let ex = (half as f32 - er.width as f32) / 2.0;
        let edit_rect = canvas.draw_text_colored(
            Point2 { x: ex, y: btn_y },
            edit_text,
            btn_font,
            edit_color,
        );
        self.edit_hitbox = mxcfb_rect {
            top: bottom_y as u32,
            left: 0,
            width: dw / 2,
            height: bottom_h,
        };
        let _ = edit_rect;

        // Vertical divider
        canvas.fill_rect(
            Point2 {
                x: Some(half),
                y: Some(bottom_y),
            },
            Vector2 { x: 1, y: bottom_h },
            color::LIGHT_GRAY,
        );

        // Delete button (right half)
        let delete_color = if writable { color::BLACK } else { color::MEDIUM_GRAY };
        let delete_text = self.strings.delete_event;
        let dr = canvas.measure_text(delete_text, btn_font);
        let dx = half as f32 + (half as f32 - dr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: dx, y: btn_y },
            delete_text,
            btn_font,
            delete_color,
        );
        self.delete_hitbox = mxcfb_rect {
            top: bottom_y as u32,
            left: dw / 2,
            width: dw / 2,
            height: bottom_h,
        };

        if !writable {
            // Show a small read-only hint above the bottom bar
            let hint = self.strings.readonly_event;
            let hr = canvas.measure_text(hint, crate::scale_f32(28.0));
            let hx = (dw as f32 - hr.width as f32) / 2.0;
            canvas.draw_text_colored(
                Point2 {
                    x: hx,
                    y: (bottom_y - crate::scale_u32(40) as i32) as f32,
                },
                hint,
                crate::scale_f32(28.0),
                color::MEDIUM_GRAY,
            );
        }

        if self.delete_modal {
            self.draw_delete_modal(canvas);
        }

        canvas.update_full();
    }
}

impl EventScene {
    fn draw_delete_modal(&mut self, canvas: &mut Canvas) {
        let dw = canvas.display_width();
        let dh = crate::display_height();

        // Dim the background by drawing a translucent overlay (e-ink: just a
        // light fill clipped to the edges so the modal is visually distinct).
        let modal_w = (dw as f32 * 0.7) as u32;
        let modal_h = crate::scale_u32(360);
        let modal_x = (dw - modal_w) / 2;
        let modal_y = (dh - modal_h) / 2;

        canvas.fill_rect(
            Point2 {
                x: Some(modal_x as i32),
                y: Some(modal_y as i32),
            },
            Vector2 { x: modal_w, y: modal_h },
            color::WHITE,
        );
        canvas.draw_rect(
            Point2 {
                x: Some(modal_x as i32),
                y: Some(modal_y as i32),
            },
            Vector2 { x: modal_w, y: modal_h },
            4,
        );

        // Title
        let title = self.strings.confirm_delete;
        let tf = crate::scale_f32(46.0);
        let tr = canvas.measure_text(title, tf);
        canvas.draw_text_colored(
            Point2 {
                x: (modal_x + (modal_w - tr.width) / 2) as f32,
                y: (modal_y + crate::scale_u32(40)) as f32,
            },
            title,
            tf,
            color::BLACK,
        );

        // Message
        let msg = self.strings.delete_confirm_msg;
        let mf = crate::scale_f32(34.0);
        let mr = canvas.measure_text(msg, mf);
        canvas.draw_text_colored(
            Point2 {
                x: (modal_x + (modal_w - mr.width) / 2) as f32,
                y: (modal_y + crate::scale_u32(120)) as f32,
            },
            msg,
            mf,
            color::DARK_GRAY,
        );

        // Show event title underneath for confirmation
        let snippet = if self.event.summary.len() > 40 {
            format!("{}…", &self.event.summary.chars().take(40).collect::<String>())
        } else {
            self.event.summary.clone()
        };
        let sf = crate::scale_f32(32.0);
        let sr = canvas.measure_text(&snippet, sf);
        canvas.draw_text_colored(
            Point2 {
                x: (modal_x + (modal_w - sr.width) / 2) as f32,
                y: (modal_y + crate::scale_u32(170)) as f32,
            },
            &snippet,
            sf,
            color::BLACK,
        );

        // Buttons
        let btn_y = modal_y + modal_h - crate::scale_u32(90);
        let btn_font = crate::scale_f32(38.0);
        let btn_pad = crate::scale_u32(24);

        // Cancel (No)
        let no_text = self.strings.no;
        let nr = canvas.measure_text(no_text, btn_font);
        let no_x = modal_x + crate::scale_u32(60);
        let mut no_rect = canvas.draw_text_colored(
            Point2 {
                x: no_x as f32,
                y: btn_y as f32,
            },
            no_text,
            btn_font,
            color::BLACK,
        );
        no_rect.left = no_rect.left.saturating_sub(btn_pad);
        no_rect.top = no_rect.top.saturating_sub(btn_pad / 2);
        no_rect.width += 2 * btn_pad;
        no_rect.height += btn_pad;
        canvas.draw_rect(
            Point2 {
                x: Some(no_rect.left as i32),
                y: Some(no_rect.top as i32),
            },
            Vector2 {
                x: no_rect.width,
                y: no_rect.height,
            },
            3,
        );
        self.confirm_no_hitbox = no_rect;
        let _ = nr;

        // Confirm (Yes)
        let yes_text = self.strings.yes;
        let yr = canvas.measure_text(yes_text, btn_font);
        let yes_x = modal_x + modal_w - crate::scale_u32(60) - yr.width;
        let mut yes_rect = canvas.draw_text_colored(
            Point2 {
                x: yes_x as f32,
                y: btn_y as f32,
            },
            yes_text,
            btn_font,
            color::BLACK,
        );
        yes_rect.left = yes_rect.left.saturating_sub(btn_pad);
        yes_rect.top = yes_rect.top.saturating_sub(btn_pad / 2);
        yes_rect.width += 2 * btn_pad;
        yes_rect.height += btn_pad;
        canvas.draw_rect(
            Point2 {
                x: Some(yes_rect.left as i32),
                y: Some(yes_rect.top as i32),
            },
            Vector2 {
                x: yes_rect.width,
                y: yes_rect.height,
            },
            3,
        );
        self.confirm_yes_hitbox = yes_rect;
    }
}
