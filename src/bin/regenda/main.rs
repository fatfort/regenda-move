#![allow(non_camel_case_types, non_snake_case, dead_code, unused_imports)]

#[macro_use]
extern crate downcast_rs;
#[macro_use]
extern crate log;

mod caldav;
mod canvas;
mod config;
mod i18n;
mod rmpp_hal;
mod scene;

use crate::caldav::{cache, FetchStatus};
use chrono::NaiveDate;

/// The most recently-shown top-level view (Day or Week). Sub-views like
/// Event / Settings / Month return to this on back, instead of always
/// resetting to today.
#[derive(Clone, Copy)]
enum TopView {
    Day(NaiveDate),
    Week(NaiveDate),
}

fn build_top_scene(
    view: TopView,
    all_events: &[caldav::Event],
    all_calendars: Vec<caldav::CalendarInfo>,
    strings: &'static i18n::Strings,
    tz: chrono_tz::Tz,
    stale_since: Option<DateTime<Utc>>,
) -> Box<dyn Scene> {
    match view {
        TopView::Day(d) => Box::new(DayScene::new(
            d,
            all_events,
            all_calendars,
            strings,
            tz,
            stale_since,
        )),
        TopView::Week(s) => Box::new(WeeklyScene::new(
            s,
            all_events,
            all_calendars,
            strings,
            tz,
            stale_since,
        )),
    }
}
use crate::canvas::Canvas;
use crate::config::Config;
use crate::rmpp_hal::input::start_input_threads;
use crate::rmpp_hal::types::{DeviceKind, DisplayInfo, InputEvent};
use crate::scene::*;
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::sleep;
use std::time::{Duration, SystemTime};

/// Runtime-detected display info. Populated by `init_display_info` once the
/// QTFB backend has detected the device. Scenes read it through the helper
/// functions below to derive sizes, fonts, and layout.
static DISPLAY_INFO: OnceLock<DisplayInfo> = OnceLock::new();

pub fn init_display_info(info: DisplayInfo) {
    let _ = DISPLAY_INFO.set(info);
}

fn display_info() -> DisplayInfo {
    *DISPLAY_INFO.get().expect("display info not initialised")
}

pub fn display_width() -> u32 {
    display_info().width
}

pub fn display_height() -> u32 {
    display_info().height
}

pub fn ui_scale() -> f32 {
    display_info().ui_scale
}

pub fn device_kind() -> DeviceKind {
    display_info().device
}

/// Round `n` (rMPP-native pixels) by the runtime UI scale to give the
/// device-correct equivalent. Matches the manual `(x * 7.3/11.8).round()`
/// values from the move branch.
pub fn scale_u32(n: u32) -> u32 {
    (n as f32 * ui_scale()).round() as u32
}

