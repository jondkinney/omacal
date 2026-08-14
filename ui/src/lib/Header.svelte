<!-- ui/src/lib/Header.svelte -->
<script lang="ts">
  import { reauthMessage, syncLight, type AppStatus } from './status';
  import type { Calendar } from './calendars';
  import { escapeCloses } from './dismiss.svelte';
  import { listable } from './filmstrip';
  import CalendarPopover from './CalendarPopover.svelte';
  import SettingsModal from './SettingsModal.svelte';
  import ViewSwitcher, { type View } from './ViewSwitcher.svelte';

  let {
    status, anchorMs, weekStartMs, busy, error, calendars, view, onpick,
    onsettingschange,
    listMode, onToggleList,
    onPrev, onNext, onToday, onSearch, onSignIn, onSync, oncalendarchange,
    open = $bindable(false),
  }: {
    status: AppStatus | null;
    /** The date every view is rendered against — `App`'s own anchor. Day and
     *  Month are built from this one directly. */
    anchorMs: number;
    /** The Monday of `anchorMs`'s week, which is what Week view renders. */
    weekStartMs: number;
    busy: boolean;
    error: string | null;
    calendars: Calendar[];
    /** The view the switcher shows as current — `App`'s own `view` state,
     *  passed straight through. */
    view: View;
    /** Forwarded straight to `ViewSwitcher`'s `onpick`. */
    onpick: (v: View) => void;
    /** Whether Day, Week and Month are drawn as a list — `App`'s own state,
     *  which is the stored preference (filmstrip spec §4). */
    listMode: boolean;
    /** Asked to flip it. Not flipped here: the value is stored in the settings
     *  table, and `App` owns every write in this app. */
    onToggleList: () => void;
    onPrev: () => void; onNext: () => void; onToday: () => void;
    /** Opens the search overlay. A small control rather than a field: the
     *  header was emptied on purpose (settings spec §1) and putting a
     *  permanent input back into it would undo that. */
    onSearch: () => void;
    /** Passed straight through to `SettingsModal` — see its own comment. */
    onsettingschange?: (s: import('./settings').AppSettings) => void;
    onSignIn: () => void; onSync: () => void; oncalendarchange: () => void;
    /** Bound through to `CalendarPopover` — lets `App` open the picker
     *  straight after a sign-in, from outside the popover's own trigger. */
    open?: boolean;
  } = $props();

  // The title names the month of whatever unit is actually on screen. Week's
  // has always been the month its *Monday* falls in — the week of Mon 29 Jan
  // reads "January" even though it runs into February — but Day and Month
  // render `anchorMs`'s own month, and titling those from the week start
  // names the wrong one whenever the anchor's week began in the previous
  // month. Reachable in two keystrokes from today (`3`, then `L`: a September
  // grid titled "August"), and in one from Day view on any 1st-of-month that
  // isn't a Monday.
  const titleMs = $derived(view === 'week' ? weekStartMs : anchorMs);
  const title = $derived(
    new Date(titleMs).toLocaleDateString(undefined, { month: 'long', year: 'numeric' })
  );

  // `‹`/`›` step the current view's unit, exactly as `H`/`L` do (`App`'s own
  // `step`), so the label has to follow the view too: a control announced as
  // "Previous week" that moves the grid by a month — or, in Month view,
  // sometimes not at all and sometimes across a month boundary — is worse
  // than either behaviour on its own.
  const NAV_UNIT: Record<View, string> = {
    day: 'day', week: 'week', month: 'month', year: 'year', bigyear: 'year',
  };
  const unit = $derived(NAV_UNIT[view]);
  const connected = $derived((status?.accounts.length ?? 0) > 0);
  /** Accounts whose Google sign-in is dead — the backend has stopped syncing
   *  them and re-consent is the only fix, so this is the one failure with a
   *  button on it. */
  const reauth = $derived(status?.needs_reauth ?? []);

  // "Synced 4 min ago" is a function of the clock, so it has to be told the
  // clock moved. Without this it only ever recomputes when `status` changes —
  // that is, when a sync succeeds — so it froze at its last value exactly when
  // sync had stopped working and its staleness was the thing worth seeing.
  // Same shape as WeekGrid's current-time line.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { now = Date.now(); }, 30_000);
    return () => clearInterval(id);
  });

  // The zone every time on screen is drawn in — the browser's, which is the
  // system's. Named in the header because nothing else says it: 15:00 reads
  // identically in Sofia and in Delhi, and which one the grid means was
  // otherwise something the user had to remember. The offset rides in the
  // hover off `now`, so a DST change while the app is open is not shown stale.
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const zoneOffset = $derived(
    new Intl.DateTimeFormat(undefined, { timeZoneName: 'shortOffset' })
      .formatToParts(new Date(now))
      .find((p) => p.type === 'timeZoneName')?.value ?? ''
  );
  /**
   * The status light: one call, one answer, used for both the colour and the
   * words (spec §2). See `syncLight` — deriving them separately is how a dot
   * ends up green while announcing a failure.
   */
  const light = $derived(
    syncLight(
      {
        connected, busy, error, reauth: reauth.length > 0,
        lastSyncMs: status?.last_sync_ms ?? null,
      },
      now,
    ),
  );

  /** The hamburger's menu. Everything that used to sit in the header and is
   *  used rarely now lives here (spec §1). */
  let menuOpen = $state(false);
  let settingsOpen = $state(false);

  function openSettings() {
    menuOpen = false;
    settingsOpen = true;
  }

  /** Runs `fn` and shuts the menu — every item in it is a one-shot action, and
   *  a menu left standing over the thing it just did is a menu the user has to
   *  dismiss before they can see what happened. */
  function fromMenu(fn: () => void) {
    menuOpen = false;
    fn();
  }

  /**
   * **A parent-driven `open` opens the menu it now lives in.**
   *
   * `App` sets `open` after every sign-in so a freshly imported set of
   * calendars — all switched on by default — is never left syncing behind the
   * user's back without them having seen it. That worked while the picker sat
   * directly in the header; behind a closed hamburger, `open` becoming true
   * would render nothing at all and the behaviour would be silently gone.
   *
   * One-way on purpose: closing the picker leaves the menu standing, which is
   * what makes the layers behave like layers.
   */
  $effect(() => {
    if (open) menuOpen = true;
  });

  // Escape closes the menu — but only when it is the topmost thing open.
  // `CalendarPopover` has its own `window` handler, so without the `!open`
  // term one keystroke closes the picker *and* the menu it sits inside, and a
  // user who meant "not that panel" loses both.
  //
  // There is deliberately no `!settingsOpen` term beside it. It reads as the
  // matching precaution and is dead: `openSettings` closes the menu on the way
  // to opening the modal, so `menuOpen` is already false whenever the modal is
  // up. A guard that can never fire is a guard no test can prove, and this
  // file has been through that once already.
  escapeCloses(() => menuOpen && !open, () => (menuOpen = false));
