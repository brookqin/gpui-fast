//! Browser destruction must stop submission just like an involuntary loss.
//! Native renderer teardown still ignores its own explicit destruction.
pub(crate) fn is_terminal(reason: wgpu::DeviceLostReason) -> bool {
    cfg!(target_family = "wasm") || reason != wgpu::DeviceLostReason::Destroyed
}