pub fn scale_f32(f: f32) -> f32 {
    f * ui_scale()
}

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "INFO") };
    }
    env_logger::init();
    info!("reGenda starting");

    // Handle SIGTERM gracefully (AppLoad sends this on swipe-to-close)
    setup_signal_handler();

    // Load config
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            error!("Config error: {:?}", e);
            // We'll show the error in the loading scene
            run_with_error(&format!("{:?}", e));
            return;
        }
    };

    let strings = i18n::get_strings(config.language_str());
    let tz: chrono_tz::Tz = config
        .timezone_str()
        .parse()
        .unwrap_or(chrono_tz::UTC);

    info!("Timezone: {}, Language: {}", tz, config.language_str());

    // Initialize display (also populates DISPLAY_INFO for scene scaling)
    let mut canvas = Canvas::new();
    init_display_info(canvas.display_info());
    let (input_tx, input_rx) = std::sync::mpsc::channel::<InputEvent>();
    start_input_threads(input_tx, canvas.qtfb_fd());

    // Offline-first: try loading the cache before hitting the network. If
    // a cache exists, we skip the loading scene entirely and render with
    // cached events immediately; the background refresh still runs and will
    // swap in fresh data (clearing the stale banner) when it succeeds.
    let cache_path = cache::resolve_path(config.cache_path.as_deref());
    let cached = cache::load(&cache_path);

    // Shared app state
    let mut all_events: Vec<caldav::Event> = Vec::new();
    let mut all_calendars: Vec<caldav::CalendarInfo> = Vec::new();
    let mut current_stale_since: Option<DateTime<Utc>> = None;

    let fetch_status = if let Some(ref c) = cached {
        all_events = c.events.clone();
        all_calendars = c.calendars.clone();
        current_stale_since = Some(c.fetched_at);
        Arc::new(Mutex::new(FetchStatus::Done {
            calendars: c.calendars.clone(),
            events: c.events.clone(),
            stale_since: Some(c.fetched_at),
            pending_oauth: Vec::new(),
        }))
    } else {
        Arc::new(Mutex::new(FetchStatus::Loading {
            message: strings.loading.to_string(),
        }))
    };

    // Sources the user has cancelled out of OAuth for during this session.
    // Avoids re-prompting on every refresh — re-launch regenda to retry.
    // In-memory only by design (per spec): cancellation shouldn't persist
    // across restarts.
    let mut dismissed_oauth: HashSet<String> = HashSet::new();

    // Tracks the user's last top-level view so sub-scenes return back to it
    // on cancel/close instead of jumping to today.
    let mut last_top_view: TopView = TopView::Day(chrono::Local::now().date_naive());

    // Always kick off a background refresh on startup — fresh data wins when
    // online; offline re-runs silently keep showing the cached snapshot.
    {
        let status = fetch_status.clone();
        let config_clone = config.clone();
        std::thread::spawn(move || {
            let result = caldav::fetch_all(&config_clone);
            *status.lock().unwrap() = result;
        });
    }

    // Main loop
    const FPS: u16 = 10;
    const FRAME_DURATION: Duration = Duration::from_millis(1000 / FPS as u64);

    // If we already have cached data, render the Day view immediately. Otherwise
    // show the loading scene and wait for the first fetch result.
    let mut current_scene: Box<dyn Scene> = if cached.is_some() {
        let today = chrono::Local::now().date_naive();
        Box::new(DayScene::new(
            today,
            &all_events,
            all_calendars.clone(),
            strings,
            tz,
            current_stale_since,
        ))
    } else {
        Box::new(LoadingScene::new(fetch_status.clone(), strings))
    };

    loop {
        let before_input = SystemTime::now();
        for event in input_rx.try_iter() {
            current_scene.on_input(event);
        }

        // Background refresh poll: if the spawned fetch thread has produced
        // fresh (or fallback-cached) results, fold them into the live DayScene
        // without leaving the scene. Staying in-place avoids a loading-screen
        // flicker on every successful background refresh.
        apply_background_refresh(
            &mut current_scene,
            &fetch_status,
            &mut all_events,
            &mut all_calendars,
            &mut current_stale_since,
        );

        current_scene.draw(&mut canvas);
        current_scene = update(
            current_scene,
            &mut canvas,
            &fetch_status,
            &config,
            strings,
            tz,
            &mut all_events,
            &mut all_calendars,
            &mut current_stale_since,
            &mut dismissed_oauth,
            &mut last_top_view,
        );

        let elapsed = before_input.elapsed().unwrap();
        if elapsed < FRAME_DURATION {
            sleep(FRAME_DURATION - elapsed);
        }
    }
}

/// Check the shared fetch status and, if the current scene is a `DayScene`,
/// fold any completed background-refresh result into it in place.
fn apply_background_refresh(
    current_scene: &mut Box<dyn Scene>,
    fetch_status: &Arc<Mutex<FetchStatus>>,
    all_events: &mut Vec<caldav::Event>,
    all_calendars: &mut Vec<caldav::CalendarInfo>,
    current_stale_since: &mut Option<DateTime<Utc>>,
) {
    let status = fetch_status.lock().unwrap().clone();
    let FetchStatus::Done {
        calendars,
        events,
        stale_since,
        pending_oauth: _,
    } = status
    else {
        return;
    };

    if let Some(day) = current_scene.downcast_mut::<DayScene>() {
        // Only apply if the snapshot differs from what DayScene is already
        // showing. Compares `stale_since` (None = fresh) and event count to
        // avoid re-applying the same snapshot every frame.
        if day.stale_since == stale_since && day.events_total() == events.len() {
            return;
        }
        *all_events = events.clone();
        *all_calendars = calendars.clone();
        *current_stale_since = stale_since;
        day.apply_refresh(events, calendars, stale_since);
        return;
    }

    if let Some(week) = current_scene.downcast_mut::<WeeklyScene>() {
        if week.stale_since == stale_since && week.events_total() == events.len() {
            return;
        }
        *all_events = events.clone();
        *all_calendars = calendars.clone();
        *current_stale_since = stale_since;
        week.apply_refresh(events, calendars, stale_since);
    }
}

