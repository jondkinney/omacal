//! The menu bar's graphical popup. Data stays in the existing calendar feed;
//! this window owns no calendar store, authentication, or sync loop.
use tauri::{AppHandle, Manager};

pub(crate) fn toggle(app: &AppHandle, rect: tauri::Rect) -> tauri::Result<()> {
    let window = match app.get_webview_window("menubar") {
        Some(w) => w,
        None => tauri::WebviewWindowBuilder::new(app, "menubar",
                tauri::WebviewUrl::App("index.html?menubar".into()))
            .title("OmaCal agenda").inner_size(420.0, 600.0)
            .decorations(false).resizable(false).always_on_top(true)
            .skip_taskbar(true).visible(false).build()?,
    };
    if window.is_visible()? { return window.hide(); }
    let scale = window.scale_factor()?;
    let point = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    let monitor = window.monitor_from_point(point.x, point.y)?;
    let scale = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(scale);
    let width = 420.0 * scale;
    let mut x = point.x + size.width - width;
    let mut y = point.y + size.height;
    if let Some(m) = monitor {
        let area = m.work_area();
        x = x.max(area.position.x as f64)
            .min((area.position.x as f64 + area.size.width as f64 - width).max(area.position.x as f64));
        y = y.max(area.position.y as f64);
        let height = (area.position.y as f64 + area.size.height as f64 - y).min(m.size().height as f64 * 0.8).max(200.0 * scale);
        window.set_size(tauri::PhysicalSize::new(width, height))?;
    }
    window.set_position(tauri::PhysicalPosition::new(x, y))?;
    window.show()?;
    window.set_focus()
}

#[tauri::command]
pub async fn menubar_feed(state: tauri::State<'_, crate::AppState>) -> Result<crate::upcoming::Feed, String> {
    crate::upcoming::current(&state.pool, crate::now_ms()).await.map_err(|e| crate::errors::user_facing(&e))
}

pub(crate) fn joinable(feed: &crate::upcoming::Feed, now: i64) -> Option<&crate::upcoming::FeedEvent> {
    let early = i64::from(feed.panel.as_ref().map(|p| p.join_minutes).unwrap_or(5)) * 60_000;
    feed.events.iter().filter(|e| !e.all_day && e.start_ms <= now + early && e.end_ms > now
        && e.conference.as_deref().is_some_and(|url| url.starts_with("https://") || url.starts_with("http://")))
        .min_by_key(|e| (e.start_ms <= now, if e.start_ms > now { i128::from(e.start_ms) } else { -i128::from(e.start_ms) }))
}

#[tauri::command]
pub async fn menubar_action(app: AppHandle, state: tauri::State<'_, crate::AppState>, action: String) -> Result<(), String> {
    match action.as_str() {
        "close" => {},
        "open" => crate::tray::show_main_window(&app),
        "preferences" => crate::tray::preferences(&app),
        "quick-add" => crate::tray::quick_add(&app),
        "sync" => { crate::sync_loop::request_now(&app); return Ok(()); },
        "quit" => app.exit(0),
        "join" => {
            let now = crate::now_ms();
            let feed = crate::upcoming::current(&state.pool, now).await.map_err(|e| e.to_string())?;
            let event = joinable(&feed, now).ok_or("No meeting is ready to join.")?;
            crate::browser::open_external(event.conference.as_deref().unwrap()).map_err(|e| e.to_string())?;
        },
        date => match crate::tray::action_for(&format!("at:{date}")) {
            Some(crate::tray::TrayAction::OpenAt(date)) => crate::tray::open_at(&app, &date),
            _ => return Err("Unknown menu bar action.".into()),
        },
    }
    if let Some(w) = app.get_webview_window("menubar") { let _ = w.hide(); }
    Ok(())
}
