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
    use super::ImmediateLayout;
    use objc2_app_kit::NSAnimationContext;
    use objc2_quartz_core::CATransaction;

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
