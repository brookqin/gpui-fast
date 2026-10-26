// Modified by Longbridge for gpui-fast.
//! Observe renderer loss without replacing the renderer's own error handler.
use crate::fast::browser_surface::lookup;
use gpui::DynamicSurfaceId;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use wasm_bindgen::{JsValue, closure::Closure, prelude::wasm_bindgen};

#[wasm_bindgen(
    inline_js = "export function watchSurfaceDevice(device, callback) { let listener = callback; device.lost.then(info => { if (listener) listener(info.message || info.reason || 'Device lost'); }); return () => { listener = null; }; }"
)]
extern "C" {
    #[wasm_bindgen(js_name = watchSurfaceDevice)]
    fn watch_device(device: &JsValue, callback: &JsValue) -> js_sys::Function;
}

pub(crate) struct DeviceWatch {
    ids: Rc<RefCell<HashSet<DynamicSurfaceId>>>,
    disconnect: js_sys::Function,
    _callback: Closure<dyn FnMut(String)>,
}
impl DeviceWatch {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let ids = Rc::new(RefCell::new(HashSet::new()));
        let weak = Rc::downgrade(&ids);
        let callback = Closure::new(move |message: String| {
            if let Some(ids) = weak.upgrade() {
                for id in ids.borrow().iter() {
                    if let Some(source) = lookup(*id) {
                        source.failed(&format!("GPU device lost: {message}"));
                        {
                            let mut stats = source.stats.borrow_mut();
                            stats.unavailable = true;
                            stats.device_lost = true;
                        }
                        source.changed();
                    }
                }
            }
        });
        let disconnect = watch_device(device.as_webgpu().unwrap().as_ref(), callback.as_ref());
        Self {
            ids,
            disconnect,
            _callback: callback,
        }
    }
    pub(crate) fn track(&self, id: DynamicSurfaceId) {
        let mut ids = self.ids.borrow_mut();
        ids.retain(|id| lookup(*id).is_some());
        ids.insert(id);
    }
}
impl Drop for DeviceWatch {
    fn drop(&mut self) {
        // A later promise settlement must never call a freed Rust closure.
        let _ = self.disconnect.call0(&JsValue::UNDEFINED);
    }
}
