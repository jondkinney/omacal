/** The five slots the plan settles on (spec §10).
 *
 * In a plain module rather than in `ViewSwitcher.svelte`, where it lived until
 * `shortcuts.ts` needed it: `tsconfig.test.json` compiles `src/**\/*.ts` and no
 * `.svelte` at all, so a pure module importing a type from a component fails
 * the `tsc` half of `npm run check` while passing the `svelte-check` half.
 * `ViewSwitcher` re-exports it, so every existing import site is untouched. */
export type View = 'day' | 'week' | 'month' | 'year' | 'bigyear';
