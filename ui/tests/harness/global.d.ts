import type { Harness } from './tauri';

declare global {
  interface Window {
    /** The test control surface installed by `installTauriStub`. */
    __harness: Harness;
  }
}

export {};
