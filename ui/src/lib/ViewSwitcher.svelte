<!-- ui/src/lib/ViewSwitcher.svelte -->
<script module lang="ts">
  /** The five slots the plan settles on (spec §10). `year` and `bigyear` ship
   *  disabled — Plan 4 fills them in rather than this control being rebuilt
   *  when it does. */
  export type View = 'day' | 'week' | 'month' | 'year' | 'bigyear';
</script>

<script lang="ts">
  const SLOTS: { id: View; label: string; disabled?: boolean }[] = [
    { id: 'day', label: 'Day' },
    { id: 'week', label: 'Week' },
    { id: 'month', label: 'Month' },
    { id: 'year', label: 'Year', disabled: true },
    { id: 'bigyear', label: 'Big Year', disabled: true },
  ];

  let { view, onpick }: { view: View; onpick: (v: View) => void } = $props();
</script>

<div class="vswitch" role="group" aria-label="View">
  {#each SLOTS as slot}
    <button
      class:active={view === slot.id}
      disabled={slot.disabled}
      aria-pressed={view === slot.id}
      onclick={() => onpick(slot.id)}
    >{slot.label}</button>
  {/each}
</div>

<style>
  .vswitch { display: flex; gap: 1px; }
  .vswitch button {
    font: inherit; font-size: 11px; color: var(--muted); cursor: pointer;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    border: 0; padding: 4px 10px;
  }
  .vswitch button:first-child { border-radius: 6px 0 0 6px; }
  .vswitch button:last-child { border-radius: 0 6px 6px 0; }
  .vswitch button.active { background: var(--accent); color: var(--bg); font-weight: 600; }
  .vswitch button:disabled { opacity: .5; cursor: default; }
</style>
