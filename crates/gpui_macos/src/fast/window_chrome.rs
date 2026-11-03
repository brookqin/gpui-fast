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

/// Preserve the window's actual backing until fullscreen has fully ended.
/// The toolbar and standard buttons remain owned and animated by AppKit.
#[derive(Default)]
pub(crate) struct NativeTitlebar {
    backdrop: Option<OpaqueBackdrop>,
}

struct OpaqueBackdrop {
    color: objc2::rc::Retained<objc2_app_kit::NSColor>,
    opaque: bool,
    blur: Option<(objc2::rc::Retained<objc2_app_kit::NSView>, bool)>,
}

fn owns_titlebar(window: &objc2_app_kit::NSWindow) -> bool {
    window.toolbar().is_some_and(|toolbar| {
        &*toolbar.identifier() == objc2_foundation::ns_string!("gpui-native-unified-titlebar")
    })
}

/// Called before application-triggered toggleFullScreen:, before AppKit can
/// take its transition snapshot. The will-enter/exit callbacks cover system
/// controls too. Repeated callbacks must not overwrite the saved backdrop.
pub(crate) fn prepare_fullscreen_toggle(window: *mut objc2::runtime::AnyObject) {
    // SAFETY: Called on the main thread after the existing closed-window guard.
    let state = unsafe { crate::window::get_window_state(&*window) };
    suspend_blur(&mut state.lock());
    CATransaction::flush();
}

pub(crate) fn suspend_blur(state: &mut crate::window::MacWindowState) {
    use objc2_app_kit::{NSColor, NSView, NSWindow};
    // SAFETY: Lifecycle hooks run on the main thread with this live window.
    let window = unsafe { &*state.native_window.cast::<NSWindow>() };
    if !owns_titlebar(window)
        || state.background_appearance != gpui::WindowBackgroundAppearance::Blurred
    {
        return;
    }
    let _layout = ImmediateLayout::new();
    if state.fast_chrome.backdrop.is_none() {
        let blur = state
            .blurred_view
            .and_then(|view| {
                // SAFETY: The view is installed in the live window. Retain it until
                // restoration, even if a caller replaces the background meanwhile.
                unsafe { objc2::rc::Retained::retain(view.cast::<NSView>()) }
            })
            .map(|view| {
                let hidden = view.isHidden();
                (view, hidden)
            });
        state.fast_chrome.backdrop = Some(OpaqueBackdrop {
            color: window.backgroundColor(),
            opaque: window.isOpaque(),
            blur,
        });
    }
    if let Some((view, _)) = state
        .fast_chrome
        .backdrop
        .as_ref()
        .and_then(|saved| saved.blur.as_ref())
    {
        view.setHidden(true);
    }
    // Keep the GPU surface's alpha blending: it now composites onto a solid
    // native backing instead of a live behind-window visual effect. Marking
    // the Metal layer opaque would turn its transparent clear pixels black.
    window.setBackgroundColor(Some(
        &NSColor::windowBackgroundColor().colorWithAlphaComponent(1.),
    ));
    window.setOpaque(true);
    // This flag suppresses the *extra* native toolbar background; the backing
    // below GPUI is opaque, so no desktop translucency remains.
    window.setTitlebarAppearsTransparent(true);
}

pub(crate) fn restore_blur(state: &mut crate::window::MacWindowState) {
    let Some(saved) = state.fast_chrome.backdrop.take() else {
        return;
    };
    let _layout = ImmediateLayout::new();
    // SAFETY: The completion/failure callback holds the live window state.
    let window = unsafe { &*state.native_window.cast::<objc2_app_kit::NSWindow>() };
    if state.background_appearance == gpui::WindowBackgroundAppearance::Blurred {
        window.setBackgroundColor(Some(&saved.color));
        window.setOpaque(saved.opaque);
        window.setTitlebarAppearsTransparent(state.transparent_titlebar);
        if let Some((view, hidden)) = saved.blur {
            view.setHidden(hidden);
        }
    }
}

extern "C" fn did_enter(
    this: *mut objc2::runtime::AnyObject,
    _: objc2::runtime::Sel,
    _: *mut objc2::runtime::AnyObject,
) {
    // SAFETY: This is the registered NSWindow delegate callback.
    let state = unsafe { crate::window::get_window_state(&*this) };
    suspend_blur(&mut state.lock());
}

extern "C" fn failed_to_enter(
    this: *mut objc2::runtime::AnyObject,
    _: objc2::runtime::Sel,
    _: *mut objc2::runtime::AnyObject,
) {
    // SAFETY: This is the registered NSWindow delegate callback.
    let state = unsafe { crate::window::get_window_state(&*this) };
    restore_blur(&mut state.lock());
}
