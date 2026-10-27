// Modified by Longbridge for gpui-fast.
//! Keep browser composition keys out of normal key bindings and text insertion.

/// The first IME keydown can precede compositionstart, and the final keydown
/// can follow compositionend. Both may have isComposing=false but keyCode=229.
/// Check before dispatching bindings: Enter confirms an IME candidate and must
/// not also submit an input. The caller leaves the browser default untouched
/// so composition startup, navigation and confirmation can proceed normally.
/// Composition/input events continue to update the platform input handler.
pub(crate) fn owns_key_down(active: bool, event_composing: bool, key_code: u32) -> bool {
    active || event_composing || key_code == 229
}

#[cfg(test)]
mod tests {
    use super::owns_key_down;

    #[test]
    fn composition_boundary_keys_remain_owned_by_the_browser() {
        // First Latin-looking key before compositionstart; final key after end.
        assert!(owns_key_down(false, false, 229));
        // Enter/Backspace used by candidate selection, with either source of state.
        assert!(owns_key_down(true, false, 13));
        assert!(owns_key_down(false, true, 8));
        assert!(owns_key_down(true, true, 27));
        // Normal typing and submission resume immediately after composition.
        assert!(!owns_key_down(false, false, 78));
        assert!(!owns_key_down(false, false, 13));
        assert!(!owns_key_down(false, false, 8));
    }
}
