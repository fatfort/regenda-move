use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

/// Parsed calendar from PROPFIND response.
#[derive(Clone, Debug)]
pub struct PropfindCalendar {
    pub href: String,
    pub display_name: Option<String>,
    pub color: Option<String>,
    pub is_calendar: bool,
}

/// Parsed event data from REPORT response.
#[derive(Clone, Debug)]
pub struct ReportEvent {
    pub href: String,
    pub ical_data: String,
}

/// Parse a PROPFIND multistatus response to discover calendars.
pub fn parse_propfind_calendars(xml: &str) -> Result<Vec<PropfindCalendar>> {
    let mut reader = Reader::from_str(xml);
    let mut calendars = Vec::new();

    let mut in_response = false;
    let mut in_href = false;
    let mut in_displayname = false;
    let mut in_calendar_color = false;
    let mut in_resourcetype = false;
    let mut current_href = String::new();
    let mut current_name = None;
    let mut current_color = None;
    let mut is_calendar = false;

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                match local {
                    "response" => {
                        in_response = true;
                        current_href.clear();
                        current_name = None;
                        current_color = None;
                        is_calendar = false;
                    }
                    "href" => in_href = true,
                    "displayname" => in_displayname = true,
                    "calendar-color" => in_calendar_color = true,
                    "resourcetype" => in_resourcetype = true,
                    "calendar" if in_resourcetype => is_calendar = true,
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                match local {
                    "response" => {
                        if in_response {
                            calendars.push(PropfindCalendar {
                                href: current_href.clone(),
                                display_name: current_name.clone(),
                                color: current_color.clone(),
                                is_calendar,
                            });
                            in_response = false;
                        }
                    }
                    "href" => in_href = false,
                    "displayname" => in_displayname = false,
                    "calendar-color" => in_calendar_color = false,
                    "resourcetype" => in_resourcetype = false,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.unescape() {
                    let text = text.to_string();
                    if in_href && in_response {
                        current_href = text;
                    } else if in_displayname {
                        current_name = Some(text);
                    } else if in_calendar_color {
                        current_color = Some(text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                log::warn!("XML parse error: {:?}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(calendars)
}

/// Parse a REPORT multistatus response to extract calendar event data.
pub fn parse_report_events(xml: &str) -> Result<Vec<ReportEvent>> {
    let mut reader = Reader::from_str(xml);
    let mut events = Vec::new();

    let mut in_response = false;
    let mut in_href = false;
    let mut in_calendar_data = false;
    let mut current_href = String::new();
    let mut current_data = String::new();

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                match local {
                    "response" => {
                        in_response = true;
                        current_href.clear();
                        current_data.clear();
                    }
                    "href" => in_href = true,
                    "calendar-data" => in_calendar_data = true,
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref();
                let local = local_name(name_bytes);
                match local {
                    "response" => {
                        if in_response && !current_data.is_empty() {
                            events.push(ReportEvent {
                                href: current_href.clone(),
                                ical_data: current_data.clone(),
                            });
                        }
                        in_response = false;
                    }
                    "href" => in_href = false,
                    "calendar-data" => in_calendar_data = false,
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.unescape() {
                    let text = text.to_string();
                    if in_href && in_response {
                        current_href = text;
                    } else if in_calendar_data {
                        current_data.push_str(&text);
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                if let Ok(text) = std::str::from_utf8(e.as_ref()) {
                    if in_calendar_data {
                        current_data.push_str(text);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                log::warn!("XML parse error in report: {:?}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(events)
}

/// Extract local name from a potentially namespaced XML element name.
fn local_name(name: &[u8]) -> &str {
    let s = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = s.rfind(':') {
        &s[pos + 1..]
    } else {
        s
    }
}
