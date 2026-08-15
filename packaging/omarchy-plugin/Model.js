// Pure formatting and grouping for the omacal.upcoming bar widget.
// No Qt types beyond Date, so every function here is testable by eye and
// none of it can hold a reference into the QML tree.

// Parses the feed file's text into an array of event objects, tolerating an
// absent or half-written file (the writer renames atomically, but the reader
// must still never throw — a broken feed means "no events", not a broken bar).
function parseFeed(text) {
  if (!text || String(text).trim() === "") return null
  try {
    var feed = JSON.parse(String(text))
    if (!feed || !Array.isArray(feed.events)) return null
    return feed
  } catch (e) {
    return null
  }
}

// Midnight of the local day `offset` days after `nowMs`. The feed carries each
// event's calendar zone, but the bar buckets days in the machine's own zone —
// "today" on a status bar means the day the user is sitting in, and quattro's
// own clock widget makes the same call.
function dayStart(nowMs, offset) {
  var d = new Date(nowMs)
  d.setHours(0, 0, 0, 0)
  return d.getTime() + offset * 86400000
}

function pad(n) {
  return (n < 10 ? "0" : "") + n
}

function clock(ms) {
  var d = new Date(ms)
  return pad(d.getHours()) + ":" + pad(d.getMinutes())
}

var DAY_NAMES = ["SUNDAY", "MONDAY", "TUESDAY", "WEDNESDAY", "THURSDAY", "FRIDAY", "SATURDAY"]
var MONTH_NAMES = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"]

// Section title for a day further out than tomorrow: "FRIDAY 22 AUG".
function dayTitle(ms) {
  var d = new Date(ms)
  return DAY_NAMES[d.getDay()] + " " + d.getDate() + " " + MONTH_NAMES[d.getMonth()]
}

// "09:30 – 10:15", or "ALL DAY" — the row's leading column.
function timeText(ev) {
  if (ev.all_day) return "ALL DAY"
  return clock(ev.start_ms) + " – " + clock(ev.end_ms)
}

// The caption under a title: location and headcount, whichever exist.
// A solo event (attendees 0) says nothing about people rather than "0 people".
function metaText(ev) {
  var parts = []
  if (ev.location) {
    // Meeting links pasted into the location field are noise at caption
    // length; show the host instead of the whole URL.
    var loc = String(ev.location)
    var m = loc.match(/^https?:\/\/([^\/\s]+)/)
    parts.push(m ? m[1] : loc)
  }
  if (ev.attendees > 1) parts.push(ev.attendees + " people")
  else if (ev.attendees === 1) parts.push("1 person")
  if (ev.response === "needsAction") parts.push("not answered")
  else if (ev.response === "tentative") parts.push("maybe")
  return parts.join("  ·  ")
}

// "in 25 min" / "in 3 h" for the hero's meta line; "" when nothing is ahead.
function leadText(ms, nowMs) {
  var min = Math.round((ms - nowMs) / 60000)
  if (min <= 0) return "now"
  if (min < 60) return "in " + min + " min"
  var h = Math.floor(min / 60)
  if (h < 24) return "in " + h + " h" + (min % 60 ? " " + (min % 60) + " min" : "")
  return "in " + Math.floor(h / 24) + " d"
}

// "ends in 35 min" for a running event's caption suffix.
function endsText(ev, nowMs) {
  var min = Math.round((ev.end_ms - nowMs) / 60000)
  if (min < 60) return "ends in " + Math.max(1, min) + " min"
  return "ends " + clock(ev.end_ms)
}

