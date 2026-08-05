import { invoke } from '@tauri-apps/api/core';

export type AppStatus = {
  accounts: string[];
  last_sync_ms: number | null;
  demo: boolean;
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
