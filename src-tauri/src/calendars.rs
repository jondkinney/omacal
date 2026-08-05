use crate::AppState;
use omacal_store::CalendarRow;

#[tauri::command]
pub async fn get_calendars(state: tauri::State<'_, AppState>) -> Result<Vec<CalendarRow>, String> {
    omacal_store::list_calendars(&state.pool)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

#[tauri::command]
pub async fn set_calendar_selected(
    state: tauri::State<'_, AppState>,
    id: i64,
    on: bool,
) -> Result<(), String> {
    omacal_store::set_selected(&state.pool, id, on)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}

/// Returns how many events were removed, so the UI can say what happened.
#[tauri::command]
pub async fn set_calendar_sync(
    state: tauri::State<'_, AppState>,
    id: i64,
    on: bool,
) -> Result<u64, String> {
    omacal_store::set_sync_enabled(&state.pool, id, on)
        .await
        .map_err(|e| crate::errors::user_facing(&e))
}
