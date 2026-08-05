<!-- ui/src/lib/AllDayBand.svelte — PLACEHOLDER, replaced in Task 14 -->
<script lang="ts">
  import type { Lane, UiEvent } from './api';
  let { lanes, events, overflow }:
    { lanes: Lane[]; events: UiEvent[]; overflow: number[] } = $props();
</script>

{#if lanes.length}
  <div class="band">{lanes.length} all-day · {events.length} events · {overflow.length} hidden</div>
{/if}

<style>
  .band { font-size: 9.5px; color: var(--muted); padding: 3px 0 6px 44px;
          border-bottom: 1px solid var(--hairline); }
</style>
