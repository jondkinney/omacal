//! A deliberately small ICS reader and writer.
//!
//! Small because omacal already speaks the hard half of RFC 5545: recurrence
//! lines (`RRULE`/`EXDATE`/`RDATE`) pass through **verbatim** to
//! `omacal-core`'s expander, which builds its own ICS internally. What this
//! module owns is the container: unfolding, components, parameters, the four
//! date-time shapes, text escaping, VALARM triggers — and, on the write side,
//! serializing new components and patching a VTODO's status inside an
//! otherwise-untouched resource. Round-tripping bytes we do not model is the
//! design constraint throughout: a CalDAV write replaces a whole resource,
//! so anything dropped in parsing would be deleted for real on the server.

use jiff::civil::{Date, DateTime};
use jiff::Timestamp;

/// One content line, unfolded: `NAME;PARAM=V;PARAM2=V2:value`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Uppercased.
    pub name: String,
    /// Parameter names uppercased; values as written (quotes stripped).
    pub params: Vec<(String, String)>,
    /// Raw value — unescape with [`unescape`] where the property is TEXT.
    pub value: String,
}

impl Property {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// One `BEGIN:`…`END:` block.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// Uppercased: `VCALENDAR`, `VEVENT`, `VTODO`, `VALARM`, …
    pub name: String,
    pub props: Vec<Property>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }
    pub fn prop_value(&self, name: &str) -> Option<&str> {
        self.prop(name).map(|p| p.value.as_str())
    }
    pub fn components<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Component> {
        self.children.iter().filter(move |c| c.name.eq_ignore_ascii_case(name))
    }
}

/// RFC 5545 line unfolding: a CRLF (or bare LF — real servers emit both)
/// followed by one space or tab continues the previous line.
fn unfold(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in src.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

/// Splits `NAME;PARAMS:value`, honouring quoted parameter values (a TZID may
/// legally contain a colon inside quotes).
fn parse_line(line: &str) -> Option<Property> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut segs = head.split(';');
    let name = segs.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let params = segs
        .filter_map(|s| {
            let (n, v) = s.split_once('=')?;
            Some((n.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        })
        .collect();
    Some(Property { name, params, value: value.to_string() })
}

/// Parses a whole ICS document into its root component tree. Tolerant by
/// intent: unknown properties are kept, stray lines outside any component are
/// dropped, and an unterminated component closes at end of input — a broken
/// resource should yield what it can, not abort a sync.
pub fn parse(src: &str) -> Option<Component> {
    let mut stack: Vec<Component> = Vec::new();
    let mut root: Option<Component> = None;
    for line in unfold(src) {
        let prop = parse_line(&line)?;
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component {
                name: prop.value.trim().to_ascii_uppercase(),
                props: Vec::new(),
                children: Vec::new(),
            }),
            "END" => {
                let done = stack.pop()?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => root = Some(done),
                }
            }
            _ => {
                if let Some(top) = stack.last_mut() {
                    top.props.push(prop);
                }
            }
        }
    }
    // Unterminated nesting: fold whatever is open into the outermost block.
    while let Some(done) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(done),
            None => root = Some(done),
        }
    }
    root
}

/// RFC 5545 TEXT unescaping.
pub fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// RFC 5545 TEXT escaping, for serialization.
pub fn escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// The four shapes an ICS date-time property takes.
#[derive(Debug, Clone, PartialEq)]
pub enum IcsTime {
    /// `VALUE=DATE` — an all-day date, no zone of its own.
    Date(Date),
    /// `…Z` — an instant.
    Utc(Timestamp),
    /// `TZID=…` — a wall time in a named zone.
    Zoned { dt: DateTime, tzid: String },
    /// Neither `Z` nor `TZID` — a floating wall time, resolved against the
    /// calendar's zone at conversion.
    Floating(DateTime),
}

fn parse_basic_date(v: &str) -> Option<Date> {
    if v.len() != 8 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Date::new(v[0..4].parse().ok()?, v[4..6].parse().ok()?, v[6..8].parse().ok()?).ok()
}

fn parse_basic_datetime(v: &str) -> Option<DateTime> {
    // 20260815T093000
    let (d, t) = v.split_once('T')?;
    let date = parse_basic_date(d)?;
    if t.len() < 6 || !t.as_bytes()[..6].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let time = jiff::civil::Time::new(
        t[0..2].parse().ok()?,
        t[2..4].parse().ok()?,
        t[4..6].parse().ok()?,
        0,
    )
    .ok()?;
    Some(date.to_datetime(time))
}

/// Reads a DTSTART/DTEND/DUE/COMPLETED/RECURRENCE-ID property into its shape.
pub fn parse_time(p: &Property) -> Option<IcsTime> {
    let v = p.value.trim();
    if p.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || (v.len() == 8 && !v.contains('T')) {
        return parse_basic_date(v).map(IcsTime::Date);
    }
    if let Some(stripped) = v.strip_suffix('Z') {
        let dt = parse_basic_datetime(stripped)?;
        return dt.to_zoned(jiff::tz::TimeZone::UTC).ok().map(|z| IcsTime::Utc(z.timestamp()));
    }
    let dt = parse_basic_datetime(v)?;
    match p.param("TZID") {
        Some(tzid) => Some(IcsTime::Zoned { dt, tzid: tzid.to_string() }),
        None => Some(IcsTime::Floating(dt)),
    }
}

