const state = $state({ start: 0, end: 24 });
export const visibleHours = () => state;
export function setVisibleHoursState(start = 0, end = 24) {
  state.start = start; state.end = end;
}
