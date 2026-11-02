// Modified by Longbridge for gpui-fast.
//! Keep titlebar geometry corrections out of AppKit's fullscreen animations.
use objc2_app_kit::NSAnimationContext;
use objc2_quartz_core::CATransaction;
use std::{marker::PhantomData, rc::Rc};

/// AppKit may call window layout inside an animated fullscreen transaction.
/// Traffic lights must immediately match the custom titlebar geometry instead
/// of independently interpolating from their native fullscreen positions.
/// Nested groups preserve the surrounding system transition on every return.
pub(crate) struct ImmediateLayout(PhantomData<Rc<()>>);

impl ImmediateLayout {
    pub(crate) fn new() -> Self {
        NSAnimationContext::beginGrouping();
        let context = NSAnimationContext::currentContext();
        context.setDuration(0.);
        context.setAllowsImplicitAnimation(false);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        Self(PhantomData)
    }
}

impl Drop for ImmediateLayout {
    fn drop(&mut self) {
        CATransaction::commit();
        NSAnimationContext::endGrouping();
    }
}

#[cfg(test)]
mod tests {
    use super::{ImmediateLayout, fullscreen_options};
    use objc2_app_kit::NSAnimationContext;
    use objc2_quartz_core::CATransaction;

    #[test]
    fn native_titlebar_does_not_cover_fullscreen_content() {
        use objc2_app_kit::NSApplicationPresentationOptions as Options;
        let proposed = Options::FullScreen | Options::AutoHideDock | Options::AutoHideMenuBar;
        assert_eq!(
            fullscreen_options(proposed, true),
            proposed | Options::AutoHideToolbar
        );
        assert_eq!(fullscreen_options(proposed, false), proposed);
        assert_eq!(fullscreen_options(Options::Default, true), Options::Default);
    }

    #[test]
    fn titlebar_layout_preserves_the_surrounding_animation() {
        NSAnimationContext::beginGrouping();
        NSAnimationContext::currentContext().setDuration(0.75);
        NSAnimationContext::currentContext().setAllowsImplicitAnimation(true);
        CATransaction::begin();
        CATransaction::setDisableActions(false);
        {
            let _layout = ImmediateLayout::new();
            assert_eq!(NSAnimationContext::currentContext().duration(), 0.);
            assert!(!NSAnimationContext::currentContext().allowsImplicitAnimation());
            assert!(CATransaction::disableActions());
        }
        assert_eq!(NSAnimationContext::currentContext().duration(), 0.75);
        assert!(NSAnimationContext::currentContext().allowsImplicitAnimation());
        assert!(!CATransaction::disableActions());
        CATransaction::commit();
        NSAnimationContext::endGrouping();
    }
}

/// Prefer AppKit's own unified toolbar when the requested geometry matches it.
/// Resizing a standard titlebar from 32 to 52 points after AppKit restores it
/// causes a visible second movement on fullscreen exit, even without implicit
/// animation. A real (empty) native toolbar makes AppKit own that geometry for
/// the whole transition. GPUI still draws and handles the toolbar content.
/// Other requested positions retain the existing custom-position path.
pub(crate) fn install_native_titlebar(
    native_window: *mut objc2::runtime::AnyObject,
    state: &Rc<parking_lot::Mutex<crate::window::MacWindowState>>,
) {
    use gpui::{point, px};
    use objc2::MainThreadOnly;
    use objc2_app_kit::{NSToolbar, NSToolbarDisplayMode, NSWindow, NSWindowToolbarStyle};
    use objc2_foundation::ns_string;

    {
        let mut state = state.lock();
        if !state.app_owns_titlebar_drag
            || state.traffic_light_position != Some(point(px(19.), px(19.)))
        {
            return;
        }
        // Stop manual repositioning before AppKit sends any layout callbacks.
        state.traffic_light_position = None;
    }
    let mtm = crate::main_thread_marker().expect("macOS windows require the main thread");
    let toolbar = NSToolbar::initWithIdentifier(
        NSToolbar::alloc(mtm),
        ns_string!("gpui-native-unified-titlebar"),
    );
    toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
    toolbar.setAllowsUserCustomization(false);
    toolbar.setAutosavesConfiguration(false);
    // SAFETY: Called by MacWindow::open with its newly created NSWindow/NSPanel,
    // on the main thread, without holding the window state lock.
    let window = unsafe { &*native_window.cast::<NSWindow>() };
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.setToolbar(Some(&toolbar));
}

