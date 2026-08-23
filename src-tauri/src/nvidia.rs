//! Keeping WebKit's DMA-BUF renderer off the Nvidia driver — and only there.
//!
//! Under the proprietary Nvidia driver, WebKitGTK's DMA-BUF renderer can fail
//! to allocate its GBM buffers, and the window comes up blank — on the machine
//! of someone who just installed the app and has no reason to suspect their
//! GPU. Exporting `WEBKIT_DISABLE_DMABUF_RENDERER=1` avoids that. Exporting
//! it *unconditionally* is the opposite bug: on a hybrid laptop the
//! compositor renders on the integrated GPU, where DMA-BUF works, and
//! disabling it there drops WebKit onto a shared-memory path that makes every
//! scroll sluggish. renCal shipped the blanket version and had to walk it
//! back (their #45 then #102); this is the scoped version, so we skip the
//! first half of that experiment.
//!
//! WebKit renders on the session's EGL display — the compositor's GPU. So
//! the variable is set only when *that* GPU is the Nvidia one, not merely
//! because the Nvidia module is loaded.
//!
//! Split like `tray`: the decision is pure and tested, the sysfs and
//! environment reads around it are the untested OS half.

use std::path::Path;

/// Mesa-backed drivers whose DMA-BUF path is known-good. One of these
/// holding the boot GPU on a hybrid machine means the compositor renders
/// there and the workaround must not fire.
const MESA_GPU_DRIVERS: [&str; 4] = ["i915", "xe", "amdgpu", "radeon"];

/// A render device as [`nvidia_backs_session`] sees it: which kernel driver
/// bound it, and whether the firmware booted on it.
struct Gpu {
    driver: String,
    boot_vga: bool,
}

/// Exports `WEBKIT_DISABLE_DMABUF_RENDERER=1` when the Nvidia GPU backs the
/// session. Must run before the builder assembles — GTK and WebKit read the
/// variable once, when they initialise, and never again.
pub(crate) fn apply_if_needed() {
    // An explicit value wins, whatever it says: WebKit reads `0` as "keep
    // the renderer on", so a user's own export doubles as the escape hatch
    // for the day this detection guesses wrong.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }

    // The proprietary driver is not even loaded — nothing to work around.
    if !Path::new("/proc/driver/nvidia/version").exists() {
        return;
    }

    if nvidia_backs_session(compositor_primary_driver(), &gpus()) {
        tracing::info!("Nvidia GPU backs the session; disabling WebKit's DMA-BUF renderer");
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    } else {
        tracing::info!(
            "Nvidia driver present but another GPU backs the session; \
             keeping WebKit's DMA-BUF renderer"
        );
    }
}

/// The decision alone, pure so every hybrid case is testable on machines
/// with none of the hardware.
///
/// `explicit_primary` is the compositor's own device list when it published
/// one: the outer `Some` means a list named a primary device, the inner
/// `Option` is that device resolved to its driver. A named-but-unresolvable
/// device reads as *not* Nvidia — keeping a working renderer is the cheap
/// mistake, disabling one is the expensive one.
fn nvidia_backs_session(explicit_primary: Option<Option<String>>, gpus: &[Gpu]) -> bool {
    if let Some(primary) = explicit_primary {
        return primary.as_deref() == Some("nvidia");
    }

    let has_mesa_gpu =
        gpus.iter().any(|g| MESA_GPU_DRIVERS.contains(&g.driver.as_str()));
    let nvidia_is_boot_vga = gpus.iter().any(|g| g.driver == "nvidia" && g.boot_vga);

    // An Nvidia-only machine, or a hybrid that boots on the Nvidia card —
    // the GPU compositors pick as primary by default. A Mesa GPU holding
    // boot_vga means the session's EGL device is the Mesa one and DMA-BUF
    // works fine there.
    !has_mesa_gpu || nvidia_is_boot_vga
}

/// The compositor's explicit device list, resolved to the primary entry's
/// driver. Hyprland's aquamarine reads `AQ_DRM_DEVICES`, wlroots reads
/// `WLR_DRM_DEVICES`; in both, the first entry is the GPU the session
/// renders on. `None` when neither variable names a device.
fn compositor_primary_driver() -> Option<Option<String>> {
    for var in ["AQ_DRM_DEVICES", "WLR_DRM_DEVICES"] {
        if let Some(devices) = std::env::var_os(var) {
            let devices = devices.to_string_lossy();
            if let Some(primary) = devices.split(':').find(|entry| !entry.is_empty()) {
                return Some(device_driver(Path::new(primary)));
            }
        }
    }
    None
}

