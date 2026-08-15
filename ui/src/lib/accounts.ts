import { invoke } from '@tauri-apps/api/core';

/** One connected account, as `accounts::list_accounts` shapes it. */
export type Account = {
  id: number;
  email: string;
  provider: string; // 'google' | 'caldav'
};

export const listAccounts = () => invoke<Account[]>('list_accounts');

/** Signs an account out: revokes the Google grant (best-effort), clears the
 *  keyring entry, and deletes the account's local data — calendars, events,
 *  tasks. Resolves to the accounts that remain. */
export const signOut = (accountId: number) => invoke<Account[]>('sign_out', { accountId });
