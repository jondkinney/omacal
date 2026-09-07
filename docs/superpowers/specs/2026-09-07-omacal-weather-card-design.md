# The weather card — design (2026-09-07)

The sky in a day header has been a label since the forecast arrived: a glyph
and the day's high, from Open-Meteo, for a place resolved the way the
Omarchy bar resolves its own. This record makes it a button, and says what
the card behind it must always do.

## The rule that comes first

**The card names the place the forecast is for, and how that place was
decided, before any number.** The place may not be where the user is: with
no city set in the bar it is guessed from the connection's IP, which can be
a city off and, through a VPN, a country off. A forecast for the wrong place
with no way to tell is worse than none. So the report carries `source`
(`configured` for the bar's weather setting, `detected` for the IP guess,
`demo`), the card prints the place in capitals with a line underneath in
plain words ("from your connection's location, which may be a city off" /
"set in the bar's weather panel"), and a cache from before this record,
which has no source, reads as detected — the honest reading of a place
nobody chose. The CLI's `omacal weather` leads with the same line, and the
skill tells agents to name the place when they answer about weather.

A detected place is now named the way the bar names it: wttr.in's own
location line (`?format=%l`, "Gurugram") rather than its JSON area name
("Gurgaon"), so the two surfaces agree.

## What the card shows

One request to Open-Meteo, the same keyless call as before with more fields:
the daily code, high and low the headers always drew, plus chance of rain,
strongest wind, sunrise and sunset per day, and a `current` block. The cache
keeps its shape with the new fields optional, so an older cache still draws
the headers and makes a card that says what it can.

- **Today**: the glyph and the temperature *now*, the place and its source,
  "Now, as of HH:MM" (a reading can be a refresh interval old), then feels
  like, wind and humidity, the day's high and low, sunrise and sunset.
- **Any other day**: the glyph and the high, the place and its source, the
  day's name, then low, chance of rain and wind, sunrise and sunset.
- Units follow the temperature setting: km/h beside Celsius, mph beside
  Fahrenheit, each rounded once at display from the stored Celsius and km/h.
- Weather stays decoration: nothing here is an error surface. A field the
  forecast lacks is a line the card lacks.

## Mechanics

- The header's `.wx` span is a `<button>` styled as the label it was, in the
  Week grid and the Filmstrip; it hands `App` the day and its own rect.
- `App` keeps the whole report beside the by-date map and mounts one
  `WeatherPopover` per click, positioned by `placePopover`, focused on mount,
  closed by Escape, the scrim, or another click.
- `omacal weather [--json]` prints the cached report: place and source first,
  then now, then the days.

## Tests

The Open-Meteo parser with the current block and the extras, an old cache
still reading, wttr.in's place line; the CLI's text in both units; the card
in four fixtures (today, a later day, a configured place, a bare cache) with
a golden for today; the app-level click, Escape and unit; and the mutation
that removes the source line reddens every spec that guards it.