/// AppKit otherwise leaves the empty native toolbar above the GPUI surface in
/// fullscreen. Only our geometry-only toolbar should follow the system menu
/// bar's reveal behavior; the application's GPUI toolbar stays in its content.
pub(crate) fn register_fullscreen_presentation(decl: &mut objc2::runtime::ClassBuilder) {
    use objc2::{runtime::AnyObject, runtime::Sel, sel};
    use objc2_app_kit::{NSApplicationPresentationOptions, NSWindow};
    unsafe {
        decl.add_method(
            sel!(windowDidEnterFullScreen:),
            did_enter as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        decl.add_method(
            sel!(windowDidFailToEnterFullScreen:),
            failed_to_enter as extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
        );
        decl.add_method(
            sel!(window:willUseFullScreenPresentationOptions:),
            fullscreen_presentation
                as extern "C" fn(
                    *mut AnyObject,
                    Sel,
                    *mut NSWindow,
                    NSApplicationPresentationOptions,
                ) -> NSApplicationPresentationOptions,
        );
    }
}

extern "C" fn fullscreen_presentation(
    _: *mut objc2::runtime::AnyObject,
    _: objc2::runtime::Sel,
    window: *mut objc2_app_kit::NSWindow,
    options: objc2_app_kit::NSApplicationPresentationOptions,
) -> objc2_app_kit::NSApplicationPresentationOptions {
    // SAFETY: AppKit supplies the live window for this delegate callback.
    let window = unsafe { &*window };
    let native_titlebar = owns_titlebar(window);
    fullscreen_options(options, native_titlebar)
}

fn fullscreen_options(
    options: objc2_app_kit::NSApplicationPresentationOptions,
    native_titlebar: bool,
) -> objc2_app_kit::NSApplicationPresentationOptions {
    use objc2_app_kit::NSApplicationPresentationOptions as Options;
    if native_titlebar && options.contains(Options::FullScreen) {
        options | Options::AutoHideToolbar
    } else {
        options
    }
}

/// Views are retained because AppKit can reparent or replace the titlebar while
/// crossing Spaces. Restore every view we hid, including the old container.
#[derive(Default)]
pub(crate) struct NativeTitlebar {
    hidden: Vec<(objc2::rc::Retained<objc2_app_kit::NSView>, bool)>,
}

fn owns_titlebar(window: &objc2_app_kit::NSWindow) -> bool {
    window.toolbar().is_some_and(|toolbar| {
        &*toolbar.identifier() == objc2_foundation::ns_string!("gpui-native-unified-titlebar")
    })
}

pub(crate) fn hide_native_titlebar(state: &mut crate::window::MacWindowState) {
    // SAFETY: Lifecycle hooks run on the main thread with this live window.
    let window = unsafe { &*state.native_window.cast::<objc2_app_kit::NSWindow>() };
    if !owns_titlebar(window) {
        return;
    }
    let _layout = ImmediateLayout::new();
    // Upstream makes titlebars opaque during fullscreen on macOS 15.3+.
    // Our empty toolbar must never paint a second background over GPUI.
    window.setTitlebarAppearsTransparent(true);
    let Some(button) = window.standardWindowButton(objc2_app_kit::NSWindowButton::CloseButton)
    else {
        return;
    };
    // Same standard-button ancestry used by GPUI's existing geometry handling;
    // no private class names or selectors. Hiding preserves the 52 px layout.
    let container = unsafe { button.superview().and_then(|view| view.superview()) };
    if let Some(container) = container {
        if !state
            .fast_chrome
            .hidden
            .iter()
            .any(|(view, _)| *view == container)
        {
            let was_hidden = container.isHidden();
            state
                .fast_chrome
                .hidden
                .push((container.clone(), was_hidden));
        }
        container.setHidden(true);
    }
}

pub(crate) fn restore_native_titlebar(state: &mut crate::window::MacWindowState) {
    let _layout = ImmediateLayout::new();
    for (view, was_hidden) in state.fast_chrome.hidden.drain(..) {
        view.setHidden(was_hidden);
    }
}

extern "C" fn did_enter(
    this: *mut objc2::runtime::AnyObject,
    _: objc2::runtime::Sel,
    _: *mut objc2::runtime::AnyObject,
) {
    // SAFETY: This is the registered NSWindow delegate callback.
    let state = unsafe { crate::window::get_window_state(&*this) };
    hide_native_titlebar(&mut state.lock());
}

extern "C" fn failed_to_enter(
    this: *mut objc2::runtime::AnyObject,
    _: objc2::runtime::Sel,
    _: *mut objc2::runtime::AnyObject,
) {
    // SAFETY: This is the registered NSWindow delegate callback.
    let state = unsafe { crate::window::get_window_state(&*this) };
    restore_native_titlebar(&mut state.lock());
}