/// Resolves a shape to `(epoch_ms, tz_name, is_all_day)` against the
/// calendar's zone. A TZID jiff does not know (a Windows zone name, a private
/// Apple alias) falls back to the calendar's zone rather than dropping the
/// event — a meeting an hour off beats a meeting missing.
pub fn resolve(t: &IcsTime, cal_tz: &str) -> Option<(i64, String, bool)> {
    match t {
        IcsTime::Date(d) => {
            let z = d.to_datetime(jiff::civil::Time::midnight()).in_tz(cal_tz).ok()?;
            Some((z.timestamp().as_millisecond(), cal_tz.to_string(), true))
        }
        IcsTime::Utc(ts) => Some((ts.as_millisecond(), "UTC".to_string(), false)),
        IcsTime::Zoned { dt, tzid } => match dt.in_tz(tzid) {
            Ok(z) => Some((z.timestamp().as_millisecond(), tzid.clone(), false)),
            Err(_) => {
                tracing::warn!(%tzid, "unknown TZID; resolving in the calendar's zone");
                let z = dt.in_tz(cal_tz).ok()?;
                Some((z.timestamp().as_millisecond(), cal_tz.to_string(), false))
            }
        },
        IcsTime::Floating(dt) => {
            let z = dt.in_tz(cal_tz).ok()?;
            Some((z.timestamp().as_millisecond(), cal_tz.to_string(), false))
        }
    }
}

/// An ISO 8601 duration (`P1D`, `PT1H30M`, `-PT15M`) in milliseconds.
pub fn parse_duration_ms(v: &str) -> Option<i64> {
    let (neg, rest) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let rest = rest.strip_prefix('P')?;
    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut ms: i64 = 0;
    let mut num = String::new();
    for c in date_part.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            ms += n * match c {
                'W' => 7 * 86_400_000,
                'D' => 86_400_000,
                _ => return None,
            };
        }
    }
    for c in time_part.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            ms += n * match c {
                'H' => 3_600_000,
                'M' => 60_000,
                'S' => 1_000,
                _ => return None,
            };
        }
    }
    if !num.is_empty() {
        return None;
    }
    Some(if neg { -ms } else { ms })
}

/// One VEVENT, lifted out of a resource. Times stay in their ICS shapes;
/// the store conversion resolves them against the calendar zone.
#[derive(Debug, Clone, PartialEq)]
pub struct CalEvent {
    pub uid: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    /// Lowercased; `confirmed` when absent, as Google's mapping assumes.
    pub status: String,
    pub start: IcsTime,
    /// Exactly one of `end`/`duration_ms` is normally present; neither means
    /// the RFC's defaults (instant event, or one day for all-day).
    pub end: Option<IcsTime>,
    pub duration_ms: Option<i64>,
    /// `RRULE:`/`EXDATE…`/`RDATE…` lines verbatim — omacal-core's expander
    /// takes exactly these.
    pub recurrence: Vec<String>,
    /// Set on an exception component overriding one occurrence.
    pub recurrence_id: Option<IcsTime>,
    pub sequence: i64,
    /// Minutes-before, from relative VALARM TRIGGERs. Display/audio → popup;
    /// email → email.
    pub alarms: Vec<(String, i64)>,
    pub organizer_email: Option<String>,
    pub conference_uri: Option<String>,
    pub last_modified_ms: Option<i64>,
    pub attendees: Vec<CalAttendee>,
}

/// One invitee off an `ATTENDEE` line.
#[derive(Debug, Clone, PartialEq)]
pub struct CalAttendee {
    pub email: String,
    pub display_name: Option<String>,
    /// Google's spelling of PARTSTAT: `accepted` | `declined` | `tentative`
    /// | `needsAction` — normalised here so the store sees one vocabulary.
    pub response_status: String,
    pub optional: bool,
}

fn read_attendee(p: &Property) -> Option<CalAttendee> {
    let v = p.value.trim();
    let email = v.strip_prefix("mailto:").unwrap_or(v).to_string();
    if email.is_empty() {
        return None;
    }
    let response_status = match p
        .param("PARTSTAT")
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("ACCEPTED") => "accepted",
        Some("DECLINED") => "declined",
        Some("TENTATIVE") => "tentative",
        _ => "needsAction",
    }
    .to_string();
    Some(CalAttendee {
        email,
        display_name: p.param("CN").map(str::to_string),
        response_status,
        optional: p.param("ROLE").is_some_and(|r| r.eq_ignore_ascii_case("OPT-PARTICIPANT")),
    })
}

/// One VTODO.
#[derive(Debug, Clone, PartialEq)]
pub struct CalTodo {
    pub uid: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub due: Option<IcsTime>,
    /// Lowercased; `needs-action` when absent.
    pub status: String,
    pub completed: Option<IcsTime>,
    pub priority: i64,
}

fn text(c: &Component, name: &str) -> Option<String> {
    c.prop_value(name).map(unescape).filter(|s| !s.trim().is_empty())
}

fn alarm_minutes(alarm: &Component) -> Option<(String, i64)> {
    let trigger = alarm.prop("TRIGGER")?;
    // Absolute triggers (VALUE=DATE-TIME) do not map to minutes-before.
    if trigger.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
        return None;
    }
    // RELATED=END alarms are rare and mean something else; skip rather than lie.
    if trigger.param("RELATED").is_some_and(|v| v.eq_ignore_ascii_case("END")) {
        return None;
    }
    let ms = parse_duration_ms(trigger.value.trim())?;
    let minutes = (-ms) / 60_000;
    if minutes < 0 {
        return None; // an alarm after the start is not a reminder
    }
    let method = match alarm.prop_value("ACTION").map(str::to_ascii_uppercase).as_deref() {
        Some("EMAIL") => "email",
        _ => "popup",
    };
    Some((method.to_string(), minutes))
}

