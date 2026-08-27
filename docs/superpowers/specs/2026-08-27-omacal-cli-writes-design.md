# omacal — CLI phase 2: writes, executed by the running app

*2026-08-27. Follows phase 1 (the read-only CLI, shipped v0.5.0) and the
event-write design (2026-08-07), whose guards are the entire reason this
phase has the shape it has.*

## Goal

A script or an agent can create, reschedule, answer and delete events from a
terminal — `omacal events create --title "Standup" …` — and every such write
passes through the guards the form already earned: the etag conflict that
refuses to clobber another device's edit, the recurrence scopes, the
notification policy that decides who gets mailed, the error sanitizer that
keeps upstream messages out of user-facing output. Phase 1's promise was
"this surface cannot damage anything"; phase 2's is "this surface can only do
what the app itself would do".

## Scope

In: the request/response transport, the write command surface (`events
create / update / delete / respond`), occurrence addressing, the notification
flag, envelope and exit-code extensions, the app-side handler, security.

Out, deliberately (§9): task (VTODO) writes, calendar management, reminder
overrides, a watch/subscribe mode, Windows.

## 1. The one rule: the app writes, or nothing does

The CLI never opens the database for writing and never carries a second copy
of any guard. Both halves of that are load-bearing:

- The guards live in the running app (`write.rs`), behind commands that hold
  state a cold process does not have — cached etags, the token cache, the
  sync loop that has to be told what changed. A CLI that reimplemented "just
  the simple case" would be the fork in the vocabulary this codebase spends
  comments preventing.
- **A write with no app running is refused, never satisfied by starting
  one.** `--sync-now`'s habit of launching an instance when none existed put
  three zombies on the reference machine and undid every widget-Quit within
  minutes (OPERATIONS, 2026-08-20) — that mistake is not getting a second
  edition with write access. Exit 5, `"omacal is not running — launch it
  first"`, and nothing else happens.

## 2. Transport: a local socket the app owns

The single-instance channel the widget drives (`--sync-now`, `--quit`) is
argv forwarding: one-way, fire-and-forget. Fine for "sync now", useless here
— **the reply is the feature**. An agent that creates an event needs the
created occurrence back; one that hits an etag conflict needs to be told,
in the envelope it can parse, that the write was refused and why. So phase 2
adds a second, dedicated channel and leaves the single-instance plumbing
exactly as it is:

- A Unix domain socket, bound by the running app at setup:
  `$XDG_RUNTIME_DIR/omacal/ipc.sock` where the runtime dir exists (Linux,
  including inside Flatpak, where the runtime dir is app-private), else
  `<app data dir>/ipc.sock` (macOS). Directory `0700`, socket `0600` — the
  same-user boundary is the security model (§7).
- Protocol: one JSON request per connection, newline-terminated, then one
  JSON reply — the **same envelope the CLI already prints** (`{"ok":true,…}`
  / `{"ok":false,…}`), so the CLI's job on success is to relay bytes, not
  translate them. Requests are capped at 64 KiB; anything larger is a
  refusal, not a read.
- Every request carries `{"v":1,"cmd":…,…}`. An unknown version or command
  answers `ok:false, kind:"usage"` — an old CLI against a newer app (or the
  reverse) degrades into a legible refusal, never a hang.
- Startup unlinks any stale socket before binding (a `hard_restart` leaves
  one behind by design — `_exit` runs no cleanups, and that is fine
  *because* this line exists). A CLI whose connect fails treats the app as
  not running; a stale socket file and no listener answer the same way.

## 3. The command surface

Under the `events` group phase 1 established. Times are civil — `--date
2026-08-30 --start 14:00 --end 15:30` — read in the app's display zone,
because that is the zone every time in omacal means (settings spec); the
second time zone is ink and takes no part here.

```
omacal events create   --title T --date D --start HH:MM --end HH:MM
                       [--end-date D] [--calendar ID] [--location L]
                       [--description TEXT] [--guest a@b]…
                       [--all-day --last-day D] [--notify all|none]
omacal events update   ID --occurrence MS [changed fields…]
                       [--scope this|following|all] [--notify all|none]
omacal events delete   ID --occurrence MS [--scope this|following|all]
                       [--notify all|none]
omacal events respond  ID yes|maybe|no [--scope this|all] [--occurrence MS]
```

- **Occurrence identity is the app's own pair** — event id plus occurrence
  start in ms — exactly as phase 1 prints it in `events list`, `agenda` and
  `search`, so the read surface hands the write surface its arguments with
  no translation. This is the same `(id, start_ms)` doctrine the popover,
  the drag layer and the reminders already share (event-write spec §1).
- **Recurrence scope is never guessed.** A write that touches a recurring
  event without `--scope` is exit 2, the three options named in the error —
  the CLI counterpart of the form's scope question. Non-recurring events
  need no flag and refuse one (`kind:"usage"`), so a script cannot carry a
  meaningless `--scope all` into a future where it means something.
- `--calendar` absent means the app's default-calendar rule, the same
  "primary, else first writable" the form seeds with; an unwritable target
  is the app's refusal, relayed.
