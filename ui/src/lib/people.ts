// ui/src/lib/people.ts
//
// The guest field's autocomplete corpus (2026-08-23, by request): everyone
// the user has ever met with, distilled on the Rust side from the guest
// lists already in the store. Deliberately not the People API — that would
// cost a sensitive OAuth scope and a re-verification for a set of people
// almost identical to this one. See `omacal_store::known_guests`.

import { invoke } from '@tauri-apps/api/core';

/** One person from the user's own meeting history. The backend returns the
 *  list most-met first, so position already encodes rank. */
export type KnownGuest = {
  email: string;
  display_name: string | null;
  met: number;
};

export const knownGuests = () => invoke<KnownGuest[]>('known_guests');