fn read_event(c: &Component) -> Option<CalEvent> {
    let uid = c.prop_value("UID")?.trim().to_string();
    let start = parse_time(c.prop("DTSTART")?)?;
    let recurrence = c
        .props
        .iter()
        .filter(|p| matches!(p.name.as_str(), "RRULE" | "EXDATE" | "RDATE"))
        .map(raw_line)
        .collect();
    Some(CalEvent {
        uid,
        summary: text(c, "SUMMARY"),
        description: text(c, "DESCRIPTION"),
        location: text(c, "LOCATION"),
        status: c
            .prop_value("STATUS")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "confirmed".into()),
        end: c.prop("DTEND").and_then(parse_time),
        duration_ms: c.prop_value("DURATION").and_then(parse_duration_ms),
        recurrence,
        recurrence_id: c.prop("RECURRENCE-ID").and_then(parse_time),
        sequence: c.prop_value("SEQUENCE").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
        alarms: c.components("VALARM").filter_map(alarm_minutes).collect(),
        organizer_email: c.prop_value("ORGANIZER").map(|v| {
            v.trim().strip_prefix("mailto:").unwrap_or(v.trim()).to_string()
        }),
        conference_uri: c
            .prop_value("URL")
            .map(str::trim)
            .filter(|v| v.starts_with("http"))
            .map(str::to_string),
        last_modified_ms: c
            .prop("LAST-MODIFIED")
            .and_then(parse_time)
            .and_then(|t| resolve(&t, "UTC"))
            .map(|(ms, _, _)| ms),
        attendees: c
            .props
            .iter()
            .filter(|p| p.name == "ATTENDEE")
            .filter_map(read_attendee)
            .collect(),
        start,
    })
}

/// Re-renders a parsed property as the ICS line the expander expects.
fn raw_line(p: &Property) -> String {
    let mut s = p.name.clone();
    for (n, v) in &p.params {
        s.push(';');
        s.push_str(n);
        s.push('=');
        s.push_str(v);
    }
    s.push(':');
    s.push_str(&p.value);
    s
}

fn read_todo(c: &Component) -> Option<CalTodo> {
    Some(CalTodo {
        uid: c.prop_value("UID")?.trim().to_string(),
        summary: text(c, "SUMMARY"),
        description: text(c, "DESCRIPTION"),
        due: c.prop("DUE").and_then(parse_time),
        status: c
            .prop_value("STATUS")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "needs-action".into()),
        completed: c.prop("COMPLETED").and_then(parse_time),
        priority: c.prop_value("PRIORITY").and_then(|v| v.trim().parse().ok()).unwrap_or(0),
    })
}

/// Every VEVENT in a resource — the master first when both master and
/// exceptions are present, which is the order the store conversion wants.
pub fn events_in(root: &Component) -> Vec<CalEvent> {
    let mut out: Vec<CalEvent> = root.components("VEVENT").filter_map(read_event).collect();
    out.sort_by_key(|e| e.recurrence_id.is_some());
    out
}

/// Every VTODO in a resource.
pub fn todos_in(root: &Component) -> Vec<CalTodo> {
    root.components("VTODO").filter_map(read_todo).collect()
}

fn fmt_utc(ts: Timestamp) -> String {
    let z = ts.to_zoned(jiff::tz::TimeZone::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        z.year(),
        z.month(),
        z.day(),
        z.hour(),
        z.minute(),
        z.second()
    )
}

/// Rewrites the VTODO with `uid` inside `raw` to completed / reopened,
/// touching nothing else in the resource. Physical-line surgery on purpose:
/// re-serializing the parse tree would normalize every line and lose
/// vendor extensions this module does not model.
pub fn patch_todo_status(raw: &str, uid: &str, completed: bool, now: Timestamp) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut block: Vec<String> = Vec::new();
    let mut in_todo = false;
    for raw_line in raw.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.eq_ignore_ascii_case("BEGIN:VTODO") {
            in_todo = true;
            block.clear();
            block.push(line.to_string());
            continue;
        }
        if !in_todo {
            out.push(line.to_string());
            continue;
        }
        if line.eq_ignore_ascii_case("END:VTODO") {
            block.push(line.to_string());
            // Only the matching UID gets patched; other VTODOs pass through.
            let joined = block.join("\n");
            let this_uid = parse(&format!("BEGIN:VCALENDAR\n{joined}\nEND:VCALENDAR"))
                .and_then(|r| r.components("VTODO").next().and_then(|t| t.prop_value("UID").map(|u| u.trim().to_string())));
            if this_uid.as_deref() == Some(uid) {
                let mut patched: Vec<String> = block
                    .iter()
                    .filter(|l| {
                        let u = l.to_ascii_uppercase();
                        !(u.starts_with("STATUS")
                            || u.starts_with("COMPLETED:")
                            || u.starts_with("COMPLETED;")
                            || u.starts_with("PERCENT-COMPLETE"))
                    })
                    .cloned()
                    .collect();
                let insert_at = patched.len() - 1; // before END:VTODO
                if completed {
                    patched.splice(
                        insert_at..insert_at,
                        [
                            "STATUS:COMPLETED".to_string(),
                            format!("COMPLETED:{}", fmt_utc(now)),
                            "PERCENT-COMPLETE:100".to_string(),
                        ],
                    );
                } else {
                    patched.splice(insert_at..insert_at, ["STATUS:NEEDS-ACTION".to_string()]);
                }
                out.extend(patched);
            } else {
                out.extend(block.iter().cloned());
            }
            in_todo = false;
            block.clear();
            continue;
        }
        block.push(line.to_string());
    }
    if out.is_empty() {
        return None;
    }
    Some(out.join("\r\n"))
}

