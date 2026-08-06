import { mount, unmount } from 'svelte';
import WeekGrid from '../../src/lib/WeekGrid.svelte';
import EventBlock from '../../src/lib/EventBlock.svelte';
import AllDayBand from '../../src/lib/AllDayBand.svelte';
import Header from '../../src/lib/Header.svelte';
import CalendarPopover from '../../src/lib/CalendarPopover.svelte';
import EventPopover from '../../src/lib/EventPopover.svelte';
import { FIXTURES } from '../fixtures';
import { installTauriStub } from './tauri';

// Palette normally arrives from the Rust get_palette command; the harness
// applies the same fallback_dark values so snapshots are deterministic.
const PALETTE: Record<string, string> = {
  '--bg': '#17171a', '--surface': '#1e1e22', '--text': '#e8e8ea',
  '--muted': '#8a8a90', '--accent': '#5b8def',
  '--hairline': 'rgba(255,255,255,.055)',
  '--hour-rule': 'rgba(255,255,255,.035)',
  '--today-tint': 'rgba(255,255,255,.028)',
};
for (const [k, v] of Object.entries(PALETTE)) {
  document.documentElement.style.setProperty(k, v);
}
document.body.style.background = PALETTE['--bg'];
document.body.style.color = PALETTE['--text'];

const params = new URLSearchParams(location.search);
const name = params.get('c') ?? 'WeekGrid';
const fixture = params.get('f') ?? 'default';

const COMPONENTS: Record<string, any> = { WeekGrid, EventBlock, AllDayBand, Header, CalendarPopover, EventPopover };
const target = document.getElementById('app')!;

if (name === 'App') {
  // App is the whole application, not a leaf: it takes no props, and every
  // input it has arrives over the Tauri IPC. So the stub goes in first, and
  // the component is imported only afterwards — nothing may call `invoke`
  // before `window.__TAURI_INTERNALS__` exists.
  installTauriStub(fixture);
  const { default: App } = await import('../../src/App.svelte');
  mount(App, { target });
} else {
  const props = FIXTURES[name]?.[fixture];
  if (!props) {
    target.textContent = `no fixture ${name}/${fixture}`;
  } else {
    // EventBlock is absolutely positioned; give it a sized relative parent.
    if (name === 'EventBlock') {
      target.style.position = 'relative';
      target.style.height = '480px';
      target.style.width = '220px';
    }
    // Unlike its neighbours here, CalendarPopover calls `invoke` itself —
    // ticking a box or clicking Add/Remove goes straight to the three
    // calendar commands. It still takes its `calendars` from a fixture
    // prop rather than `get_calendars`, so the scenario name only matters
    // for the write commands the stub answers.
    if (name === 'CalendarPopover') installTauriStub(fixture);
    // WeekGrid takes its own `week` from a fixture prop too, same as every
    // other leaf here, but clicking one of its blocks opens a real
    // `EventPopover` that calls `event_detail`/`refresh_event`/
    // `respond_to_event` itself — installed unconditionally (harmless for
    // the specs that never click anything) rather than gated on the
    // fixture name, since it is the *click*, not the fixture, that decides
    // whether anything actually reaches `invoke`.
    if (name === 'WeekGrid') installTauriStub(fixture);
    if (name === 'EventPopover') {
      // EventPopover calls `invoke` itself too (`respond_to_event`), same
      // reasoning as CalendarPopover above.
      installTauriStub(fixture);
      // In the real app, WeekGrid owns whether EventPopover exists at all —
      // `onclose` sets its `detail` to null, which unmounts the popover via
      // `{#if}`. Mounted standalone here, nothing else plays that role, so
      // `onclose` has to actually remove the component itself or "Escape
      // closes it" would have nothing to observe: the fixture's own
      // `onclose` is a no-op, since it can't know about this harness.
      let app: object;
      app = mount(EventPopover, { target, props: { ...props, onclose: () => unmount(app) } });
    } else {
      mount(COMPONENTS[name], { target, props });
    }
  }
}