#[allow(clippy::too_many_arguments)]
fn update(
    scene: Box<dyn Scene>,
    canvas: &mut Canvas,
    fetch_status: &Arc<Mutex<FetchStatus>>,
    config: &Config,
    strings: &'static i18n::Strings,
    tz: chrono_tz::Tz,
    all_events: &mut Vec<caldav::Event>,
    all_calendars: &mut Vec<caldav::CalendarInfo>,
    current_stale_since: &mut Option<DateTime<Utc>>,
    dismissed_oauth: &mut HashSet<String>,
    last_top_view: &mut TopView,
) -> Box<dyn Scene> {
    // Snapshot the current top-level view (Day or Week) every frame so that
    // in-scene navigation (e.g. paging through days inside DayScene) is
    // captured for "back" purposes — sub-scenes will return to this exact
    // state instead of always jumping to today.
    if let Some(day) = scene.downcast_ref::<DayScene>() {
        *last_top_view = TopView::Day(day.current_date);
    } else if let Some(week) = scene.downcast_ref::<WeeklyScene>() {
        *last_top_view = TopView::Week(week.current_week_start);
    }

    // Loading scene transitions
    if let Some(loading) = scene.downcast_ref::<LoadingScene>() {
        // OAuth takes precedence over DayScene transition: if any pending
        // source hasn't been dismissed this session, route through OAuthScene.
        // This applies whether `needs_oauth` came from FetchStatus::Done's
        // pending_oauth (partial success) or NeedsOAuth (everything failed).
        if let Some(server_name) = loading
            .needs_oauth
            .iter()
            .find(|s| !dismissed_oauth.contains(*s))
            .cloned()
        {
            if let Some(server_config) = config.sources.get(&server_name) {
                return Box::new(OAuthScene::new(server_name, server_config, strings));
            }
        }
        if loading.data_ready {
            // Extract data from fetch status
            let status = fetch_status.lock().unwrap().clone();
            if let FetchStatus::Done {
                calendars,
                events,
                stale_since,
                pending_oauth: _,
            } = status
            {
                *all_calendars = calendars;
                *all_events = events;
                *current_stale_since = stale_since;
            }
            return build_top_scene(
                *last_top_view,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            );
        }
        if loading.retry_pressed {
            // Retry fetch
            let status_clone = fetch_status.clone();
            let config_clone = config.clone();
            *fetch_status.lock().unwrap() = FetchStatus::Loading {
                message: strings.loading.to_string(),
            };
            std::thread::spawn(move || {
                let result = caldav::fetch_all(&config_clone);
                *status_clone.lock().unwrap() = result;
            });
            return Box::new(LoadingScene::new(fetch_status.clone(), strings));
        }
    }

    // OAuth scene transitions
    if let Some(oauth) = scene.downcast_ref::<OAuthScene>() {
        if oauth.auth_complete || oauth.cancel_pressed {
            // Cancel marks the source as dismissed so the next refresh
            // doesn't immediately re-prompt for the same source. auth_complete
            // does NOT dismiss — the next fetch will see the new token, drop
            // the source from pending_oauth, and the user moves on naturally.
            if oauth.cancel_pressed {
                dismissed_oauth.insert(oauth.server_name.clone());
            }
            // Re-fetch all calendars (token is now stored)
            let status_clone = fetch_status.clone();
            let config_clone = config.clone();
            *fetch_status.lock().unwrap() = FetchStatus::Loading {
                message: strings.loading.to_string(),
            };
            std::thread::spawn(move || {
                let result = caldav::fetch_all(&config_clone);
                *status_clone.lock().unwrap() = result;
            });
            return Box::new(LoadingScene::new(fetch_status.clone(), strings));
        }
    }

    // Day scene transitions
    if let Some(day) = scene.downcast_ref::<DayScene>() {
        if day.go_to_month {
            return Box::new(MonthScene::new(
                day.current_date,
                all_events.clone(),
                strings,
                tz,
            ));
        }
        if day.go_to_week {
            return Box::new(WeeklyScene::new(
                day.current_date,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            ));
        }
        if day.go_to_create {
            let writable = writable_calendars(config, all_calendars);
            if !writable.is_empty() {
                return Box::new(EditEventScene::new(
                    EditMode::Create,
                    writable,
                    day.current_date,
                    strings,
                    tz,
                ));
            }
        }
        if day.go_to_settings {
            return Box::new(SettingsScene::new(
                all_calendars.clone(),
                strings,
            ));
        }
        if let Some(idx) = day.go_to_event {
            if idx < day.events.len() {
                return Box::new(EventScene::new(
                    day.events[idx].clone(),
                    strings,
                    tz,
                ));
            }
        }
        if day.refresh_pressed {
            let status_clone = fetch_status.clone();
            let config_clone = config.clone();
            *fetch_status.lock().unwrap() = FetchStatus::Loading {
                message: strings.refreshing.to_string(),
            };
            std::thread::spawn(move || {
                let result = caldav::fetch_all(&config_clone);
                *status_clone.lock().unwrap() = result;
            });
            return Box::new(LoadingScene::new(fetch_status.clone(), strings));
        }
        if day.exit_pressed {
            canvas.clear();
            canvas.update_full();
            std::process::exit(0);
        }
        // Day change: re-filter events
        // (handled internally by DayScene's draw)
    }

    // Month scene transitions
    if let Some(month) = scene.downcast_ref::<MonthScene>() {
        if month.back_pressed {
            // Back returns to whichever top view (Day/Week) the user came from,
            // ignoring any tap-selected date (selected_date is for forward
            // navigation, handled below).
            return build_top_scene(
                *last_top_view,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            );
        }
        if let Some(date) = month.selected_date {
            return Box::new(DayScene::new(
                date,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            ));
        }
    }

    // Event scene transitions
    if let Some(event_scene) = scene.downcast_ref::<EventScene>() {
        if event_scene.back_pressed {
            return build_top_scene(
                *last_top_view,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            );
        }
        if event_scene.edit_pressed {
            let writable = writable_calendars(config, all_calendars);
            return Box::new(EditEventScene::new(
                EditMode::Edit(event_scene.event.clone()),
                writable,
                event_scene.event.date_in_tz(&tz),
                strings,
                tz,
            ));
        }
        if event_scene.delete_confirmed {
            if let (Some(cal_id), Some(event_id)) = (
                event_scene.event.source_calendar_id.clone(),
                event_scene.event.source_event_id.clone(),
            ) {
                let status_clone = fetch_status.clone();
                let config_clone = config.clone();
                *fetch_status.lock().unwrap() = FetchStatus::Loading {
                    message: strings.deleting.to_string(),
                };
                std::thread::spawn(move || {
                    if let Err(e) =
                        caldav::delete_event(&config_clone, &cal_id, &event_id)
                    {
                        log::error!("Calendar v3 delete failed: {:?}", e);
                        *status_clone.lock().unwrap() = FetchStatus::Error {
                            message: format!("Delete failed: {}", e),
                        };
                        return;
                    }
                    let result = caldav::fetch_all(&config_clone);
                    *status_clone.lock().unwrap() = result;
                });
                return Box::new(LoadingScene::new(fetch_status.clone(), strings));
            }
        }
    }

    // Edit/Create event scene transitions
    if let Some(edit) = scene.downcast_ref::<EditEventScene>() {
        if edit.cancel_pressed {
            return build_top_scene(
                *last_top_view,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            );
        }
        if let Some(req) = &edit.save_request {
            let req_clone = req.clone();
            let status_clone = fetch_status.clone();
            let config_clone = config.clone();
            *fetch_status.lock().unwrap() = FetchStatus::Loading {
                message: strings.saving.to_string(),
            };
            std::thread::spawn(move || {
                let outcome: anyhow::Result<()> = match &req_clone.mode {
                    scene::SaveMode::Insert => caldav::insert_event(
                        &config_clone,
                        &req_clone.calendar_id,
                        &req_clone.write,
                    )
                    .map(|_id| ()),
                    scene::SaveMode::Patch { event_id } => caldav::patch_event(
                        &config_clone,
                        &req_clone.calendar_id,
                        event_id,
                        &req_clone.write,
                    ),
                };
                if let Err(e) = outcome {
                    log::error!("Calendar v3 write failed: {:?}", e);
                    *status_clone.lock().unwrap() = FetchStatus::Error {
                        message: format!("Save failed: {}", e),
                    };
                    return;
                }
                let result = caldav::fetch_all(&config_clone);
                *status_clone.lock().unwrap() = result;
            });
            return Box::new(LoadingScene::new(fetch_status.clone(), strings));
        }
    }

    // Weekly scene transitions
    if let Some(week) = scene.downcast_ref::<WeeklyScene>() {
        if let Some(date) = week.go_to_day {
            return Box::new(DayScene::new(
                date,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            ));
        }
        if week.go_to_create {
            let writable = writable_calendars(config, all_calendars);
            if !writable.is_empty() {
                return Box::new(EditEventScene::new(
                    EditMode::Create,
                    writable,
                    week.current_week_start,
                    strings,
                    tz,
                ));
            }
        }
        if let Some(idx) = week.go_to_event {
            if idx < week.events.len() {
                return Box::new(EventScene::new(
                    week.events[idx].clone(),
                    strings,
                    tz,
                ));
            }
        }
        if week.go_to_month {
            return Box::new(MonthScene::new(
                week.current_week_start,
                all_events.clone(),
                strings,
                tz,
            ));
        }
        if week.go_to_settings {
            return Box::new(SettingsScene::new(
                all_calendars.clone(),
                strings,
            ));
        }
        if week.refresh_pressed {
            let status_clone = fetch_status.clone();
            let config_clone = config.clone();
            *fetch_status.lock().unwrap() = FetchStatus::Loading {
                message: strings.refreshing.to_string(),
            };
            std::thread::spawn(move || {
                let result = caldav::fetch_all(&config_clone);
                *status_clone.lock().unwrap() = result;
            });
            return Box::new(LoadingScene::new(fetch_status.clone(), strings));
        }
        if week.exit_pressed {
            canvas.clear();
            canvas.update_full();
            std::process::exit(0);
        }
    }

    // Settings scene transitions
    if let Some(settings) = scene.downcast_ref::<SettingsScene>() {
        if settings.back_pressed {
            // Update calendar visibility
            *all_calendars = settings.calendars.clone();
            return build_top_scene(
                *last_top_view,
                all_events,
                all_calendars.clone(),
                strings,
                tz,
                *current_stale_since,
            );
        }
    }

    // Cross-scene OAuth fallback: if a background refresh produced a Done with
    // pending OAuth sources and the user is sitting on a non-Loading/non-OAuth
    // scene (e.g. DayScene from a cached startup), pull them into OAuthScene
    // so the device-auth flow is reachable. Skipped for LoadingScene (handled
    // above) and OAuthScene (already there).
    if scene.downcast_ref::<OAuthScene>().is_none()
        && scene.downcast_ref::<LoadingScene>().is_none()
    {
        let status = fetch_status.lock().unwrap().clone();
        if let FetchStatus::Done { pending_oauth, .. } = status {
            if let Some(server_name) = pending_oauth
                .iter()
                .find(|s| !dismissed_oauth.contains(*s))
                .cloned()
            {
                if let Some(server_config) = config.sources.get(&server_name) {
                    return Box::new(OAuthScene::new(server_name, server_config, strings));
                }
            }
        }
    }

    scene
}

