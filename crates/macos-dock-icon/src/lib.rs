//! Sets the macOS Dock icon at runtime.
//!
//! A bundled `.app` gets its Dock icon from `icon.icns`, but `cargo tauri dev`
//! runs the bare binary and macOS shows a generic executable icon. This crate
//! fixes that with the one `AppKit` call that requires `unsafe` — isolated here
//! so the rest of the workspace can keep `unsafe_code = "forbid"`. This crate
//! denies `unsafe` too, and the one block lifts that with a stated reason.

/// Set the Dock icon from PNG bytes. No-op off the main thread or if the
/// image fails to decode; never fails the caller over a cosmetic.
#[cfg(target_os = "macos")]
pub fn set_dock_icon(png_bytes: &[u8]) {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };

    let data = NSData::with_bytes(png_bytes);

    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };

    let app = NSApplication::sharedApplication(main_thread);

    #[expect(
        unsafe_code,
        reason = "the binding is `unsafe` only because the setter may reject \
                  `None`, and this call always passes an image"
    )]
    // SAFETY: the binding states one precondition (objc2-app-kit 0.3.2,
    // `NSApplication::setApplicationIconImage`, "# Safety"): the argument
    // "might not allow `None`". This call passes `Some(&image)`, a live
    // `NSImage` that `initWithData` returned above, so `None` never reaches
    // the setter. The main-thread requirement is not part of this block's
    // argument: `sharedApplication` takes the `MainThreadMarker` obtained
    // above, so `app` exists only on the main thread.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

/// Non-macOS build: the Dock does not exist; nothing to do.
#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon(_png_bytes: &[u8]) {}
