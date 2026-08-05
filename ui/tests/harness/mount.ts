import { mount } from 'svelte';
import WeekGrid from '../../src/lib/WeekGrid.svelte';
import EventBlock from '../../src/lib/EventBlock.svelte';
import AllDayBand from '../../src/lib/AllDayBand.svelte';
import { FIXTURES } from '../fixtures';

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

const COMPONENTS: Record<string, any> = { WeekGrid, EventBlock, AllDayBand };
const target = document.getElementById('app')!;

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
  mount(COMPONENTS[name], { target, props });
}
