/**
 * The box `App.svelte` leaves for whichever view is on screen.
 *
 * Why this file exists at all. The four views (`WeekGrid`, `MonthGrid`,
 * `YearGrid`, `BigYearRibbon`) no longer carry a height of their own — each is
 * a `flex: 1` item that claims whatever `main` has left after `Header`. That is
 * the fix, and in the app it needs no number from anywhere. But every one of
 * them is *also* mounted standalone by `mount.svelte.ts`, where there is no
 * `App` and no `main`: `#app` is a bare `<div>` whose height is whatever its
 * contents come to. `flex: 1` against a parent like that does nothing at all —
 * `BigYearRibbon` would size to its own fourteen rows and then report, per
 * `.rows`, that its contents fit inside it exactly, which is the shape of an
 * assertion that cannot fail. So the harness has to supply the box the app
 * would have.
 *
 * `APP_CHROME_PX` is the whole of what `App` puts above and below a view, and
 * it is **measured, not budgeted**: `main`'s 14px of padding at each end, plus
 * `Header`'s own 23px, plus the 12px `header { margin-bottom }` in
 * `Header.svelte`. 14 + 23 + 12 + 14 = 63.
 *
 * It is the one chrome constant left anywhere in this repo, and it is here
 * rather than in `ui/src` deliberately: the app derives this number at layout
 * time and never states it, so nothing in `ui/src` can go stale. This copy can,
 * which is why `app.spec.ts` pins it — see "a standalone view gets the same box
 * the app gives it". If `Header` grows a row, that spec fails and this number
 * is what to change.
 */
export const APP_CHROME_PX = 63;

/** `APP_CHROME_PX` as the height a standalone view should be mounted at. */
export const VIEW_BOX_CSS = `calc(100vh - ${APP_CHROME_PX}px)`;