/// A brand-new single-VTODO resource.
pub fn new_todo_ics(uid: &str, summary: &str, due: Option<(&IcsTime, &str)>, now: Timestamp) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//omacal//EN".to_string(),
        "BEGIN:VTODO".to_string(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", fmt_utc(now)),
        format!("SUMMARY:{}", escape(summary)),
        "STATUS:NEEDS-ACTION".to_string(),
    ];
    if let Some((due, _cal_tz)) = due {
        match due {
            IcsTime::Date(d) => lines.push(format!(
                "DUE;VALUE=DATE:{:04}{:02}{:02}",
                d.year(),
                d.month(),
                d.day()
            )),
            IcsTime::Utc(ts) => lines.push(format!("DUE:{}", fmt_utc(*ts))),
            IcsTime::Zoned { dt, tzid } => lines.push(format!(
                "DUE;TZID={tzid}:{:04}{:02}{:02}T{:02}{:02}{:02}",
                dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second()
            )),
            IcsTime::Floating(dt) => lines.push(format!(
                "DUE:{:04}{:02}{:02}T{:02}{:02}{:02}",
                dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second()
            )),
        }
    }
    lines.push("END:VTODO".to_string());
    lines.push("END:VCALENDAR".to_string());
    lines.join("\r\n")
}

// --- event write-side -------------------------------------------------------

/// An endpoint the serializer can render: a bare date (all-day) or a wall
/// time in a named zone. UTC instants are rendered through a zone by the
/// caller — authored events keep their author's zone, like every client.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteTime {
    Date(Date),
    Zoned { dt: DateTime, tzid: String },
}

impl WriteTime {
    fn prop(&self, name: &str) -> String {
        match self {
            WriteTime::Date(d) => {
                format!("{name};VALUE=DATE:{:04}{:02}{:02}", d.year(), d.month(), d.day())
            }
            WriteTime::Zoned { dt, tzid } => format!(
                "{name};TZID={tzid}:{:04}{:02}{:02}T{:02}{:02}{:02}",
                dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second()
            ),
        }
    }
}

/// Everything a written VEVENT carries. One shape serves creates, full
/// rewrites, and exception components — presence of `recurrence_id` is what
/// makes it an exception.
#[derive(Debug, Clone, PartialEq)]
pub struct EventWrite {
    pub uid: String,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub start: WriteTime,
    pub end: WriteTime,
    /// Recurrence lines verbatim (`RRULE:...`); empty for a one-off.
    pub recurrence: Vec<String>,
    pub recurrence_id: Option<WriteTime>,
    /// `(method, minutes)` — `popup` renders DISPLAY, `email` EMAIL.
    pub alarms: Vec<(String, i64)>,
    pub sequence: i64,
}

fn vevent_lines(ev: &EventWrite, now: Timestamp) -> Vec<String> {
    let mut lines = vec![
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", ev.uid),
        format!("DTSTAMP:{}", fmt_utc(now)),
    ];
    if let Some(rid) = &ev.recurrence_id {
        lines.push(rid.prop("RECURRENCE-ID"));
    }
    lines.push(ev.start.prop("DTSTART"));
    lines.push(ev.end.prop("DTEND"));
    if let Some(s) = &ev.summary {
        lines.push(format!("SUMMARY:{}", escape(s)));
    }
    if let Some(l) = &ev.location {
        lines.push(format!("LOCATION:{}", escape(l)));
    }
    if let Some(d) = &ev.description {
        lines.push(format!("DESCRIPTION:{}", escape(d)));
    }
    for r in &ev.recurrence {
        lines.push(r.clone());
    }
    if ev.sequence > 0 {
        lines.push(format!("SEQUENCE:{}", ev.sequence));
    }
    for (method, minutes) in &ev.alarms {
        lines.push("BEGIN:VALARM".to_string());
        lines.push(format!(
            "ACTION:{}",
            if method == "email" { "EMAIL" } else { "DISPLAY" }
        ));
        lines.push("DESCRIPTION:Reminder".to_string());
        lines.push(format!("TRIGGER:-PT{minutes}M"));
        lines.push("END:VALARM".to_string());
    }
    lines.push("END:VEVENT".to_string());
    lines
}

/// A brand-new single-event resource.
pub fn new_event_ics(ev: &EventWrite, now: Timestamp) -> String {
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_string(),
        "VERSION:2.0".to_string(),
        "PRODID:-//omacal//EN".to_string(),
    ];
    lines.extend(vevent_lines(ev, now));
    lines.push("END:VCALENDAR".to_string());
    lines.join("\r\n")
}

/// The physical lines of one VEVENT block, with its position, plus whether it
/// is an exception (carries RECURRENCE-ID) — the unit every resource edit
/// below works in.
struct EventBlock {
    start: usize,
    end: usize, // inclusive, the END:VEVENT line
    is_exception: bool,
    uid: Option<String>,
}

fn split_lines(raw: &str) -> Vec<String> {
    raw.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect()
}

fn event_blocks(lines: &[String]) -> Vec<EventBlock> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            start = Some(i);
        } else if line.eq_ignore_ascii_case("END:VEVENT") {
            if let Some(s) = start.take() {
                let body = lines[s..=i].join("\n");
                let parsed = parse(&format!("BEGIN:VCALENDAR\n{body}\nEND:VCALENDAR"));
                let (uid, is_exception) = parsed
                    .as_ref()
                    .and_then(|r| r.components("VEVENT").next())
                    .map(|c| {
                        (
                            c.prop_value("UID").map(|u| u.trim().to_string()),
                            c.prop("RECURRENCE-ID").is_some(),
                        )
                    })
                    .unwrap_or((None, false));
                out.push(EventBlock { start: s, end: i, is_exception, uid });
            }
        }
    }
    out
}

