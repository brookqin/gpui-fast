# Browser dynamic surfaces

`Window::paint_dynamic_surface` inserts a renderer-owned source identity into the
ordered GPUI scene. The scene holds no JavaScript objects. `BrowserSurface` owns
an HTML video on the UI thread; dropping its final owner cancels callbacks,
unregisters the identity and requests presentation. Replacement uses a fresh
identity, so an old retained paint cannot acquire another producer's pixels.

`requestVideoFrameCallback` advances a source serial and requests presentation of
the existing scene. An animation-frame fallback checks decoded media time. No
callback imports GPUExternalTexture or invalidates an application's layout.

Copy and External share geometry, masks, rounded clip stack, render target,
shader structure, frame demand and controls. Copy updates a reusable RGBA texture
only when the source serial or dimensions change. External imports on the current
renderer device, binds `texture_external` and submits in the same synchronous
`record_frame` call. External textures and bind groups are never cached. A paused
video is imported again on every redraw. No readback or intermediate video render
target is used by External. Dynamic surfaces are lifted from scroll-layer tiles
with overlapping later primitives, preserving order and avoiding frozen pixels.

Both modes require browser WebGPU. Explicit modes report failures and never
silently switch paths. Auto can use Copy after an import exception. A host must
supply its own visible fallback for unsupported backends or device loss and must
not report that fallback as an External measurement. GPU validation errors remain
observable through the renderer's normal device error handling.

The wgpu dependency uses the upstream external-texture implementation from
`d5fde4291f8ecf1c070324a46018a7c2fce09310`, backported to 30.0.1, plus a fallible
import method so DOM exceptions can reach fallback policy. No decoder is added.

Independent Chromium 154 / Linux software-GPU checks cover changing main-canvas
pixels, pause, later-task paused redraw, seek, rectangular/rounded clipping,
ordinary GPUI occlusion, source release, same-device submission and importing-task
submission. Both forced modes pass without GPU errors. These checks establish
correctness, not a hardware performance recommendation or Safari support.

`paint_dynamic_surface_with_fit` supports Fill, Contain and Cover. Geometry is
resolved from the current decoded dimensions during rendering, so a scene may be
built before metadata arrives and remains valid when the source size changes.

Stats distinguish a recoverable import exception from an unavailable draw:
`unavailable` clears after a successful Copy fallback, while `device_lost` is
terminal for that renderer. The device observer is detached when its cache is
dropped; it does not replace the existing renderer error callback. Browser device
destruction also stops the renderer. The host may show a DOM fallback and must
request reload/recovery for the rest of the lost Canvas. Forced modes stay forced.
Independent fault injection covers import rejection, copy rejection after prior
successful draws, Auto fallback and actual GPUDevice.destroy().
