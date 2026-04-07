use super::parser;
use super::types::{CalendarInfo, Event, FetchStatus};
use super::ical;
use crate::config::Config;
use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate, Utc};

/// Fetch all calendars and events from configured CalDAV servers.
pub fn fetch_all(config: &Config) -> FetchStatus {
    let mut all_calendars = Vec::new();
    let mut all_events = Vec::new();
    let mut errors = Vec::new();

    for (server_name, server_config) in &config.sources {
        log::info!("Fetching from server: {}", server_name);

        match fetch_server(server_name, &server_config.url, &server_config.user, &server_config.password) {
            Ok((cals, evts)) => {
                all_calendars.extend(cals);
                all_events.extend(evts);
            }
            Err(e) => {
                log::error!("Failed to fetch from {}: {:?}", server_name, e);
                errors.push(format!("{}: {}", server_name, e));
            }
        }
    }

    if all_calendars.is_empty() && !errors.is_empty() {
        FetchStatus::Error {
            message: errors.join("\n"),
        }
    } else {
        all_events.sort();
        FetchStatus::Done {
            calendars: all_calendars,
            events: all_events,
        }
    }
}

fn fetch_server(
    server_name: &str,
    url: &str,
    username: &str,
    password: &str,
) -> Result<(Vec<CalendarInfo>, Vec<Event>)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")?;

    // Step 1: Discover the principal URL
    let principal_url = discover_principal(&client, url, username, password)?;

    // Step 2: Discover calendar home
    let calendar_home = discover_calendar_home(&client, &principal_url, username, password)?;

    // Step 3: List calendars
    let propfind_xml = build_calendar_propfind();
    let resp = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), &calendar_home)
        .basic_auth(username, Some(password))
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(propfind_xml)
        .send()
        .context("PROPFIND for calendars failed")?;

    let body = resp.text().context("Failed to read PROPFIND response")?;
    let parsed = parser::parse_propfind_calendars(&body)?;

    let mut calendars = Vec::new();
    let mut all_events = Vec::new();

    for cal in &parsed {
        if !cal.is_calendar {
            continue;
        }
        let cal_name = cal
            .display_name
            .clone()
            .unwrap_or_else(|| cal.href.clone());

        let cal_url = resolve_url(&calendar_home, &cal.href);

        calendars.push(CalendarInfo {
            name: cal_name.clone(),
            path: cal.href.clone(),
            color: cal.color.clone(),
            visible: true,
            server_name: server_name.to_string(),
        });

        // Step 4: Fetch events for this calendar
        match fetch_calendar_events(&client, &cal_url, username, password, &cal_name) {
            Ok(events) => all_events.extend(events),
            Err(e) => log::warn!("Failed to fetch events from {}: {:?}", cal_name, e),
        }
    }

    Ok((calendars, all_events))
}

fn discover_principal(
    client: &reqwest::blocking::Client,
    url: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:current-user-principal/>
  </d:prop>
</d:propfind>"#;

    let resp = client
        .request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url)
        .basic_auth(username, Some(password))
        .header("Depth", "0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(xml)
        .send()
        .context("PROPFIND for principal failed")?;

    let body = resp.text()?;

    // Extract principal href from response
    if let Some(href) = extract_href_from_tag(&body, "current-user-principal") {
        Ok(resolve_url(url, &href))
    } else {
        // Fall back to using the provided URL as-is
        Ok(url.to_string())
    }
}

fn discover_calendar_home(
    client: &reqwest::blocking::Client,
    principal_url: &str,
    username: &str,
    password: &str,
) -> Result<String> {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-home-set/>
  </d:prop>
</d:propfind>"#;

    let resp = client
        .request(
            reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
            principal_url,
        )
        .basic_auth(username, Some(password))
        .header("Depth", "0")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(xml)
        .send()
        .context("PROPFIND for calendar-home-set failed")?;

    let body = resp.text()?;

    if let Some(href) = extract_href_from_tag(&body, "calendar-home-set") {
        Ok(resolve_url(principal_url, &href))
    } else {
        // Fall back to principal URL
        Ok(principal_url.to_string())
    }
}

fn fetch_calendar_events(
    client: &reqwest::blocking::Client,
    calendar_url: &str,
    username: &str,
    password: &str,
    calendar_name: &str,
) -> Result<Vec<Event>> {
    let now = Utc::now().date_naive();
    let start = now - Duration::days(7);
    let end = now + Duration::days(30);

    let report_xml = build_calendar_report(start, end);

    let resp = client
        .request(
            reqwest::Method::from_bytes(b"REPORT").unwrap(),
            calendar_url,
        )
        .basic_auth(username, Some(password))
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(report_xml)
        .send()
        .context("REPORT for calendar events failed")?;

    let body = resp.text().context("Failed to read REPORT response")?;

    let parsed = parser::parse_report_events(&body)?;

    let mut events = Vec::new();
    for item in &parsed {
        let mut parsed_events = ical::parse_ical_events(&item.ical_data, calendar_name);
        events.append(&mut parsed_events);
    }

    Ok(events)
}

fn build_calendar_propfind() -> String {
    r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:apple="http://apple.com/ns/ical/">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <apple:calendar-color/>
    <cs:getctag/>
  </d:prop>
</d:propfind>"#
        .to_string()
}

fn build_calendar_report(start: NaiveDate, end: NaiveDate) -> String {
    let start_str = start.format("%Y%m%dT000000Z");
    let end_str = end.format("%Y%m%dT235959Z");

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{}" end="{}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#,
        start_str, end_str
    )
}

/// Resolve a potentially relative URL against a base URL.
fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }

    // Extract scheme + host from base
    if let Some(scheme_end) = base.find("://") {
        let after_scheme = &base[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            let origin = &base[..scheme_end + 3 + path_start];
            if href.starts_with('/') {
                return format!("{}{}", origin, href);
            }
        }
    }

    // Fall back: just append
    let base_trimmed = base.trim_end_matches('/');
    let href_trimmed = href.trim_start_matches('/');
    format!("{}/{}", base_trimmed, href_trimmed)
}

/// Simple extraction of href from within a specific XML tag.
fn extract_href_from_tag(xml: &str, tag: &str) -> Option<String> {
    // Find the tag (with any namespace prefix)
    let tag_pattern = format!(":{}", tag);
    let tag_pattern2 = format!("<{}", tag);

    let tag_start = xml.find(&tag_pattern).or_else(|| xml.find(&tag_pattern2))?;

    // Find the next <href> or <d:href> after this tag
    let rest = &xml[tag_start..];
    let href_start = rest.find(":href>").or_else(|| rest.find("<href>"))?;
    let content_start = rest[href_start..].find('>')? + href_start + 1;
    let content_end = rest[content_start..].find('<')? + content_start;

    Some(rest[content_start..content_end].trim().to_string())
}
