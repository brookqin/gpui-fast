//! UI-thread video ownership and frame demand, independent of GPU resources.
use gpui::DynamicSurfaceId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

/// Forced paths never silently fall back. Auto attempts import then copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserSurfaceMode {
    Copy,
    External,
    Auto,
}

/// Counters describe actual GPU operations, not decoder progress alone.
#[derive(Clone, Debug, Default)]
pub struct BrowserSurfaceStats {
    pub decoded_frames: u64,
    pub draws: u64,
    pub copies: u64,
    pub imports: u64,
    pub failures: u64,
    pub last_error: Option<String>,
    pub actual_mode: Option<BrowserSurfaceMode>,
    /// The most recent draw could not provide pixels; successful fallback clears it.
    pub unavailable: bool,
    /// Device loss is terminal for this renderer; the host must recover or fall back.
    pub device_lost: bool,
}

/// The last owner cancels callbacks and unregisters the source generation.
/// Keep the video alive here even if it has no DOM presentation.
#[derive(Clone)]
pub struct BrowserSurface(pub(crate) Rc<Source>);

pub(crate) struct Source {
    pub(crate) id: DynamicSurfaceId,
    pub(crate) video: web_sys::HtmlVideoElement,
    pub(crate) mode: BrowserSurfaceMode,
    pub(crate) serial: Cell<u64>,
    pub(crate) stats: RefCell<BrowserSurfaceStats>,
    wake: Rc<dyn Fn()>,
    callback: RefCell<Option<Closure<dyn FnMut(f64)>>>,
    pending: Cell<Option<i32>>,
    video_callback: bool,
    events: RefCell<Vec<(&'static str, Closure<dyn FnMut(web_sys::Event)>)>>,
    previous_time: Cell<f64>,
}

thread_local! {
    static SOURCES: RefCell<HashMap<DynamicSurfaceId, Weak<Source>>> = RefCell::new(HashMap::new());
}

pub(crate) fn lookup(id: DynamicSurfaceId) -> Option<Rc<Source>> {
    SOURCES.with(|sources| sources.borrow().get(&id).and_then(Weak::upgrade))
}

impl BrowserSurface {
    /// The callback should request presentation of the owning GPUI window.
    pub fn new(
        video: web_sys::HtmlVideoElement,
        mode: BrowserSurfaceMode,
        wake: Rc<dyn Fn()>,
    ) -> Self {
        let video_callback = js_sys::Reflect::get(&video, &"requestVideoFrameCallback".into())
            .is_ok_and(|value| value.is_function());
        let source = Rc::new(Source {
            id: DynamicSurfaceId::new(),
            video,
            mode,
            serial: Cell::new(1),
            stats: RefCell::new(BrowserSurfaceStats::default()),
            wake,
            callback: RefCell::new(None),
            pending: Cell::new(None),
            video_callback,
            events: RefCell::new(Vec::new()),
            previous_time: Cell::new(-1.),
        });
        SOURCES.with(|sources| {
            sources
                .borrow_mut()
                .insert(source.id, Rc::downgrade(&source))
        });
        let weak = Rc::downgrade(&source);
        *source.callback.borrow_mut() = Some(Closure::new(move |_: f64| {
            if let Some(source) = weak.upgrade() {
                source.pending.set(None);
                let time = source.video.current_time();
                if source.video.ready_state() >= 2
                    && (source.video_callback || source.previous_time.get() != time)
                {
                    source.previous_time.set(time);
                    source.stats.borrow_mut().decoded_frames += 1;
                    source.changed();
                }
                if source.video_callback || !source.video.paused() {
                    source.schedule();
                }
            }
        }));
        for name in [
            "loadeddata",
            "seeked",
            "pause",
            "resize",
            "ended",
            "playing",
        ] {
            let weak = Rc::downgrade(&source);
            let callback = Closure::new(move |_: web_sys::Event| {
                if let Some(source) = weak.upgrade() {
                    source.changed();
                    source.schedule();
                }
            });
            if source
                .video
                .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
                .is_ok()
            {
                source.events.borrow_mut().push((name, callback));
            }
        }
        source.schedule();
        Self(source)
    }

    /// Identity to paint in the GPUI scene.
    pub fn id(&self) -> DynamicSurfaceId {
        self.0.id
    }
    /// Snapshot the actual rendering path and failures.
    pub fn stats(&self) -> BrowserSurfaceStats {
        self.0.stats.borrow().clone()
    }
    /// Request a new frame after an application-controlled source change.
    pub fn invalidate(&self) {
        self.0.changed();
    }
}

impl Source {
    pub(crate) fn changed(&self) {
        self.serial.set(self.serial.get().wrapping_add(1));
        (self.wake)();
    }

    fn schedule(&self) {
        if self.pending.get().is_some() {
            return;
        }
        let callback = self.callback.borrow();
        let Some(callback) = callback.as_ref() else {
            return;
        };
        let result = if self.video_callback {
            js_sys::Reflect::get(&self.video, &"requestVideoFrameCallback".into())
                .and_then(|f| {
                    f.unchecked_into::<js_sys::Function>()
                        .call1(&self.video, callback.as_ref())
                })
                .map(|id| id.as_f64().unwrap_or_default() as i32)
        } else {
            web_sys::window()
                .ok_or_else(|| JsValue::from_str("No browser window"))
                .and_then(|window| {
                    window.request_animation_frame(callback.as_ref().unchecked_ref())
                })
        };
        match result {
            Ok(id) => self.pending.set(Some(id)),
            Err(error) => self.failed(&format!("Frame callback failed: {error:?}")),
        }
    }

    pub(crate) fn failed(&self, error: &str) {
        let mut stats = self.stats.borrow_mut();
        stats.failures += 1;
        stats.last_error = Some(error.to_owned());
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        SOURCES.with(|sources| sources.borrow_mut().remove(&self.id));
        (self.wake)();
        if let Some(id) = self.pending.take() {
            if self.video_callback {
                if let Ok(function) =
                    js_sys::Reflect::get(&self.video, &"cancelVideoFrameCallback".into())
                {
                    if let Some(function) = function.dyn_ref::<js_sys::Function>() {
                        let _ = function.call1(&self.video, &JsValue::from_f64(id as f64));
                    }
                }
            } else if let Some(window) = web_sys::window() {
                let _ = window.cancel_animation_frame(id);
            }
        }
        for (name, callback) in self.events.get_mut().drain(..) {
            let _ = self
                .video
                .remove_event_listener_with_callback(name, callback.as_ref().unchecked_ref());
        }
    }
}
