<!-- ui/src/lib/EventBlock.svelte — PLACEHOLDER, replaced in Task 13 -->
<script lang="ts">
  import type { UiEvent, Placed } from './api';
  let { event, placed }: { event: UiEvent; placed: Placed } = $props();
</script>

<div
  class="ev"
  style="top:{placed.top * 100}%; height:{placed.height * 100}%;
         left:calc({(placed.column * 100) / placed.columns}% + 3px);
         width:calc({100 / placed.columns}% - 6px);"
>{event.title}</div>

<style>
  .ev { position: absolute; border-radius: 6px; padding: 2px 6px; font-size: 10px;
        overflow: hidden; border-left: 2px solid var(--accent);
        background: color-mix(in srgb, var(--accent) 7%, transparent); }
</style>
