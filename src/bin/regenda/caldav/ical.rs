use super::types::Event;
use crate::canvas::color;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use ical::parser::ical::component::IcalEvent;
use ical::parser::ical::IcalParser;
use rrule::{RRule, RRuleSet, Tz as RTz, Unvalidated};
use std::collections::HashMap;

/// Parsed properties of a single VEVENT, kept around long enough to
/// resolve RRULE expansion + RECURRENCE-ID overrides at the calendar level.
struct ParsedVEvent {
    event: Event,
    /// TZID parameter on DTSTART, if any (used to anchor RRULE expansion in
    /// the original wall-clock zone — required for DST correctness).
    dtstart_tzid: Option<String>,
    /// Raw RRULE rule body (no `RRULE:` prefix), if present.
    rrule: Option<String>,
    /// RDATE values + their TZID, one entry per comma-separated date.
    rdates: Vec<(String, Option<String>)>,
    /// EXDATE values + their TZID, one entry per comma-separated date.
    exdates: Vec<(String, Option<String>)>,
    /// If present, this VEVENT is an override for the master's instance at
    /// this UTC moment. Master VEVENTs do not have RECURRENCE-ID.
    recurrence_id: Option<DateTime<Utc>>,
}

/// Parse iCalendar data, expand recurring events into instances, and return a
/// flat list of events within `[now-7d, now+30d]`.
pub fn parse_ical_events(
    ical_data: &str,
    calendar_name: &str,
    calendar_color: Option<color>,
) -> Vec<Event> {
    let reader = std::io::BufReader::new(ical_data.as_bytes());
    let parser = IcalParser::new(reader);

    let mut parsed: Vec<ParsedVEvent> = Vec::new();
    for calendar in parser.flatten() {
        for vevent in calendar.events {
            if let Some(p) = parse_vevent(&vevent, calendar_name, calendar_color) {
                parsed.push(p);
            }
        }
    }

    // Match the date-based window used by `fetch_google_events_propfind` in
    // client.rs: an event is in-range if its UTC *date* falls in
    // `[today-7, today+30]`. Using calendar-date bounds (not raw datetime
    // arithmetic) means a recurring 08:45-UTC instance on the boundary day
    // still expands — otherwise we'd drop today-minus-7 morning events.
    let today = Utc::now().date_naive();
    let window_start_date = today - Duration::days(7);
    let window_end_date = today + Duration::days(30);
    let window_start = Utc.from_utc_datetime(
        &window_start_date
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
    );
    let window_end = Utc.from_utc_datetime(
        &window_end_date
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap()),
    );

    // Group by UID so we can pair masters with their RECURRENCE-ID overrides.
    let mut by_uid: HashMap<String, Vec<ParsedVEvent>> = HashMap::new();
    for p in parsed {
        by_uid.entry(p.event.uid.clone()).or_default().push(p);
    }

    let mut out: Vec<Event> = Vec::new();
    for (_uid, group) in by_uid {
        let (overrides, masters): (Vec<_>, Vec<_>) =
            group.into_iter().partition(|p| p.recurrence_id.is_some());

        for master in masters {
            let has_recurrence = master.rrule.is_some() || !master.rdates.is_empty();
            if !has_recurrence {
                if in_window(&master.event, window_start, window_end) {
                    out.push(master.event);
                }
                continue;
            }

            let instances = expand_recurrence(&master, &overrides, window_start, window_end);
            out.extend(instances);
        }

        // Overrides whose master is missing (rare, but possible if the master's
        // already been pruned upstream): emit them as standalone events.
        // We can detect "no master matched" by tracking; here we always re-emit
        // overrides only if they don't match any expanded instance. Since
        // `expand_recurrence` already substitutes overrides into the result,
        // skip this branch when a master existed.
    }

    out
}

fn in_window(event: &Event, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
    event.start >= start && event.start <= end
}

