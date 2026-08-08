import { invoke } from '@tauri-apps/api/core';

export type AppStatus = {
  accounts: string[];
  last_sync_ms: number | null;
  demo: boolean;
  /** True when the window's controls are drawn over the webview instead of in
   *  a strip above it, so the header has to leave room for them — macOS with
   *  `titleBarStyle: "Overlay"`, and nothing else.
   *
   *  This is the only thing `ui/src` knows about the platform it runs on, and
   *  it deliberately arrives as a *property of the window* rather than as an
   *  OS name: "leave room at the left" is what the header needs to decide, and
   *  the OS is only one of the two things that decides it (see
   *  `status::controls_overlay_content` for the other). It rides on
   *  `get_status` because that is already fetched on mount; the alternative
   *  was `@tauri-apps/plugin-os`, a dependency for one boolean. */
  overlay_titlebar: boolean;
};

export const getStatus = () => invoke<AppStatus>('get_status');
export const signIn = () => invoke<string>('sign_in');
export const syncNow = () => invoke<number>('sync_now');

/** "just now" / "4 min ago" / "2 h ago" — deliberately coarse. */
export function relativeTime(ms: number | null, now = Date.now()): string {
  if (ms === null) return 'never';
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}