// Groups the feed into the popup's sections at `nowMs`. The popup is a
// glance, not a planner, so it shows **one day**:
//
//   NOW       timed events started and not yet over
//   ONGOING   multi-day all-day events that began before today (a trip, a
//             conference). Context, not agenda — they never count as "today
//             still has something".
//   TODAY     starts later today (all-day events starting today included)
//
// When nothing is left for today, the day section becomes the **nearest
// future day that has anything** — Friday evening with an empty Saturday
// shows Sunday's plans under "SUNDAY 16 AUG", not a blank. Days between are
// silently skipped; days past the chosen one are never shown.
//
// Events already over are dropped — the feed refreshes on sync, and between
// syncs this is what keeps the list truthful. Rows beyond `cap` are cut from
// the tail: losing the afternoon matters less than losing the meeting you
// are in.
function sections(events, nowMs, cap) {
  if (!events) return []
  var today = dayStart(nowMs, 0)
  var now = []
  var ongoing = []
  var byOffset = {}
  var offsets = []

  for (var i = 0; i < events.length; i++) {
    var ev = events[i]
    if (ev.end_ms <= nowMs) continue
    if (!ev.all_day && ev.start_ms <= nowMs) {
      now.push(ev)
      continue
    }
    // All-day starts are midnight in the calendar's zone; anchoring half a
    // day in buckets them onto the right local date whatever the skew.
    var anchor = ev.all_day ? ev.start_ms + 43200000 : ev.start_ms
    var offset = Math.floor((dayStart(anchor, 0) - today) / 86400000)
    if (ev.all_day && offset < 0) {
      ongoing.push(ev)
      continue
    }
    if (offset < 0) offset = 0
    if (!byOffset[offset]) {
      byOffset[offset] = []
      offsets.push(offset)
    }
    byOffset[offset].push(ev)
  }
  offsets.sort(function(a, b) { return a - b })

  // Today when it still has rows; otherwise the nearest day that does.
  var chosen = -1
  if (byOffset[0]) chosen = 0
  else if (offsets.length > 0) chosen = offsets[0]

  var out = []
  var total = 0
  function push(title, rows) {
    var kept = rows.slice(0, Math.max(0, cap - total))
    if (kept.length === 0) return
    total += kept.length
    out.push({ title: title, rows: kept })
  }

  if (now.length) push("NOW", now)
  if (ongoing.length) push("ONGOING", ongoing)
  if (chosen === 0) push("TODAY", byOffset[0])
  else if (chosen === 1) push("TOMORROW", byOffset[1])
  // Title from the chosen day itself, not from an event's start instant —
  // an all-day start is a foreign-zone midnight and can misname the day.
  else if (chosen > 1) push(dayTitle(dayStart(nowMs, chosen) + 43200000), byOffset[chosen])
  return out
}

// "until 12 DEC" for an ongoing multi-day event. `end_ms` is an exclusive
// midnight in the *calendar's* zone; twelve hours back lands mid-way into
// the last covered day, which reads as the right local date for any
// calendar-vs-machine zone skew under half a day (a 1 ms step does not —
// verified against a Sofia calendar read from an IST machine).
function untilText(ev) {
  var d = new Date(ev.end_ms - 43200000)
  return "until " + d.getDate() + " " + MONTH_NAMES[d.getMonth()]
}

// The next timed event still ahead — what the hero counts down to.
function nextAhead(events, nowMs) {
  if (!events) return null
  for (var i = 0; i < events.length; i++) {
    var ev = events[i]
    if (!ev.all_day && ev.start_ms > nowMs) return ev
  }
  return null
}

// The timed event we are currently inside, if any.
function current(events, nowMs) {
  if (!events) return null
  for (var i = 0; i < events.length; i++) {
    var ev = events[i]
    if (!ev.all_day && ev.start_ms <= nowMs && ev.end_ms > nowMs) return ev
  }
  return null
}

function title(ev) {
  return ev && ev.title ? String(ev.title) : "(no title)"
}

// The feed's task slice (OmaCal ≥ 0.2.0), re-judged against the popup's own
// clock: what the writer called "due soon" may have become overdue since.
function taskRows(feed, nowMs) {
  if (!feed || !Array.isArray(feed.tasks)) return []
  var out = []
  for (var i = 0; i < feed.tasks.length; i++) {
    var t = feed.tasks[i]
    var deadline = t.all_day ? t.due_ms + 86400000 : t.due_ms
    out.push({
      title: t.title,
      color: t.color || null,
      list: t.list || "",
      overdue: deadline <= nowMs,
      label: taskLabel(t, nowMs)
    })
  }
  return out
}

function taskLabel(t, nowMs) {
  var deadline = t.all_day ? t.due_ms + 86400000 : t.due_ms
  if (deadline <= nowMs) return "OVERDUE"
  var today = dayStart(nowMs, 0)
  var anchor = t.all_day ? t.due_ms + 43200000 : t.due_ms
  var offset = Math.floor((dayStart(anchor, 0) - today) / 86400000)
  if (offset <= 0) return t.all_day ? "today" : clock(t.due_ms)
  if (offset === 1) return "tomorrow"
  return dayTitle(anchor)
}