/// Build an `RRuleSet` for `master`, iterate instances in the window, and
/// substitute any RECURRENCE-ID overrides for matching instances.
fn expand_recurrence(
    master: &ParsedVEvent,
    overrides: &[ParsedVEvent],
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Vec<Event> {
    let tz = resolve_rrule_tz(master.dtstart_tzid.as_deref());
    let dt_start: DateTime<RTz> = master.event.start.with_timezone(&tz);

    let mut set = RRuleSet::new(dt_start);

    if let Some(rule_str) = &master.rrule {
        match rule_str.parse::<RRule<Unvalidated>>() {
            Ok(rule) => match rule.validate(dt_start) {
                Ok(validated) => set = set.rrule(validated),
                Err(e) => {
                    log::warn!(
                        "RRULE validate failed for {:?} ({:?}): {}",
                        master.event.summary,
                        rule_str,
                        e
                    );
                    return single_master_fallback(master, window_start, window_end);
                }
            },
            Err(e) => {
                log::warn!(
                    "RRULE parse failed for {:?} ({:?}): {}",
                    master.event.summary,
                    rule_str,
                    e
                );
                return single_master_fallback(master, window_start, window_end);
            }
        }
    }

    for (raw, tzid) in &master.rdates {
        if let Some(dt) = parse_recurrence_date(raw, tzid.as_deref()) {
            set = set.rdate(dt.with_timezone(&tz));
        }
    }
    for (raw, tzid) in &master.exdates {
        if let Some(dt) = parse_recurrence_date(raw, tzid.as_deref()) {
            set = set.exdate(dt.with_timezone(&tz));
        }
    }

    let after = window_start.with_timezone(&tz);
    let before = window_end.with_timezone(&tz);
    let result = set.after(after).before(before).all(500);

    let event_duration: Option<Duration> = master.event.end.map(|e| e - master.event.start);

    // Map RECURRENCE-ID (UTC) -> override
    let mut override_map: HashMap<DateTime<Utc>, &ParsedVEvent> = HashMap::new();
    for o in overrides {
        if let Some(rid) = o.recurrence_id {
            override_map.insert(rid, o);
        }
    }

    let mut events = Vec::with_capacity(result.dates.len());
    for inst in result.dates {
        let inst_utc = inst.with_timezone(&Utc);
        if let Some(o) = override_map.remove(&inst_utc) {
            // Only include the override if it falls within the window itself
            // (an override may have been moved out of the window).
            if in_window(&o.event, window_start, window_end) {
                events.push(o.event.clone());
            }
            continue;
        }

        let mut ev = master.event.clone();
        ev.start = inst_utc;
        ev.end = event_duration.map(|d| inst_utc + d);
        events.push(ev);
    }

    // Any override with no matching expansion (e.g. moved into window from
    // outside, or RECURRENCE-ID didn't line up exactly with the rule): emit
    // it standalone if it lands inside the window.
    for (_, o) in override_map {
        if in_window(&o.event, window_start, window_end) {
            events.push(o.event.clone());
        }
    }

    events
}

/// If RRULE parsing fails, fall back to emitting the master as a single event
/// (matches pre-fix behavior — better than dropping it entirely).
fn single_master_fallback(
    master: &ParsedVEvent,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
) -> Vec<Event> {
    if in_window(&master.event, window_start, window_end) {
        vec![master.event.clone()]
    } else {
        Vec::new()
    }
}

fn resolve_rrule_tz(tzid: Option<&str>) -> RTz {
    if let Some(name) = tzid {
        if let Ok(t) = name.parse::<chrono_tz::Tz>() {
            return t.into();
        }
    }
    RTz::UTC
}

fn parse_recurrence_date(value: &str, tzid: Option<&str>) -> Option<DateTime<Utc>> {
    // Reuse parse_datetime with synthesized params.
    let params: Vec<(String, Vec<String>)> = match tzid {
        Some(t) => vec![("TZID".to_string(), vec![t.to_string()])],
        None => Vec::new(),
    };
    parse_datetime(value, &params).map(|(dt, _)| dt)
}

fn parse_vevent(
    vevent: &IcalEvent,
    calendar_name: &str,
    calendar_color: Option<color>,
) -> Option<ParsedVEvent> {
    let mut summary = String::new();
    let mut uid = String::new();
    let mut dtstart_str = None;
    let mut dtstart_params: Vec<(String, Vec<String>)> = Vec::new();
    let mut dtend_str = None;
    let mut dtend_params: Vec<(String, Vec<String>)> = Vec::new();
    let mut location = None;
    let mut description = None;

    let mut rrule: Option<String> = None;
    let mut rdates: Vec<(String, Option<String>)> = Vec::new();
    let mut exdates: Vec<(String, Option<String>)> = Vec::new();
    let mut recurrence_id: Option<DateTime<Utc>> = None;

    for prop in &vevent.properties {
        match prop.name.as_str() {
            "SUMMARY" => {
                summary = prop.value.clone().unwrap_or_default();
            }
            "UID" => {
                uid = prop.value.clone().unwrap_or_default();
            }
            "DTSTART" => {
                dtstart_str = prop.value.clone();
                if let Some(params) = &prop.params {
                    dtstart_params = params.clone();
                }
            }
            "DTEND" => {
                dtend_str = prop.value.clone();
                if let Some(params) = &prop.params {
                    dtend_params = params.clone();
                }
            }
            "LOCATION" => {
                let val = prop.value.clone().unwrap_or_default();
                if !val.is_empty() {
                    location = Some(val);
                }
            }
            "DESCRIPTION" => {
                let val = prop.value.clone().unwrap_or_default();
                if !val.is_empty() {
                    let val = val
                        .replace("\\n", "\n")
                        .replace("\\,", ",")
                        .replace("\\;", ";")
                        .replace("\\\\", "\\");
                    description = Some(val);
                }
            }
            "RRULE" => {
                if let Some(v) = &prop.value {
                    rrule = Some(v.clone());
                }
            }
            "RDATE" => {
                if let Some(v) = &prop.value {
                    let tzid = extract_tzid(prop.params.as_ref());
                    for part in v.split(',') {
                        let part = part.trim();
                        if !part.is_empty() {
                            rdates.push((part.to_string(), tzid.clone()));
                        }
                    }
                }
            }
            "EXDATE" => {
                if let Some(v) = &prop.value {
                    let tzid = extract_tzid(prop.params.as_ref());
                    for part in v.split(',') {
                        let part = part.trim();
                        if !part.is_empty() {
                            exdates.push((part.to_string(), tzid.clone()));
                        }
                    }
                }
            }
            "RECURRENCE-ID" => {
                if let Some(v) = &prop.value {
                    let params = prop.params.clone().unwrap_or_default();
                    if let Some((dt, _)) = parse_datetime(v, &params) {
                        recurrence_id = Some(dt);
                    }
                }
            }
            _ => {}
        }
    }

    let dtstart_raw = dtstart_str?;
    let (start, all_day) = parse_datetime(&dtstart_raw, &dtstart_params)?;
    let end = dtend_str.and_then(|s| parse_datetime(&s, &dtend_params).map(|(dt, _)| dt));

    let dtstart_tzid = extract_tzid(Some(&dtstart_params));

    Some(ParsedVEvent {
        event: Event {
            uid,
            summary,
            start,
            end,
            location,
            description,
            calendar_name: calendar_name.to_string(),
            calendar_color,
            all_day,
        },
        dtstart_tzid,
        rrule,
        rdates,
        exdates,
        recurrence_id,
    })
}

fn extract_tzid(params: Option<&Vec<(String, Vec<String>)>>) -> Option<String> {
    params?.iter().find_map(|(k, v)| {
        if k == "TZID" {
            v.first().cloned()
        } else {
            None
        }
    })
}

/// Parse a DTSTART/DTEND value, handling DATE, DATETIME, and TZID.
/// Returns (DateTime<Utc>, is_all_day).
fn parse_datetime(
    value: &str,
    params: &[(String, Vec<String>)],
) -> Option<(DateTime<Utc>, bool)> {
    let value = value.trim();

    // All-day event: DATE format YYYYMMDD
    if value.len() == 8 && value.chars().all(|c| c.is_ascii_digit()) {
        let date = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
        let dt = date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
        return Some((Utc.from_utc_datetime(&dt), true));
    }

    // Check for TZID parameter
    let tzid = params.iter().find_map(|(k, v)| {
        if k == "TZID" {
            v.first().cloned()
        } else {
            None
        }
    });

    // DATETIME with Z suffix (UTC)
    if value.ends_with('Z') {
        let clean = value.trim_end_matches('Z');
        let ndt = NaiveDateTime::parse_from_str(clean, "%Y%m%dT%H%M%S").ok()?;
        return Some((Utc.from_utc_datetime(&ndt), false));
    }

    // DATETIME with TZID
    let ndt = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;

    if let Some(tz_name) = tzid {
        if let Ok(tz) = tz_name.parse::<chrono_tz::Tz>() {
            let local = tz.from_local_datetime(&ndt).earliest()?;
            return Some((local.with_timezone(&Utc), false));
        }
    }

    // Assume UTC if no timezone info
    Some((Utc.from_utc_datetime(&ndt), false))
}