- Wire form: the request embeds the same serde `EventInput` vocabulary
  `write.rs` already defines. One vocabulary, zero translation layers —
  a field added to `EventInput` is a field the CLI can carry the day it
  exists.

## 4. The notification flag may not be guessed

The form asks "email the guests?" out loud. The CLI never prompts (phase 1
doctrine), so the question becomes a flag with no default *whenever it has
consequences*: any write on an event that has guests — create with
`--guest`, update, delete, all of them — requires `--notify all|none`
explicitly, and its absence is exit 2 with the choice named. Writes with no
guests accept the flag but do not require it; nothing is mailed either way.
A refusal here costs a script one flag; a silent default costs somebody a
meeting-cancelled email they did not mean to send, and those are not
comparable prices.

`respond` carries no flag: answering an invitation notifies the organizer by
the protocol's own nature, on every client, and pretending otherwise would
be a lie shaped like an option.

## 5. Envelope, exit codes, registers

Phase 1's contract extends; nothing existing changes meaning.

- Exit codes: `0` ok · `2` usage · `3` no database · `4` error — joined by
  `5` **not running** (§1) and `6` **refused by the app's own guards**: the
  etag conflict, a validation the form would also have shown, an unwritable
  calendar. The distinction 4/6 is the one an agent acts on — retry nothing
  on 6, read the message, change the request.
- `--json` prints the envelope relayed from the app, `kind` set to
  `"usage" | "not-running" | "refused" | "error" | "timeout"`. The human
  register prints the same fact as a sentence — which is already the app's
  sanitized `user_facing` string, because the handler answers with nothing
  else (§6).
- The CLI waits for the app synchronously — a write goes to Google or a
  CalDAV server before it answers — with a 60-second cap: past it,
  `kind:"timeout"`, exit 4, and the truth that the write's fate is unknown
  stated in the message. No retry loop; retrying an unknown-fate write is
  how duplicates are minted.

## 6. The app-side handler

A new `ipc.rs`, deliberately shaped like `cli.rs`'s mirror image:

- An accept loop spawned in setup, one task per connection, reading the one
  request line, dispatching on `cmd`, replying with the envelope. The
  dispatch calls **the same command implementations the webview invokes** —
  `create_event`, `update_event`, `delete_event_cmd`, `respond_to_event` —
  through the same `AppState`, so there is exactly one code path from any
  surface to any provider. Where those functions currently take
  `tauri::State`, they grow an `_impl` split (the `set_sync_interval_impl`
  pattern settings.rs already uses) rather than a parallel body.
- After a successful write, the handler triggers exactly what the webview
  triggers after its own: the reload event to the window (the grid a user
  is looking at updates while the agent works) and the widget feed refresh.
  An agent's write must be indistinguishable from a form's write from every
  seat in the house.
- Errors cross the socket **already sanitized** — the handler answers with
  `errors::user_facing` output and never a raw provider message, so the
  sanitizer stays the single gate it is today (event-write spec §7).

## 7. Security

- The socket is same-user by filesystem permission (`0600` in a `0700`
  dir); that is the trust boundary, and it is the same one the database
  file already has. `SO_PEERCRED` uid verification is cheap Linux hardening
  on top, not the model.
- The handler accepts the enumerated commands and nothing else; requests
  are size-capped; malformed JSON is a usage refusal. Nothing in any reply
  can carry a secret, because replies are built from the same sanitized
  vocabulary the webview sees and tokens never leave the keyring.
- No network listener, ever. This is a local socket for the local user; the
  moment somebody wants remote, the answer is SSH, not a port.

## 8. Testing

Per the testing standard: the pure core tested exhaustively, the I/O shell
thin enough to inspect.

- Pure and tested: flag parsing to request JSON (table, like phase 1's);
  the scope-refusal rules (§3) as a table; the notify-flag requirement
  matrix (§4); envelope→exit-code mapping including 5 and 6; app-side
  request decode to `EventInput` (round-trip against the same serde the
  webview uses, so the vocabularies provably cannot fork).
- The handler's dispatch tested against a fixture pool exactly as
  `write.rs`'s own tests run — the socket is not needed to prove the
  dispatch, only the plumbing.
- One end-to-end: bind a real socket in a temp dir, run the accept loop
  against a fixture state, drive create → conflict → delete through the
  same client function the CLI links. No GUI, no network; the Radicale
  recipe (caldav e2e) already covers the provider legs.

## 9. Deferred

- **Tasks**: VTODO complete/add/delete over the same socket — phase 3, and
  the transport needs no change for it.
- **Recurrence authoring** (`--repeat` on create, rule edits): the scope
  machinery passes through untouched; the authoring vocabulary deserves its
  own design pass rather than an RRULE string bolted on.
- **Watch mode** (a subscription over the socket, events-as-they-change):
  wanted for agents, different protocol shape, not now.
- **Reminder overrides, calendar CRUD, Windows named pipes, `--launch`.**

The sharpest sentence phase 1 could say was "it cannot damage anything".
Phase 2 retires that sentence and replaces it with a better one: "it can do
exactly what you could do, and nothing you could not."