</script>

<!-- `data-tauri-drag-region` here and on the title, not on a wrapper around
     everything: with `titleBarStyle: "Overlay"` there is no title bar left to
     drag the window by, and without a region named in the DOM the window
     cannot be moved at all. Tauri's injected handler walks the composed path
     from whatever was clicked and stops at the first interactive element, so
     a bare attribute means "clicks that land on *this* element" — the
     header's own free space and padding, and the title's own box. Every
     button below stays a button.

     Deliberately not `data-tauri-drag-region="deep"`, which would hand the
     whole subtree over. **The reason has changed and the constraint has not.**
     It used to be `CalendarPopover`'s open panel, which sat directly in this
     header; the picker has since moved behind the hamburger, but it moved
     *into the menu*, which is still inside this header — so the panel's
     account labels, calendar names and hint text are still here, joined now by
     the menu's own items. `"deep"` would make every one of them a place to
     drag the window from instead of a thing to read or click.

     The settings modal is the one panel this no longer covers: it renders as a
     sibling of `<header>`, outside it, so nothing about it depends on this. -->
<header class:overlay={status?.overlay_titlebar} data-tauri-drag-region>
  <div class="left">
    <h1 data-tauri-drag-region>{title}</h1>
    <div class="nav">
      <button onclick={onPrev} aria-label="Previous {unit}">‹</button>
      <button onclick={onNext} aria-label="Next {unit}">›</button>
    </div>
    <button class="today" onclick={onToday}>Today</button>
    <ViewSwitcher {view} {onpick} />
    <!-- **Absent in Year and Big Year, not present and inert** (filmstrip spec
         §2): a control that does nothing is worse than one that is not there.
         `listable` is the same predicate `App`'s `F` key reads, so the key
         cannot set a preference in a view that offers no way to see or undo it.

         One button rather than two, `aria-pressed` rather than a pair of
         `aria-label`s that change under the pointer: the accessible name stays
         "List view" whichever mode it is in, and the state is the state. The
         glyph shows the mode currently on screen — `☰` while it is a list, `▦`
         while it is a grid — which is the pairing the design names it by. -->
    {#if listable(view)}
      <button
        class="filmstrip"
        aria-label="List view"
        aria-pressed={listMode}
        title="List view (F)"
        onclick={onToggleList}
      >{listMode ? '☰' : '▦'}</button>
    {/if}
  </div>

  <div class="right">
    <span class="tz" title="{zone} · {zoneOffset}">{zone}</span>

    {#if status?.demo}
      <span class="demo">DEMO DATA</span>
    {/if}

    <!-- **Spec §2: a light, not a sentence.** `is this stale?` is a question
         answered by glancing, so the state stays in the header while the words
         move into the hover.

         `role="img"` with a name rather than a bare decorative span: a colour
         alone is not a status, and this is what a screen reader and a spec
         both read. Deliberately not `role="status"` — that is a live region and
         would announce every routine sync, which is noise for the one state
         (`synced`) that exists to be ignored. The failure case is announced by
         the error banner below, which is real text. -->
    <span
      class="light {light.state}"
      role="img"
      aria-label={light.label}
      title={light.label}
    ></span>

    {#if !connected && !status?.demo}
      <!-- Stays in the header rather than moving behind the hamburger. The
           three controls §1 moves are all *rare*; this is the one thing a new
           user has to find, and it is the whole of the app until they do. -->
      <button class="primary" onclick={onSignIn} disabled={busy}>
        {busy ? 'Connecting…' : 'Connect Google Calendar'}
      </button>
    {/if}

    <button class="search" aria-label="Search" title="Search (/)" onclick={onSearch}>⌕</button>

    <div class="menuwrap">
      <button
        class="burger"
        aria-label="Menu"
        aria-expanded={menuOpen}
        onclick={(e) => {
          menuOpen = !menuOpen;
          // WebKit does not focus a <button> on click, only on Tab — the same
          // reason `CalendarPopover.toggle` does this. Without it Escape has
          // nothing local to bubble from until the user tabs somewhere.
          if (menuOpen) (e.currentTarget as HTMLElement).focus();
        }}
      >☰</button>

      {#if menuOpen}
        <!-- Click-away, a sibling rather than a document listener: no global
             state to leak if this unmounts while open. `CalendarPopover`'s own
             scrim renders above this one when its panel is open, so dismissing
             the picker and dismissing the menu are two separate clicks —
             which is the honest behaviour for two nested layers. -->
        <button class="scrim" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>

        <div class="menu" role="group" aria-label="Menu">
          {#if calendars.length > 0}
            <!-- **Rehomed, not rewritten.** The whole component moves inside
                 the menu with its trigger, its panel and its `bind:open`
                 intact, so everything it does — `selected` and `sync_enabled`
                 staying separate, the busy set, the keyed each — travels
                 unchanged. Its own specs mount it standalone and never knew
                 where it lived. -->
            <CalendarPopover {calendars} onchange={oncalendarchange} bind:open />
          {/if}
          {#if connected && !status?.demo}
            <!-- Demo mode's seeded account never went through OAuth, so a sync
                 would only fail; offering the button at all would be a control
                 that exists solely to produce an error. Same reasoning covers
                 Add account: sign_in refuses server-side in demo mode
                 (demo_sync_guard) regardless of whether an account is already
                 connected. -->
            <button onclick={() => fromMenu(onSync)} disabled={busy}>Sync now</button>
            <button onclick={() => fromMenu(onSignIn)} disabled={busy}>Add account</button>
          {/if}
          <button onclick={openSettings}>Settings…</button>
        </div>
      {/if}
    </div>
  </div>
</header>

{#if settingsOpen}
  <SettingsModal
    accounts={status?.accounts ?? []}
    {busy}
    onclose={() => (settingsOpen = false)}
    {calendars}
    {oncalendarchange}
    {onsettingschange}
    onSignIn={() => {
      settingsOpen = false;
      onSignIn();
    }}
  />
{/if}

<!-- Below the header rather than inside it, and free to wrap. The likeliest
     first-run failure is the missing config file, whose actionable half —
     "Create it with client_id and client_secret" — is the part that used to
     fall off the end of a 320px ellipsised line and live only in a title
     attribute nobody hovers. -->
{#if error}
  <p class="err">{error}</p>
{/if}

<!-- The reconnect prompt, and the reason it is not the `error` banner above:
     `error` is transient — cleared by the next navigation, overwritten by the
     next failure — and re-consenting is the only fix for this state, so the
     message has to sit there until it happens and has to carry the button
     that does it. Never in demo mode, where no account went through OAuth and
     `sign_in` refuses server-side anyway (`demo_sync_guard`). -->
{#if reauth.length > 0 && !status?.demo}
  <p class="err reauth" data-testid="reauth-banner">
    <span>{reauthMessage(reauth)}</span>
    <button class="primary" onclick={onSignIn} disabled={busy}>
      {busy ? 'Connecting…' : 'Reconnect'}
    </button>
  </p>
{/if}

<style>
  /* `padding-top`, moved here out of `App`'s `main` — see the comment there
     for why it is layout-neutral. A drag region covers its own box and
     nothing above it, so this is what carries the top edge of the window into
     something the user can grab; as `main` padding the same 14px was dead,
     and it is the first strip anyone reaches for to move a window. Padding
     rather than margin, deliberately: a margin is outside the element and
     would be dead in exactly the same way. */
  header { display: flex; align-items: center; justify-content: space-between;
           gap: 12px; padding-top: 14px; margin-bottom: 12px; flex-wrap: wrap; }
  /* Room for macOS's close/minimise/zoom buttons, which `titleBarStyle:
     "Overlay"` draws *over* this webview rather than in a strip above it.
     Without it the month title renders underneath them.

     Measured, not guessed — the first guess at this was 78px. An NSWindow
     built exactly as tao builds ours (`titlebarAppearsTransparent` +
     `FullSizeContentView`; see tauri-runtime-wry's `title_bar_style`) puts the
     three buttons at x 7..61 and y 6..22, in CSS pixels from the webview's own
     top-left, on macOS 26.3.1. `App`'s `main` has already inset this header by
     its 16px gutter, so 60px more starts the title at x=76 — 15px clear of the
     zoom button, and about where a macOS toolbar's first item sits.

     Only under `.overlay`, which is `status.overlay_titlebar` and is false on
     Linux even though `tauri.conf.json` names `titleBarStyle` there too: the
     key is macOS-only, Omarchy keeps its controls in their own strip, and this
     padding would be a dead gap. */
  header.overlay { padding-left: 60px; }
  .left, .right { display: flex; align-items: center; gap: 8px; }
  h1 { font-size: 19px; font-weight: 600; letter-spacing: -.025em; margin: 0; white-space: nowrap; }
  .nav { display: flex; gap: 1px; }
  button { font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
           background: color-mix(in srgb, var(--text) 6%, transparent);
           border: 0; border-radius: 6px; padding: 4px 10px; }
  button:disabled { opacity: .5; cursor: default; }
  .nav button { width: 22px; padding: 3px 0; font-size: 13px; }
  .today { border: 1px solid color-mix(in srgb, var(--text) 12%, transparent); background: none; }
  .primary { background: var(--accent); color: var(--bg); font-weight: 600; }
  .demo { font-size: 10.5px; }

  /* Spec §2. Eight pixels, no label, and **the healthy state must not draw the
     eye** — `--sync-ok` is a neutral at 30%/28% rather than a hue, so a synced
     calendar reads as present and unremarkable. The two states worth noticing
     borrow the variables that already mean those things. */
  .light { width: 8px; height: 8px; border-radius: 50%; flex: none; display: block;
           background: var(--sync-idle); }
  .light.synced { background: var(--sync-ok); }
  .light.syncing { background: var(--accent); }
  .light.failed { background: var(--error); }

  /* A glyph, not a field. The magnifier is the one control search gets in the
     header; the overlay is where the typing happens. */
  .search { font-size: 15px; line-height: 1; padding: 2px 8px; }

  /* Beside the switcher rather than inside it, and styled to sit *next to* it
     rather than as a sixth slot: the toggle is orthogonal to the view (spec
     §1), and a button sharing the switcher's joined-segment shape would read as
     a view you can be in. Pressed takes the same accent fill the active view
     slot does, because that is what "on" already looks like in this header. */
  .filmstrip { font-size: 12px; line-height: 1; padding: 4px 8px; }
  .filmstrip[aria-pressed='true'] { background: var(--accent); color: var(--bg); }

  .menuwrap { position: relative; }
  .burger { font-size: 13px; line-height: 1; padding: 3px 8px; }
  .scrim { position: fixed; inset: 0; background: none; border: 0; cursor: default;
           z-index: 40; }
  /* Above its own scrim, and below `CalendarPopover`'s panel — which renders
     inside this one and carries its own z-index of 41. */
  .menu { position: absolute; right: 0; top: calc(100% + 6px); z-index: 41;
          min-width: 150px; display: flex; flex-direction: column; gap: 2px;
          background: var(--surface); border: 1px solid var(--hairline);
          border-radius: 8px; padding: 6px; box-shadow: 0 8px 28px rgba(0, 0, 0, .45); }
  .menu button { text-align: left; background: none; width: 100%; }
  .menu button:hover:not(:disabled) { background: color-mix(in srgb, var(--text) 6%, transparent); }
  .demo { color: var(--demo); letter-spacing: .06em; font-weight: 600; }

  .tz { font-size: 10px; color: var(--muted); letter-spacing: .03em;
        white-space: nowrap; }
  .err { color: var(--error); font-size: 11.5px; line-height: 1.45; margin: 0 0 12px;
         padding: 7px 10px; border-radius: 6px;
         background: color-mix(in srgb, var(--error) 9%, transparent);
         overflow-wrap: anywhere; }
  /* The one banner with a button in it: the sentence wraps, the button
     stays a button. Inherits .err's colours — this is still a failure, just
     one with a fix attached. */
  .reauth { display: flex; align-items: center; justify-content: space-between;
            gap: 12px; flex-wrap: wrap; }
  .reauth button { border: none; border-radius: 6px; padding: 5px 12px;
                   font-size: 11.5px; cursor: pointer; }
</style>
