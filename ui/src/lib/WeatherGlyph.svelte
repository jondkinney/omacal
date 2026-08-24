<!-- ui/src/lib/WeatherGlyph.svelte -->
<script lang="ts">
  /** One sky, one glyph. The eight buckets are `weather::bucket_for_code`'s
   *  — the Omarchy widget's own grouping — drawn in the app's thin-line
   *  icon style (the filmstrip's repeat/people/camera family) rather than
   *  the widget's Nerd Font glyphs, which a webview off Omarchy cannot be
   *  trusted to have. `currentColor` throughout, so the host decides the
   *  ink the way it does for every other meta icon. */
  let { bucket, size = 14 }: { bucket: string; size?: number } = $props();

  const PATHS: Record<string, string> = {
    clear:
      'M8 5.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6ZM8 1.6v1.6M8 12.8v1.6M1.6 8h1.6M12.8 8h1.6M3.5 3.5l1.1 1.1M11.4 11.4l1.1 1.1M12.5 3.5l-1.1 1.1M4.6 11.4l-1.1 1.1',
    partly:
      'M5.4 5.5a2.3 2.3 0 0 1 4-1.5M7.4 2v1M11.5 4.1l.7-.7M11 6h1M3.9 3.4l.7.7M10.9 12.5H5a2.3 2.3 0 0 1-.3-4.58 3.1 3.1 0 0 1 6-.5 2.15 2.15 0 0 1 .2 5.08Z',
    overcast:
      'M11.4 12H4.6a2.6 2.6 0 0 1-.3-5.18 3.5 3.5 0 0 1 6.8-.6 2.35 2.35 0 0 1 .3 5.78Z',
    fog:
      'M11.4 8.5H4.6a2.4 2.4 0 0 1-.3-4.78 3.3 3.3 0 0 1 6.4-.55 2.2 2.2 0 0 1 .7 5.33ZM3 11h10M4.5 13.5h7',
    drizzle:
      'M11.4 9H4.6a2.4 2.4 0 0 1-.3-4.78 3.3 3.3 0 0 1 6.4-.55A2.2 2.2 0 0 1 11.4 9ZM6 11l-.6 1.6M9.4 11l-.6 1.6',
    rain:
      'M11.4 9H4.6a2.4 2.4 0 0 1-.3-4.78 3.3 3.3 0 0 1 6.4-.55A2.2 2.2 0 0 1 11.4 9ZM5.2 10.8l-.7 2M8 10.8l-.7 2M10.8 10.8l-.7 2',
    snow:
      'M11.4 9H4.6a2.4 2.4 0 0 1-.3-4.78 3.3 3.3 0 0 1 6.4-.55A2.2 2.2 0 0 1 11.4 9ZM5.5 11v2M4.63 11.5l1.74 1M6.37 11.5l-1.74 1M10.1 11v2M9.23 11.5l1.74 1M10.97 11.5l-1.74 1',
    thunder:
      'M11.6 8.4H4.8a2.3 2.3 0 0 1-.3-4.58 3.2 3.2 0 0 1 6.2-.5 2.15 2.15 0 0 1 .9 5.08ZM8.6 8.6l-2 3h2l-1.4 2.8',
  };
  const d = $derived(PATHS[bucket] ?? PATHS.overcast);
</script>

<svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true">
  <path fill="none" stroke="currentColor" stroke-width="1.3"
        stroke-linecap="round" stroke-linejoin="round" {d} />
</svg>
