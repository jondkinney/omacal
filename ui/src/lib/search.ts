import { invoke } from '@tauri-apps/api/core';

/**
 * One search result: an **event**, and the occurrence being offered.
 *
 * Mirrors `omacal_core::search::Hit`. A recurring event appears once, resolved
 * to the occurrence nearest today — see that type for why, and why it is
 * neither the first of a window nor the series' own start.
 */
export type Hit = {
  eventId: number;
  title: string;
  startMs: number;
  endMs: number;
};

/**
 * Titles containing `query`, on calendars the user displays, nearest first.
 *
 * A lookup and nothing else (spec §7): no write path, no network, and no
 * second way to reach an event's detail — clicking a result opens the popover
 * that already owns that, with every guard it has.
 */
export const searchEvents = (query: string) => invoke<Hit[]>('search_events', { query });