/// Rewrites the *master* VEVENT of `uid` inside `raw` with `ev`'s fields,
/// preserving every property this module does not model. When
/// `drop_exceptions` is set (a series edit that moved its times), exception
/// blocks of that uid are removed too — their RECURRENCE-IDs name occurrence
/// starts that no longer exist.
pub fn rewrite_master(
    raw: &str,
    uid: &str,
    ev: &EventWrite,
    now: Timestamp,
    drop_exceptions: bool,
) -> Option<String> {
    let lines = split_lines(raw);
    let blocks = event_blocks(&lines);
    let master = blocks
        .iter()
        .find(|b| !b.is_exception && b.uid.as_deref() == Some(uid))?;

    // The master block, with the properties the edit owns removed and the
    // fresh ones inserted. Unknown lines (X-…, ORGANIZER, ATTENDEE, CLASS…)
    // pass through untouched.
    let owned = |l: &str| {
        let u = l.to_ascii_uppercase();
        u.starts_with("SUMMARY") || u.starts_with("LOCATION") || u.starts_with("DESCRIPTION:")
            || u.starts_with("DESCRIPTION;") || u.starts_with("DTSTART") || u.starts_with("DTEND")
            || u.starts_with("DURATION") || u.starts_with("RRULE") || u.starts_with("RDATE")
            || u.starts_with("SEQUENCE") || u.starts_with("DTSTAMP")
    };
    // A time change invalidates EXDATEs too (they name occurrence starts).
    let drop_exdates = drop_exceptions;

    let mut patched: Vec<String> = Vec::new();
    let mut in_alarm = false;
    for l in &lines[master.start..=master.end] {
        let u = l.to_ascii_uppercase();
        if u == "BEGIN:VALARM" {
            in_alarm = true;
            continue;
        }
        if u == "END:VALARM" {
            in_alarm = false;
            continue;
        }
        if in_alarm || owned(l) || (drop_exdates && u.starts_with("EXDATE")) {
            continue;
        }
        if u == "END:VEVENT" {
            // Insert the owned properties just before the close.
            let fresh = vevent_lines(ev, now);
            // Skip BEGIN:VEVENT/UID from the fresh render — the block keeps
            // its own — and take everything else.
            for f in &fresh[2..fresh.len() - 1] {
                patched.push(f.clone());
            }
        }
        patched.push(l.clone());
    }

    let mut out: Vec<String> = Vec::new();
    let mut skip: Vec<(usize, usize)> = vec![(master.start, master.end)];
    if drop_exceptions {
        for b in blocks.iter().filter(|b| b.is_exception && b.uid.as_deref() == Some(uid)) {
            skip.push((b.start, b.end));
        }
    }
    for (i, l) in lines.iter().enumerate() {
        if skip.iter().any(|(s, e)| i >= *s && i <= *e) {
            if i == master.start {
                out.extend(patched.iter().cloned());
            }
            continue;
        }
        out.push(l.clone());
    }
    Some(out.join("\r\n"))
}

/// Inserts (or replaces) the exception component for one occurrence — scope
/// "this" on a series. An existing exception with the same RECURRENCE-ID
/// value is replaced; the master and its siblings stay byte-identical.
pub fn upsert_exception(raw: &str, ev: &EventWrite, now: Timestamp) -> Option<String> {
    let rid = ev.recurrence_id.as_ref()?;
    let rid_value = rid.prop("RECURRENCE-ID");
    let lines = split_lines(raw);
    let blocks = event_blocks(&lines);
    // An existing exception for the same occurrence: match on the rendered
    // RECURRENCE-ID value's date-time part, ignoring parameter order.
    let same_occurrence = |b: &EventBlock| {
        b.is_exception
            && b.uid.as_deref() == Some(ev.uid.as_str())
            && lines[b.start..=b.end].iter().any(|l| {
                l.to_ascii_uppercase().starts_with("RECURRENCE-ID")
                    && l.rsplit(':').next() == rid_value.rsplit(':').next()
            })
    };
    let replaced: Option<(usize, usize)> =
        blocks.iter().find(|b| same_occurrence(b)).map(|b| (b.start, b.end));

    let close = lines
        .iter()
        .rposition(|l| l.eq_ignore_ascii_case("END:VCALENDAR"))?;

    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some((s, e)) = replaced {
            if i >= s && i <= e {
                continue;
            }
        }
        if i == close {
            out.extend(vevent_lines(ev, now));
        }
        out.push(l.clone());
    }
    Some(out.join("\r\n"))
}

/// Removes one occurrence from a series: an EXDATE on the master (matching
/// DTSTART's form) plus the removal of any exception component that named
/// that occurrence.
pub fn exclude_occurrence(raw: &str, uid: &str, occurrence: &WriteTime) -> Option<String> {
    let lines = split_lines(raw);
    let blocks = event_blocks(&lines);
    let master = blocks
        .iter()
        .find(|b| !b.is_exception && b.uid.as_deref() == Some(uid))?;
    let exdate = occurrence.prop("EXDATE");
    let rid_tail = occurrence.prop("RECURRENCE-ID");
    let doomed: Vec<(usize, usize)> = blocks
        .iter()
        .filter(|b| {
            b.is_exception
                && b.uid.as_deref() == Some(uid)
                && lines[b.start..=b.end].iter().any(|l| {
                    l.to_ascii_uppercase().starts_with("RECURRENCE-ID")
                        && l.rsplit(':').next() == rid_tail.rsplit(':').next()
                })
        })
        .map(|b| (b.start, b.end))
        .collect();

    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if doomed.iter().any(|(s, e)| i >= *s && i <= *e) {
            continue;
        }
        if i == master.end {
            out.push(exdate.clone()); // just before the master's END:VEVENT
        }
        out.push(l.clone());
    }
    Some(out.join("\r\n"))
}

