use crate::canvas::color;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Information about a discovered calendar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarInfo {
    pub name: String,
    pub path: String,
    pub color: Option<String>,
    pub visible: bool,
    pub server_name: String,
}

/// A single calendar event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub calendar_name: String,
    pub calendar_color: Option<color>,
    pub all_day: bool,
}

/// Parse a hex color string like "#0B8043" or "#0B8043FF" into a color.
pub fn parse_hex_color(hex: &str) -> Option<color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(color { r, g, b })
}

impl Event {
    /// Get the date of this event in the given timezone offset.
    pub fn date_in_tz(&self, tz: &chrono_tz::Tz) -> NaiveDate {
        use chrono::TimeZone;
        tz.from_utc_datetime(&self.start.naive_utc())
            .date_naive()
    }

    /// Last date this event occupies in the given timezone. For point events
    /// (no end), equals `date_in_tz`. For all-day events DTEND is exclusive
    /// per RFC 5545, so we subtract one day. For datetime events DTEND is
    /// inclusive, so we use the end date as-is.
    pub fn end_date_in_tz(&self, tz: &chrono_tz::Tz) -> NaiveDate {
        use chrono::TimeZone;
        match self.end {
            None => self.date_in_tz(tz),
            Some(end) => {
                let end_date = tz.from_utc_datetime(&end.naive_utc()).date_naive();
                if self.all_day && end_date > self.date_in_tz(tz) {
                    end_date - chrono::Duration::days(1)
                } else {
                    end_date
                }
            }
        }
    }

    /// Whether this event is visible on `date` in the given timezone.
    /// True for any date between start and end-date (inclusive).
    pub fn spans_date(&self, date: NaiveDate, tz: &chrono_tz::Tz) -> bool {
        let start = self.date_in_tz(tz);
        let end = self.end_date_in_tz(tz);
        date >= start && date <= end
    }

    /// Format the start time as HH:MM in the given timezone.
    pub fn start_time_str(&self, tz: &chrono_tz::Tz) -> String {
        use chrono::TimeZone;
        let local = tz.from_utc_datetime(&self.start.naive_utc());
        local.format("%H:%M").to_string()
    }

    /// Format the end time as HH:MM in the given timezone.
    pub fn end_time_str(&self, tz: &chrono_tz::Tz) -> Option<String> {
        self.end.map(|end| {
            use chrono::TimeZone;
            let local = tz.from_utc_datetime(&end.naive_utc());
            local.format("%H:%M").to_string()
        })
    }

    /// Format the start date/time for display.
    pub fn start_datetime_str(&self, tz: &chrono_tz::Tz) -> String {
        use chrono::TimeZone;
        let local = tz.from_utc_datetime(&self.start.naive_utc());
        local.format("%A, %B %-d, %Y %H:%M").to_string()
    }

    /// Format the end date/time for display.
    pub fn end_datetime_str(&self, tz: &chrono_tz::Tz) -> Option<String> {
        self.end.map(|end| {
            use chrono::TimeZone;
            let local = tz.from_utc_datetime(&end.naive_utc());
            local.format("%A, %B %-d, %Y %H:%M").to_string()
        })
    }
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start
            .cmp(&other.start)
            .then_with(|| self.summary.cmp(&other.summary))
    }
}

/// Status of a background CalDAV fetch.
#[derive(Clone, Debug)]
pub enum FetchStatus {
    Loading { message: String },
    /// `stale_since` is `None` when the data came from a fresh network fetch,
    /// or `Some(t)` when we fell back to the on-disk cache last successfully
    /// written at `t` (the UI shows a "stale since t" banner in that case).
    ///
    /// `pending_oauth` lists Google sources that still need device-auth. The
    /// UI surfaces these via OAuthScene even when other sources succeeded —
    /// otherwise a working ICS source masks Google sources that need authorization.
    Done {
        calendars: Vec<CalendarInfo>,
        events: Vec<Event>,
        stale_since: Option<DateTime<Utc>>,
        pending_oauth: Vec<String>,
    },
    Error { message: String },
    /// One or more Google sources need OAuth device authorization, and no
    /// other source returned data. Kept for the "nothing to show at all" path.
    NeedsOAuth { server_names: Vec<String> },
}
