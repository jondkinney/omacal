import { invoke } from '@tauri-apps/api/core';

export type Palette = {
  bg: string; surface: string; text: string;
  muted: string; accent: string; is_dark: boolean;
};

/** Pushes the resolved palette onto :root so all styling flows from CSS vars. */
export async function applyPalette(): Promise<Palette> {
  const p = await invoke<Palette>('get_palette');
  const r = document.documentElement.style;
  r.setProperty('--bg', p.bg);
  r.setProperty('--surface', p.surface);
  r.setProperty('--text', p.text);
  r.setProperty('--muted', p.muted);
  r.setProperty('--accent', p.accent);
  r.setProperty('--hairline', p.is_dark ? 'rgba(255,255,255,.055)' : 'rgba(0,0,0,.07)');
  r.setProperty('--hour-rule', p.is_dark ? 'rgba(255,255,255,.035)' : 'rgba(0,0,0,.05)');
  r.setProperty('--today-tint', p.is_dark ? 'rgba(255,255,255,.028)' : 'rgba(0,0,0,.025)');
  return p;
}
