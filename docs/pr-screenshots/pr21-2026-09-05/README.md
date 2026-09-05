# Omacal PR #21 screenshots

Native Tauri screenshots captured on September 5, 2026 for [PR #21](https://github.com/x3me/omacal/pull/21). All calendar data comes from the built-in synthetic demo account. Events are positioned over the bright sky/window in Nord’s City View wallpaper.

Comparison panels use unscaled, unretouched crops of native screenshots, with captions outside the captured pixels. Open images at full size to assess text legibility.

| Image | Comparison |
| --- | --- |
| [Historical contrast fix](comparisons/05-contrast-fix-before-after.png) | Immediately before and after PR #2; compositor opacity 98.5% |
| [Near-stock settings](comparisons/04-near-stock-comparison.png) | Current main at 98.5% and 100% compositor opacity; PR #21 at 4% background / 0% event transparency with compositor opacity 100% |
| [Full desktop context](raw/04-background35-events0-rounded.png) | PR #21: 35% background / 0% event transparency; compositor opacity 100% |
| [Event slider endpoints](comparisons/01-event-transparency.png) | Background transparency fixed at 35%; events 100% versus 0% transparent; compositor opacity 100% |
| [Rounded and square](comparisons/02-rounded-versus-square.png) | Background transparency 35%, event transparency 0%, compositor opacity 100% |
| [Background slider endpoints](comparisons/03-background-transparency.png) | Background transparency 0% versus 100%; events fixed at 0%; compositor opacity 100% |
| [Settings controls](raw/12-settings-background35-events0.png) | Both sliders visible at 35% background / 0% event transparency |

Slider values are **transparency**: 0% is opaque and 100% is clear. Compositor values are **opacity**: 100% applies no additional whole-window fading.

Sources were built without changes: current main `21e8e3665b1b359dfda7f0094fb6b8b227364ddb`, appearance PR `aefa01c2152c562b694a05346d799865155057fe`, before PR #2 `961015fc17df87431a56169b7a2b8f760cea6e56`, and after PR #2 `36f944dffe248346a1e09ff30d79af05104f217a`. See [builds.json](builds.json).

The stock-focused versus fully opaque main comparison is subtle. Clear event fills are an optional PR state, not a stock-main defect. A 4% app background setting corresponds to the stock unfocused baseline; it does not reproduce 1.5% focused transparency or focus-dependent fading. Current main has a shorter header than the PR build, causing a small vertical grid offset in that comparison.

The same 70×45-pixel interior region of the “Ops review” event is pixel-identical at 0%, 35%, and 100% background transparency when event transparency is 0%. The surrounding calendar pixel changes while the event fill remains RGB (49, 58, 76). See [pixel-check.json](pixel-check.json).
