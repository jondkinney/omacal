<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { matchingTimezones } from './timezone-search';
  let { id, label, zones, value = $bindable(''), disabled = false, emptyLabel }: {
    id: string; label: string; zones: string[]; value?: string; disabled?: boolean; emptyLabel: string;
  } = $props();
  let open = $state(false);
  let draft = $state('');
  let query = $state('');
  let active = $state(0);
  let input: HTMLInputElement;
  let left = $state(0), top = $state(0), width = $state(300), maxHeight = $state(220);
  const options = $derived([
    ...(!query || emptyLabel.toLowerCase().includes(query.toLowerCase()) ? [''] : []),
    ...matchingTimezones(zones, query),
  ]);
  $effect(() => { if (!open) draft = value || emptyLabel; });
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
  }
  function position() {
    if (!input || !open) return;
    const rect = input.getBoundingClientRect();
    left = rect.left; width = rect.width;
    const below = window.innerHeight - rect.bottom - 12;
    maxHeight = Math.min(220, Math.max(below, rect.top - 12));
    top = below >= Math.min(220, options.length * 30 + 8) ? rect.bottom + 4 : Math.max(8, rect.top - maxHeight - 4);
  }
  async function begin() {
    if (disabled) return;
    open = true; query = ''; active = Math.max(0, options.indexOf(value));
    input.scrollIntoView({ block: 'nearest' });
    position();
    await tick(); document.getElementById(`${id}-option-${active}`)?.scrollIntoView({ block: 'nearest' });
  }
  function close() { open = false; query = ''; draft = value || emptyLabel; }
  function choose(zone: string) { value = zone; close(); }
  async function keydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) { e.preventDefault(); e.stopPropagation(); close(); return; }
    if (e.key === 'Tab') { close(); return; }
    if (e.key === 'Enter' && open) {
      e.preventDefault(); if (options[active] !== undefined) choose(options[active]); return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    if (!open) begin();
    else active = Math.max(0, Math.min(options.length - 1, active + (e.key === 'ArrowDown' ? 1 : -1)));
    await tick(); document.getElementById(`${id}-option-${active}`)?.scrollIntoView({ block: 'nearest' });
  }
  onMount(() => {
    window.addEventListener('scroll', position, true);
    window.addEventListener('resize', position);
    return () => { window.removeEventListener('scroll', position, true); window.removeEventListener('resize', position); };
  });
</script>

<div class="zone-picker">
  <input bind:this={input} {id} aria-label={label} {disabled} role="combobox"
    aria-autocomplete="list" aria-expanded={open && !disabled} aria-controls="{id}-options"
    aria-activedescendant={open && options[active] !== undefined ? `${id}-option-${active}` : undefined}
    autocomplete="off" spellcheck="false" value={draft}
    onfocus={() => { begin(); input.select(); }}
    onclick={() => { if (!open) begin(); }}
    oninput={(e) => { draft = e.currentTarget.value; query = draft; open = true; active = 0; position(); }}
    onblur={close} onkeydown={keydown} />
  <button class="arrow" type="button" tabindex="-1" {disabled} aria-label="Show {label.toLowerCase()} options"
    onpointerdown={(e) => e.preventDefault()}
    onclick={() => { const wasOpen = open; input.focus(); if (wasOpen) close(); else begin(); }}>▾</button>
  {#if open && !disabled}
    <div use:portal class="options" id="{id}-options" role="listbox" aria-label="{label} options"
      style:left="{left}px" style:top="{top}px" style:width="{width}px" style:max-height="{maxHeight}px">
      {#each options as zone, i (zone)}
        <button type="button" role="option" id="{id}-option-{i}" aria-selected={value === zone}
          class:active={active === i} tabindex="-1" onpointerdown={(e) => e.preventDefault()}
          onmouseenter={() => (active = i)} onclick={() => choose(zone)}>{zone || emptyLabel}</button>
      {:else}<div class="empty" role="status">No matching time zones</div>{/each}
    </div>
  {/if}
</div>

<style>
  .zone-picker { width: 300px; max-width: 100%; min-width: 0; flex: 1; position: relative; }
  input { width: 100%; box-sizing: border-box; font: inherit; font-size: 13px; color: var(--text);
    background-color: color-mix(in srgb, var(--text) 5%, transparent);
    border: 1px solid var(--hairline); border-radius: 5px; padding: 4px 26px 4px 6px; }
  input:focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  button { font: inherit; font-size: 12px; color: var(--text); cursor: pointer; border: 0; }
  .arrow { position: absolute; right: 1px; top: 1px; bottom: 1px; width: 24px; background: transparent; color: var(--muted); }
  input:disabled, button:disabled { opacity: .5; cursor: default; }
  .options { position: fixed; z-index: 1000; overflow-y: auto; box-sizing: border-box;
    padding: 4px; border: 1px solid var(--hairline); border-radius: 5px; background: var(--surface, #25262b);
    box-shadow: 0 4px 16px #0004; }
  .options button { display: block; width: 100%; text-align: left; padding: 7px 8px;
    border-radius: 3px; background: transparent; }
  .options button.active { background: color-mix(in srgb, var(--accent) 25%, transparent); }
  .empty { padding: 8px; color: var(--muted); font-size: 12px; }
</style>
