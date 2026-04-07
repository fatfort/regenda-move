use super::Scene;
use crate::caldav::CalendarInfo;
use crate::canvas::{color, mxcfb_rect, Canvas, Point2, Vector2};
use crate::i18n::Strings;
use crate::rmpp_hal::types::{InputEvent, MultitouchEvent};

const HEADER_HEIGHT: u32 = 120;
const ROW_HEIGHT: u32 = 80;
const MARGIN: u32 = 60;

pub struct SettingsScene {
    pub back_pressed: bool,
    pub calendars: Vec<CalendarInfo>,
    strings: &'static Strings,
    back_hitbox: mxcfb_rect,
    cal_hitboxes: Vec<mxcfb_rect>,
    needs_redraw: bool,
}

impl SettingsScene {
    pub fn new(calendars: Vec<CalendarInfo>, strings: &'static Strings) -> Self {
        SettingsScene {
            back_pressed: false,
            calendars,
            strings,
            back_hitbox: mxcfb_rect::default(),
            cal_hitboxes: Vec::new(),
            needs_redraw: true,
        }
    }
}

impl Scene for SettingsScene {
    fn on_input(&mut self, event: InputEvent) {
        if let InputEvent::MultitouchEvent {
            event: MultitouchEvent::Release { finger },
        } = event
        {
            let pos = finger.pos;

            if Canvas::is_hitting(pos, self.back_hitbox) {
                self.back_pressed = true;
                return;
            }

            for (i, hitbox) in self.cal_hitboxes.iter().enumerate() {
                if Canvas::is_hitting(pos, *hitbox) {
                    if i < self.calendars.len() {
                        self.calendars[i].visible = !self.calendars[i].visible;
                        self.needs_redraw = true;
                    }
                    break;
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

        // === Header ===
        canvas.fill_rect(
            Point2 {
                x: Some(0),
                y: Some(0),
            },
            Vector2 {
                x: dw,
                y: HEADER_HEIGHT,
            },
            color::HEADER_BG,
        );

        self.back_hitbox = canvas.draw_text_colored(
            Point2 {
                x: 40.0,
                y: 30.0,
            },
            self.strings.back,
            42.0,
            color::WHITE,
        );
        self.back_hitbox.width += 20;
        self.back_hitbox.height += 20;

        let title = self.strings.cals_to_see;
        let tr = canvas.measure_text(title, 46.0);
        let tx = (dw as f32 - tr.width as f32) / 2.0;
        canvas.draw_text_colored(
            Point2 { x: tx, y: 30.0 },
            title,
            46.0,
            color::WHITE,
        );

        // === Calendar list ===
        self.cal_hitboxes.clear();
        let mut y = HEADER_HEIGHT + 20;
        let mut current_server = String::new();

        for cal in &self.calendars {
            // Server section header
            if cal.server_name != current_server {
                current_server = cal.server_name.clone();
                canvas.fill_rect(
                    Point2 {
                        x: Some(0),
                        y: Some(y as i32),
                    },
                    Vector2 {
                        x: dw,
                        y: ROW_HEIGHT - 10,
                    },
                    color::LIGHT_GRAY,
                );
                canvas.draw_text_colored(
                    Point2 {
                        x: MARGIN as f32,
                        y: (y + 10) as f32,
                    },
                    &current_server,
                    36.0,
                    color::DARK_GRAY,
                );
                y += ROW_HEIGHT;
            }

            let hitbox = mxcfb_rect {
                top: y,
                left: 0,
                width: dw,
                height: ROW_HEIGHT,
            };
            self.cal_hitboxes.push(hitbox);

            // Checkbox
            let checkbox_x = MARGIN as i32;
            let checkbox_y = y as i32 + 15;
            let checkbox_size = 40u32;

            canvas.draw_rect(
                Point2 {
                    x: Some(checkbox_x),
                    y: Some(checkbox_y),
                },
                Vector2 {
                    x: checkbox_size,
                    y: checkbox_size,
                },
                3,
            );

            if cal.visible {
                // Fill checkbox
                canvas.fill_rect(
                    Point2 {
                        x: Some(checkbox_x + 6),
                        y: Some(checkbox_y + 6),
                    },
                    Vector2 {
                        x: checkbox_size - 12,
                        y: checkbox_size - 12,
                    },
                    color::ACCENT,
                );
            }

            // Calendar name
            canvas.draw_text_colored(
                Point2 {
                    x: (MARGIN + checkbox_size + 30) as f32,
                    y: (y + 15) as f32,
                },
                &cal.name,
                38.0,
                color::BLACK,
            );

            // Divider
            canvas.fill_rect(
                Point2 {
                    x: Some(MARGIN as i32),
                    y: Some((y + ROW_HEIGHT - 1) as i32),
                },
                Vector2 {
                    x: dw - 2 * MARGIN,
                    y: 1,
                },
                color::LIGHT_GRAY,
            );

            y += ROW_HEIGHT;
        }

        canvas.update_full();
    }
}
