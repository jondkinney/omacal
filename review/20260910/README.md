Real captures of OmaCal's Svelte application in Chromium, using the repository IPC test harness with synthetic calendars and an otherwise empty event workspace. No generated or recreated UI.

- duplicate-draft.png: pr/event-duplication at fb54fce, the unsaved duplicate draft and open calendar picker.
- calendar-account-groups.png: pr/calendar-account-groups at ed48d7e, Google and CalDAV accounts sharing an address.
- alt-drag.png: pr/alt-drag-create at a5b63ed, an Alt-drag creation preview and live duration over synthetic events.
- visible-hours*.png: pr/visible-hours at a837540, Appearance controls and the cropped Week grid with synthetic events.
- omarchy-agenda.png: actual Panel.qml at d81b132, running in Quickshell with the installed Omarchy Ui/Commons components in an isolated headless Sway display. Only the host bar and feed/clock are test fixtures; the widget is unmodified. Captured by grim.
- menu-bar-format.png: cleanup/format at faf266a, actual Settings modal on a synthetic empty workspace.
- omarchy-day.png: actual Panel.qml and DayView.qml at 166a509, using the same isolated Quickshell/Sway host as the agenda capture. Synthetic events and a 5 AM–11 PM visible range.
- shared-rest.png and shared-hover.png: actual App/EventBlock components at 7343903 in Chromium, with two matching calendar copies and two independent meetings in a cleared synthetic workspace. Captures are clipped directly by Playwright.
