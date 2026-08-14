//! Sync on resume from suspend.
//!
//! The ticker cannot do this on its own: tokio's clock is CLOCK_MONOTONIC,
//! which freezes while the machine sleeps, so an overnight lid-close leaves
//! the next tick as far away as it was at midnight — and the calendar stale
//! until something else (a window focus) happens to nudge it. On a laptop
//! distro that is the *normal* morning, not an edge case.
//!
//! logind announces both edges of a suspend on the system bus
//! (`org.freedesktop.login1.Manager.PrepareForSleep`: `true` going down,
//! `false` coming back). The resume edge routes into
//! [`crate::sync_loop::request_now`], deliberately the *focus* path rather
//! than a raw sync: it already carries the demo gate and the 30-second
//! floor, so a machine that catnaps in and out of suspend cannot be made to
//! hammer Google by its own power management.
//!
//! Split as ever: the one decision — which signals mean "act" — is pure and
//! tested; the bus subscription is OS integration and is the untested half,
//! like `tray::build` and `update::spawn`.

/// Whether a `PrepareForSleep` body is the resume edge.
///
/// Only a well-formed `false` acts. `true` is the machine going down —
/// nothing to do, and no network to be trusted anyway. `None` is a body
/// that failed to deserialize, and it reads as sleep on purpose: the
/// signal's content is whatever the bus delivered, and a malformed message
/// must never be able to conjure network traffic.
///
/// Not `#[cfg(target_os = "linux")]`, though its only caller is: gating the
/// function would gate its test with it, and this is the pure half — the one
/// piece of this module that *can* be checked on a Mac. Marked instead as
/// allowed to be unused off Linux, which is what makes `cargo clippy
/// --workspace -- -D warnings` — the CI gate — runnable on the machine most
/// of this app is written on.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn is_resume(body: Option<bool>) -> bool {
    body == Some(false)
}

/// Starts the suspend watcher. Linux only — logind is the thing being
/// listened to — and never fatal: a box without a system bus (a container,
/// a stripped session) just goes without resume syncs, exactly as it did
/// before this module existed.
#[cfg(target_os = "linux")]
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = watch(app).await {
            tracing::warn!(%e, "suspend watcher unavailable; sync-on-resume disabled");
        }
    });
}

#[cfg(target_os = "linux")]
async fn watch(app: tauri::AppHandle) -> anyhow::Result<()> {
    use futures_util::StreamExt;

    let conn = zbus::Connection::system().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;

    let mut signals = proxy.receive_signal("PrepareForSleep").await?;
    while let Some(msg) = signals.next().await {
        let body = msg.body().deserialize::<bool>().ok();
        if is_resume(body) {
            tracing::info!("resumed from suspend; nudging sync");
            crate::sync_loop::request_now(&app);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one decision, both edges and the failure case. `None` staying
    /// quiet is the load-bearing half: it is what keeps a malformed bus
    /// message from turning into a Google request.
    #[test]
    fn only_a_well_formed_resume_edge_acts() {
        assert!(is_resume(Some(false)), "waking up is the whole point");
        assert!(!is_resume(Some(true)), "going to sleep is not a reason to sync");
        assert!(!is_resume(None), "a malformed signal must read as sleep, never resume");
    }
}
