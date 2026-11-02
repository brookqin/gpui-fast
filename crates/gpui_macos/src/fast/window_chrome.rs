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