/// Filter the in-memory calendar list down to ones the v3 write API can
/// target — every Google source whose name appears in the live config. ICS
/// sources and basic-auth CalDAV sources are excluded; the brief explicitly
/// scopes writes to Google for v1.
fn writable_calendars(
    config: &Config,
    all_calendars: &[caldav::CalendarInfo],
) -> Vec<caldav::CalendarInfo> {
    all_calendars
        .iter()
        .filter(|c| {
            config
                .sources
                .get(&c.server_name)
                .map(|s| s.is_google())
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn run_with_error(error: &str) {
    let mut canvas = Canvas::new();
    init_display_info(canvas.display_info());
    let (input_tx, input_rx) = std::sync::mpsc::channel::<InputEvent>();
    start_input_threads(input_tx, canvas.qtfb_fd());

    canvas.clear();

    let dw = canvas.display_width();
    let title = "reGenda - Configuration Error";
    let tr = canvas.measure_text(title, 52.0);
    let tx = (dw as f32 - tr.width as f32) / 2.0;
    canvas.draw_text_colored(
        canvas::Point2 { x: tx, y: 400.0 },
        title,
        52.0,
        canvas::color::BLACK,
    );

    canvas.draw_multi_line_text(
        Some(60),
        550,
        error,
        50,
        20,
        36.0,
        0.3,
        canvas::color::BLACK,
    );

    let hint = "Place config at /home/root/.config/reGenda/config.yml";
    let hr = canvas.measure_text(hint, 36.0);
    let hx = (dw as f32 - hr.width as f32) / 2.0;
    canvas.draw_text_colored(
        canvas::Point2 { x: hx, y: 1000.0 },
        hint,
        36.0,
        canvas::color::MEDIUM_GRAY,
    );

    canvas.update_full();

    // Wait for input or SIGTERM
    loop {
        for _ in input_rx.try_iter() {}
        sleep(Duration::from_millis(500));
    }
}

fn setup_signal_handler() {
    unsafe {
        libc::signal(libc::SIGTERM, handle_sigterm as *const () as libc::sighandler_t);
    }
}

extern "C" fn handle_sigterm(_sig: libc::c_int) {
    info!("Received SIGTERM, exiting gracefully");
    std::process::exit(0);
}