/// Ends a series before the `cut` occurrence — scope "following": the RRULE
/// gets `UNTIL` (replacing any COUNT), and exception components at or past
/// the cut die with the occurrences they overrode. `until_render` must match
/// DTSTART's value type — a bare date for all-day series, UTC basic time
/// otherwise — and `cut` must be rendered in the master's own zone, which is
/// what makes the exception comparison below chronological: two basic
/// date-times in the same zone order lexicographically.
pub fn truncate_series(raw: &str, uid: &str, until_render: &str, cut: &WriteTime) -> Option<String> {
    let lines = split_lines(raw);
    let blocks = event_blocks(&lines);
    let master = blocks
        .iter()
        .find(|b| !b.is_exception && b.uid.as_deref() == Some(uid))?;
    let rrule_idx = (master.start..=master.end)
        .find(|&i| lines[i].to_ascii_uppercase().starts_with("RRULE"))?;

    let rule = lines[rrule_idx].clone();
    // Drop COUNT and any existing UNTIL, then append ours.
    let (head, params) = rule.split_at(rule.find(':')? + 1);
    let kept: Vec<&str> = params
        .split(';')
        .filter(|p| {
            let u = p.trim().to_ascii_uppercase();
            !u.starts_with("COUNT=") && !u.starts_with("UNTIL=")
        })
        .collect();
    let rule = format!("{head}{};UNTIL={until_render}", kept.join(";"));

    let cut_tail = cut.prop("RECURRENCE-ID");
    let cut_tail = cut_tail.rsplit(':').next().unwrap_or("").to_string();
    let doomed: Vec<(usize, usize)> = blocks
        .iter()
        .filter(|b| {
            b.is_exception
                && b.uid.as_deref() == Some(uid)
                && lines[b.start..=b.end].iter().any(|l| {
                    l.to_ascii_uppercase().starts_with("RECURRENCE-ID")
                        && l.rsplit(':').next().unwrap_or("") >= cut_tail.as_str()
                })
        })
        .map(|b| (b.start, b.end))
        .collect();

    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if doomed.iter().any(|(s, e)| i >= *s && i <= *e) {
            continue;
        }
        if i == rrule_idx {
            out.push(rule.clone());
        } else {
            out.push(l.clone());
        }
    }
    Some(out.join("\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:abc-1\r\nSUMMARY:Standup\\, daily\r\nDTSTART;TZID=Europe/Sofia:20260817T093000\r\nDTEND;TZID=Europe/Sofia:20260817T094500\r\nSTATUS:CONFIRMED\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_simple_vevent_parses_completely() {
        let root = parse(SIMPLE).unwrap();
        let evs = events_in(&root);
        assert_eq!(evs.len(), 1);
        let e = &evs[0];
        assert_eq!(e.uid, "abc-1");
        assert_eq!(e.summary.as_deref(), Some("Standup, daily"), "escaped comma unescapes");
        assert_eq!(e.alarms, vec![("popup".to_string(), 15)]);
        let (start_ms, tz, all_day) = resolve(&e.start, "UTC").unwrap();
        assert_eq!(tz, "Europe/Sofia");
        assert!(!all_day);
        // 09:30 Sofia (UTC+3 in August) = 06:30Z.
        let z = Timestamp::from_millisecond(start_ms).unwrap().to_zoned(jiff::tz::TimeZone::UTC);
        assert_eq!((z.hour(), z.minute()), (6, 30));
    }

    #[test]
    fn folded_lines_unfold() {
        let src = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:f1\r\nSUMMARY:A very long tit\r\n le that was folded\r\nDTSTART:20260817T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR";
        let root = parse(src).unwrap();
        let evs = events_in(&root);
        assert_eq!(evs[0].summary.as_deref(), Some("A very long title that was folded"));
    }

    #[test]
    fn all_day_dates_resolve_in_the_calendar_zone() {
        let src = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:d1\nDTSTART;VALUE=DATE:20260820\nDTEND;VALUE=DATE:20260821\nEND:VEVENT\nEND:VCALENDAR";
        let root = parse(src).unwrap();
        let e = &events_in(&root)[0];
        let (ms, tz, all_day) = resolve(&e.start, "Europe/Sofia").unwrap();
        assert!(all_day);
        assert_eq!(tz, "Europe/Sofia");
        // Midnight Sofia on Aug 20 = Aug 19 21:00Z.
        let z = Timestamp::from_millisecond(ms).unwrap().to_zoned(jiff::tz::TimeZone::UTC);
        assert_eq!((z.day(), z.hour()), (19, 21));
    }

    #[test]
    fn recurrence_lines_pass_through_verbatim() {
        let src = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:r1\nDTSTART;TZID=Europe/Sofia:20260817T090000\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE\nEXDATE;TZID=Europe/Sofia:20260824T090000\nEND:VEVENT\nEND:VCALENDAR";
        let e = &events_in(&parse(src).unwrap())[0];
        assert_eq!(
            e.recurrence,
            vec![
                "RRULE:FREQ=WEEKLY;BYDAY=MO,WE".to_string(),
                "EXDATE;TZID=Europe/Sofia:20260824T090000".to_string(),
            ]
        );
    }

    #[test]
    fn a_master_sorts_before_its_exception() {
        let src = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:s1\nRECURRENCE-ID;TZID=Europe/Sofia:20260818T090000\nDTSTART;TZID=Europe/Sofia:20260818T100000\nEND:VEVENT\nBEGIN:VEVENT\nUID:s1\nDTSTART;TZID=Europe/Sofia:20260817T090000\nRRULE:FREQ=DAILY\nEND:VEVENT\nEND:VCALENDAR";
        let evs = events_in(&parse(src).unwrap());
        assert_eq!(evs.len(), 2);
        assert!(evs[0].recurrence_id.is_none(), "master first");
        assert!(evs[1].recurrence_id.is_some());
    }

    #[test]
    fn unknown_tzids_fall_back_to_the_calendar_zone() {
        let p = Property {
            name: "DTSTART".into(),
            params: vec![("TZID".into(), "Cupertino Standard Time".into())],
            value: "20260817T090000".into(),
        };
        let t = parse_time(&p).unwrap();
        let (_, tz, _) = resolve(&t, "Europe/Sofia").unwrap();
        assert_eq!(tz, "Europe/Sofia");
    }

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration_ms("PT1H30M"), Some(5_400_000));
        assert_eq!(parse_duration_ms("-PT15M"), Some(-900_000));
        assert_eq!(parse_duration_ms("P1D"), Some(86_400_000));
        assert_eq!(parse_duration_ms("P1W"), Some(604_800_000));
        assert_eq!(parse_duration_ms("nonsense"), None);
    }

    const TODO: &str = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:t-9\r\nSUMMARY:Buy stamps\r\nDUE;VALUE=DATE:20260820\r\nPRIORITY:1\r\nX-APPLE-SPECIAL:kept\r\nEND:VTODO\r\nEND:VCALENDAR";

    #[test]
    fn a_vtodo_parses() {
        let t = &todos_in(&parse(TODO).unwrap())[0];
        assert_eq!(t.uid, "t-9");
        assert_eq!(t.summary.as_deref(), Some("Buy stamps"));
        assert_eq!(t.status, "needs-action");
        assert_eq!(t.priority, 1);
        assert!(matches!(t.due, Some(IcsTime::Date(_))));
    }

    /// The whole point of line-surgery: complete a task and the vendor
    /// extension line survives untouched.
    #[test]
    fn completing_a_todo_preserves_what_we_do_not_model() {
        let now = Timestamp::from_millisecond(1_786_352_400_000).unwrap();
        let done = patch_todo_status(TODO, "t-9", true, now).unwrap();
        assert!(done.contains("X-APPLE-SPECIAL:kept"), "vendor line survives");
        assert!(done.contains("STATUS:COMPLETED"));
        assert!(done.contains("PERCENT-COMPLETE:100"));
        assert!(done.contains("COMPLETED:20260810T090000Z"));

        let back = patch_todo_status(&done, "t-9", false, now).unwrap();
        assert!(back.contains("STATUS:NEEDS-ACTION"));
        assert!(!back.contains("STATUS:COMPLETED"));
        assert!(!back.contains("PERCENT-COMPLETE"));
        assert!(back.contains("X-APPLE-SPECIAL:kept"));
    }

    #[test]
    fn patching_one_uid_leaves_its_siblings_alone() {
        let two = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:a\nSTATUS:NEEDS-ACTION\nEND:VTODO\nBEGIN:VTODO\nUID:b\nSTATUS:NEEDS-ACTION\nEND:VTODO\nEND:VCALENDAR";
        let now = Timestamp::from_millisecond(0).unwrap();
        let done = patch_todo_status(two, "b", true, now).unwrap();
        assert_eq!(done.matches("STATUS:NEEDS-ACTION").count(), 1, "a untouched");
        assert_eq!(done.matches("STATUS:COMPLETED").count(), 1, "b completed");
    }

    fn write_sample(uid: &str) -> EventWrite {
        EventWrite {
            uid: uid.into(),
            summary: Some("Planning".into()),
            location: Some("Room 2; annex".into()),
            description: None,
            start: WriteTime::Zoned {
                dt: jiff::civil::date(2026, 8, 20).at(10, 0, 0, 0),
                tzid: "Europe/Sofia".into(),
            },
            end: WriteTime::Zoned {
                dt: jiff::civil::date(2026, 8, 20).at(11, 0, 0, 0),
                tzid: "Europe/Sofia".into(),
            },
            recurrence: Vec::new(),
            recurrence_id: None,
            alarms: vec![("popup".into(), 10)],
            sequence: 0,
        }
    }

    const NOW: i64 = 1_786_352_400_000;

    #[test]
    fn a_new_event_serializes_and_reparses() {
        let now = Timestamp::from_millisecond(NOW).unwrap();
        let ics = new_event_ics(&write_sample("w1"), now);
        let evs = events_in(&parse(&ics).unwrap());
        assert_eq!(evs.len(), 1);
        let e = &evs[0];
        assert_eq!(e.uid, "w1");
        assert_eq!(e.location.as_deref(), Some("Room 2; annex"), "escaping round-trips");
        assert_eq!(e.alarms, vec![("popup".to_string(), 10)]);
        let (start_ms, tz, all_day) = resolve(&e.start, "UTC").unwrap();
        assert_eq!(tz, "Europe/Sofia");
        assert!(!all_day);
        let z = Timestamp::from_millisecond(start_ms).unwrap().to_zoned(jiff::tz::TimeZone::UTC);
        assert_eq!((z.hour(), z.minute()), (7, 0), "10:00 Sofia in August is 07:00Z");
    }

    const SERIES_RAW: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:kept\r\nBEGIN:VEVENT\r\nUID:s1\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Sofia:20260817T091500\r\nDTEND;TZID=Europe/Sofia:20260817T093000\r\nRRULE:FREQ=DAILY;COUNT=10\r\nX-PRIVATE:survives\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:s1\r\nRECURRENCE-ID;TZID=Europe/Sofia:20260819T091500\r\nSUMMARY:Standup (moved)\r\nDTSTART;TZID=Europe/Sofia:20260819T140000\r\nDTEND;TZID=Europe/Sofia:20260819T141500\r\nEND:VEVENT\r\nEND:VCALENDAR";

    #[test]
    fn rewriting_a_master_keeps_unknown_lines_and_its_exception() {
        let now = Timestamp::from_millisecond(NOW).unwrap();
        let mut ev = write_sample("s1");
        ev.summary = Some("Standup, renamed".into());
        ev.start = WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 17).at(9, 15, 0, 0),
            tzid: "Europe/Sofia".into(),
        };
        ev.end = WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 17).at(9, 30, 0, 0),
            tzid: "Europe/Sofia".into(),
        };
        ev.recurrence = vec!["RRULE:FREQ=DAILY;COUNT=10".into()];
        ev.sequence = 1;

        let out = rewrite_master(SERIES_RAW, "s1", &ev, now, false).unwrap();
        assert!(out.contains("X-PRIVATE:survives"), "unknown line survives");
        assert!(out.contains("X-WR-CALNAME:kept"), "calendar-level line survives");
        assert!(out.contains("SUMMARY:Standup\\, renamed"));
        assert!(out.contains("SUMMARY:Standup (moved)"), "the exception is untouched");
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 2);

        let evs = events_in(&parse(&out).unwrap());
        assert_eq!(evs[0].summary.as_deref(), Some("Standup, renamed"));
        assert_eq!(evs[0].sequence, 1);
    }

    #[test]
    fn a_time_change_on_the_series_drops_stale_exceptions() {
        let now = Timestamp::from_millisecond(NOW).unwrap();
        let mut ev = write_sample("s1");
        ev.recurrence = vec!["RRULE:FREQ=DAILY;COUNT=10".into()];
        let out = rewrite_master(SERIES_RAW, "s1", &ev, now, true).unwrap();
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 1, "the moved occurrence dies");
        assert!(!out.contains("RECURRENCE-ID"));
    }

    #[test]
    fn editing_one_occurrence_upserts_its_exception() {
        let now = Timestamp::from_millisecond(NOW).unwrap();
        let mut ev = write_sample("s1");
        ev.summary = Some("One-off change".into());
        ev.recurrence_id = Some(WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 18).at(9, 15, 0, 0),
            tzid: "Europe/Sofia".into(),
        });
        let out = upsert_exception(SERIES_RAW, &ev, now).unwrap();
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 3, "a new exception joined");
        assert!(out.contains("SUMMARY:One-off change"));
        assert!(out.contains("SUMMARY:Standup (moved)"), "the other exception stays");

        // Editing the SAME occurrence again replaces, not duplicates.
        ev.summary = Some("Changed twice".into());
        let out2 = upsert_exception(&out, &ev, now).unwrap();
        assert_eq!(out2.matches("BEGIN:VEVENT").count(), 3);
        assert!(out2.contains("SUMMARY:Changed twice"));
        assert!(!out2.contains("SUMMARY:One-off change"));
    }

    #[test]
    fn excluding_an_occurrence_exdates_it_and_reaps_its_exception() {
        let cut = WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 19).at(9, 15, 0, 0),
            tzid: "Europe/Sofia".into(),
        };
        let out = exclude_occurrence(SERIES_RAW, "s1", &cut).unwrap();
        assert!(out.contains("EXDATE;TZID=Europe/Sofia:20260819T091500"));
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 1, "its exception died with it");
        // The EXDATE landed inside the master block, before its END:VEVENT.
        let master_end = out.find("END:VEVENT").unwrap();
        assert!(out.find("EXDATE").unwrap() < master_end);
    }

    #[test]
    fn truncating_a_series_sets_until_and_reaps_later_exceptions() {
        let cut = WriteTime::Zoned {
            dt: jiff::civil::date(2026, 8, 19).at(9, 15, 0, 0),
            tzid: "Europe/Sofia".into(),
        };
        let out = truncate_series(SERIES_RAW, "s1", "20260819T061459Z", &cut).unwrap();
        assert!(out.contains("RRULE:FREQ=DAILY;UNTIL=20260819T061459Z"), "COUNT replaced");
        assert_eq!(out.matches("BEGIN:VEVENT").count(), 1, "the moved (later) occurrence died");
    }

    #[test]
    fn a_new_todo_serializes_and_reparses() {
        let now = Timestamp::from_millisecond(1_786_352_400_000).unwrap();
        let due = IcsTime::Date(Date::new(2026, 8, 20).unwrap());
        let ics = new_todo_ics("new-1", "Fix; the, thing", Some((&due, "Europe/Sofia")), now);
        let t = &todos_in(&parse(&ics).unwrap())[0];
        assert_eq!(t.uid, "new-1");
        assert_eq!(t.summary.as_deref(), Some("Fix; the, thing"), "escaping round-trips");
        assert!(matches!(t.due, Some(IcsTime::Date(_))));
    }
}
