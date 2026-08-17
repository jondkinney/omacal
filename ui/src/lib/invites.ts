import { invoke } from '@tauri-apps/api/core';

/**
 * One row of the invitation tray — the in-app answer to a missed
 * notification. A toast evaporates (it did, live, 2026-08-17); this list is
 * where the debt stays visible until it is paid.
 *
 * The shape arrives ready to render. In particular `start_date`/`end_date`
 * are the all-day days in the **calendar's** zone, worked out on the Rust
 * side — never derive a day from `start_ms` here; `EventDetail.start_date`
 * documents the trap (an all-day instant is a foreign-zone midnight, and
 * this browser reads it as the neighbouring day).
 */
export type PendingInvite = {
  id: number;
  title: string | null;
  start_ms: number;
  end_ms: number;
  is_all_day: boolean;
  /** First and last day covered (`yyyy-mm-dd`, last day inclusive) for an
   *  all-day invite; `null` for a timed one. */
  start_date: string | null;
  end_date: string | null;
  organizer_email: string | null;
  color: string | null;
  /** Whether Yes/Maybe/No can actually be sent — the popover's own gate,
   *  decided on the Rust side. A CalDAV invitation lists without buttons. */
  can_respond: boolean;
};

export const pendingInvites = () => invoke<PendingInvite[]>('pending_invites');