/// The GPU entries under `/sys/class/drm` — `card0`, `card1` — skipping
/// connector entries like `card0-HDMI-A-1`.
fn gpus() -> Vec<Gpu> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix("card"))
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter_map(|entry| {
            let card = entry.path();
            Some(Gpu { driver: card_driver(&card)?, boot_vga: is_boot_vga(&card) })
        })
        .collect()
}

/// Kernel driver bound to a `/sys/class/drm` card — `nvidia`, `i915`, …
fn card_driver(card: &Path) -> Option<String> {
    let target = std::fs::read_link(card.join("device/driver")).ok()?;
    Some(target.file_name()?.to_string_lossy().into_owned())
}

/// Whether the firmware booted on this card — the GPU compositors pick as
/// their primary render device when nothing says otherwise.
fn is_boot_vga(card: &Path) -> bool {
    std::fs::read_to_string(card.join("device/boot_vga")).is_ok_and(|v| v.trim() == "1")
}

/// Driver for a DRM device path as spelled in the compositor's list —
/// `/dev/dri/card1`, or the by-path symlink Hyprland configs prefer.
fn device_driver(device: &Path) -> Option<String> {
    let resolved = std::fs::canonicalize(device).ok()?;
    card_driver(&Path::new("/sys/class/drm").join(resolved.file_name()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(driver: &str, boot_vga: bool) -> Gpu {
        Gpu { driver: driver.into(), boot_vga }
    }

    /// The desktop tower with one Nvidia card — the original blank-window
    /// report. No Mesa GPU anywhere, so the renderer must go.
    #[test]
    fn an_nvidia_only_machine_disables_the_renderer() {
        assert!(nvidia_backs_session(None, &[gpu("nvidia", true)]));
        // Even when boot_vga is unreadable: with nothing but Nvidia present
        // there is no other GPU the session could be rendering on.
        assert!(nvidia_backs_session(None, &[gpu("nvidia", false)]));
    }

    /// The common hybrid laptop: the firmware boots on the integrated GPU,
    /// the compositor renders there, DMA-BUF works — disabling it is the
    /// sluggish-UI bug, not a fix.
    #[test]
    fn a_hybrid_that_boots_on_the_integrated_gpu_keeps_the_renderer() {
        assert!(!nvidia_backs_session(
            None,
            &[gpu("i915", true), gpu("nvidia", false)]
        ));
        assert!(!nvidia_backs_session(
            None,
            &[gpu("amdgpu", true), gpu("nvidia", false)]
        ));
        // boot_vga unreadable on both: a Mesa GPU is present and nothing
        // says Nvidia is primary, so the renderer stays.
        assert!(!nvidia_backs_session(
            None,
            &[gpu("xe", false), gpu("nvidia", false)]
        ));
    }

    /// The mux-switched laptop set to discrete-only, or a desktop whose
    /// display cable is on the Nvidia card: Mesa hardware present, but the
    /// session renders on Nvidia.
    #[test]
    fn a_hybrid_that_boots_on_the_nvidia_gpu_disables_the_renderer() {
        assert!(nvidia_backs_session(
            None,
            &[gpu("i915", false), gpu("nvidia", true)]
        ));
    }

    /// A compositor that names its own device list has already decided which
    /// GPU the session renders on; the card scan's guess must lose to it, in
    /// both directions.
    #[test]
    fn the_compositors_own_device_list_outranks_the_card_scan() {
        // The scan says hybrid-on-integrated, the list says Nvidia: disable.
        assert!(nvidia_backs_session(
            Some(Some("nvidia".into())),
            &[gpu("i915", true), gpu("nvidia", false)]
        ));
        // The scan says Nvidia-boots, the list says integrated: keep.
        assert!(!nvidia_backs_session(
            Some(Some("i915".into())),
            &[gpu("i915", false), gpu("nvidia", true)]
        ));
    }

    /// A device list whose primary entry did not resolve to a driver. The
    /// honest reading is "unknown", and unknown keeps the renderer: a wrong
    /// keep is a blank window with `WEBKIT_DISABLE_DMABUF_RENDERER=1` as the
    /// documented fix, a wrong disable is an app that is quietly slow forever.
    #[test]
    fn an_unresolvable_explicit_device_keeps_the_renderer() {
        assert!(!nvidia_backs_session(Some(None), &[gpu("nvidia", true)]));
    }

    /// Sysfs yielded nothing at all, but `/proc/driver/nvidia/version` got us
    /// here — the driver is loaded and no other GPU is in evidence, which is
    /// the Nvidia-only shape.
    #[test]
    fn no_readable_cards_reads_as_nvidia_only() {
        assert!(nvidia_backs_session(None, &[]));
    }
}
