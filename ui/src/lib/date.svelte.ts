import type { DateFormat } from './datefmt';
const state = $state<{ format: DateFormat }>({ format: 'locale' });
export const dateFormat = () => state.format;
export const setDateFormat = (format: DateFormat) => { state.format = format ?? 'locale'; };
