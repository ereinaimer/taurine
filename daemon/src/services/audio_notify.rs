//! OS push notifications for audio endpoint changes.
//!
//! Polling the default device cannot catch switches the OS reports
//! ambiguously (same name, virtual re-route), and enumeration stalls for
//! seconds mid-flux. This subscribes to endpoint notifications so
//! add/remove/state/default changes invalidate caches immediately.
//! Callback discipline: the OS calls us on its own thread, so handlers
//! only drop the cue cache (own mutex, no OS calls) and stamp an atomic.
//! All heavy work stays on existing threads.

/// WASAPI data-flow direction (matches EDataFlow).
/// Windows-only at runtime (win_notify is cfg(windows)); shared here so
/// tests assert the same routing on every platform.
#[allow(dead_code)]
pub(crate) const E_RENDER: u32 = 0;
/// WASAPI data-flow direction (matches EDataFlow).
#[allow(dead_code)]
pub(crate) const E_CAPTURE: u32 = 1;

/// Render default moved (or topology changed): the cue sink may point at
/// a departed endpoint. Best-effort and silent.
#[allow(dead_code)]
pub(crate) fn on_default_endpoint_changed(flow: u32) {
    if flow == E_RENDER {
        crate::services::audio::drop_cached_voice_sink();
    }
    crate::voice::device_monitor::mark_device_change();
}

/// A device arrived, left, or changed state: identity checks may be
/// stale on both sides. Best-effort and silent.
#[allow(dead_code)]
pub(crate) fn on_endpoint_topology_changed() {
    crate::services::audio::drop_cached_voice_sink();
    crate::voice::device_monitor::mark_device_change();
}

#[cfg(windows)]
pub use win_notify::{start, stop};

#[cfg(not(windows))]
pub fn start() {}

#[cfg(not(windows))]
pub fn stop() {}

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win_notify {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
    use windows_sys::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::core::{GUID, HRESULT};

    const S_OK: HRESULT = 0;
    const E_NOINTERFACE: HRESULT = 0x80004002u32 as HRESULT;

    // IID_IMMNotificationClient {7991EEC9-7E89-4D85-8390-6C703CEC60C0} (MSDN).
    const IID_IMMNOTIFICATIONCLIENT: GUID = GUID {
        data1: 0x7991EEC9,
        data2: 0x7E89,
        data3: 0x4D85,
        data4: [0x83, 0x90, 0x6C, 0x70, 0x3C, 0xEC, 0x60, 0xC0],
    };

    const IID_IUNKNOWN: GUID = GUID {
        data1: 0,
        data2: 0,
        data3: 0,
        data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    };

    // Same enumerator GUIDs as the win_com precedent in voice::capture.
    const CLSID_MMDEVICE_ENUMERATOR: GUID = GUID {
        data1: 0xBCDE0395,
        data2: 0xE52F,
        data3: 0x467C,
        data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
    };

    const IID_IMMDEVICE_ENUMERATOR: GUID = GUID {
        data1: 0xA95664D2,
        data2: 0x9614,
        data3: 0x4F35,
        data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
    };

    fn guid_eq(a: &GUID, b: &GUID) -> bool {
        a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
    }

    /// 20-byte PROPERTYKEY placeholder (16-byte GUID + 4-byte pid).
    /// Never read; only keeps the callback signature layout-compatible.
    #[repr(C)]
    pub struct NotifyPropertyKey(pub [u8; 20]);

    #[repr(C)]
    struct NotifyClient {
        vtbl: *const NotifyVtbl,
        refcount: AtomicU32,
    }

    #[repr(C)]
    struct IUnknownVtbl {
        QueryInterface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
        Release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    struct EnumeratorVtbl {
        base: IUnknownVtbl,
        EnumAudioEndpoints:
            unsafe extern "system" fn(*mut c_void, i32, u32, *mut *mut c_void) -> HRESULT,
        GetDefaultAudioEndpoint:
            unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> HRESULT,
        GetDevice: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT,
        RegisterEndpointNotificationCallback:
            unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
        UnregisterEndpointNotificationCallback:
            unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    }

    /// IMMNotificationClient vtbl in MSDN order: OnDeviceStateChanged,
    /// OnDeviceAdded, OnDeviceRemoved, OnDefaultDeviceChanged,
    /// OnPropertyValueChanged.
    #[repr(C)]
    struct NotifyVtbl {
        base: IUnknownVtbl,
        OnDeviceStateChanged: unsafe extern "system" fn(*mut c_void, *const u16, u32) -> HRESULT,
        OnDeviceAdded: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        OnDeviceRemoved: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        OnDefaultDeviceChanged:
            unsafe extern "system" fn(*mut c_void, u32, u32, *const u16) -> HRESULT,
        OnPropertyValueChanged:
            unsafe extern "system" fn(*mut c_void, *const u16, NotifyPropertyKey) -> HRESULT,
    }

    // SAFETY: COM entry point; the OS passes a valid client pointer, a valid
    // IID pointer, and a writable out pointer. Only reads/writes those.
    unsafe extern "system" fn notify_query_interface(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT {
        if riid.is_null() || ppv.is_null() {
            return E_NOINTERFACE;
        }
        // SAFETY: nulls checked above; the OS guarantees valid pointers here.
        unsafe {
            if guid_eq(&*riid, &IID_IUNKNOWN) || guid_eq(&*riid, &IID_IMMNOTIFICATIONCLIENT) {
                notify_add_ref(this);
                *ppv = this;
                S_OK
            } else {
                *ppv = std::ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }

    // SAFETY: the OS passes a valid client pointer handed out at registration.
    unsafe extern "system" fn notify_add_ref(this: *mut c_void) -> u32 {
        // SAFETY: pointer validity is the caller's (OS) contract above.
        unsafe {
            let client = &*(this as *const NotifyClient);
            client.refcount.fetch_add(1, Ordering::SeqCst) + 1
        }
    }

    // SAFETY: same contract as AddRef; the count is advisory because the
    // client is a process-lifetime leak, never freed.
    unsafe extern "system" fn notify_release(this: *mut c_void) -> u32 {
        // SAFETY: pointer validity is the caller's (OS) contract above.
        unsafe {
            let client = &*(this as *const NotifyClient);
            client.refcount.fetch_sub(1, Ordering::SeqCst) - 1
        }
    }

    // SAFETY: OS notifier thread passes valid args; the handler only drops
    // the cue cache (own mutex, no OS calls) and stamps an atomic.
    unsafe extern "system" fn on_device_state_changed(
        _this: *mut c_void,
        _: *const u16,
        _: u32,
    ) -> HRESULT {
        super::on_endpoint_topology_changed();
        S_OK
    }

    // SAFETY: same contract as OnDeviceStateChanged.
    unsafe extern "system" fn on_device_added(_this: *mut c_void, _: *const u16) -> HRESULT {
        super::on_endpoint_topology_changed();
        S_OK
    }

    // SAFETY: same contract as OnDeviceStateChanged.
    unsafe extern "system" fn on_device_removed(_this: *mut c_void, _: *const u16) -> HRESULT {
        super::on_endpoint_topology_changed();
        S_OK
    }

    // SAFETY: same contract as OnDeviceStateChanged; routes the EDataFlow
    // direction to the shared handler.
    unsafe extern "system" fn on_default_device_changed(
        _this: *mut c_void,
        flow: u32,
        _: u32,
        _: *const u16,
    ) -> HRESULT {
        super::on_default_endpoint_changed(flow);
        S_OK
    }

    // SAFETY: same contract as OnDeviceStateChanged; property churn carries
    // no cache impact, so it is ignored.
    unsafe extern "system" fn on_property_value_changed(
        _this: *mut c_void,
        _: *const u16,
        _: NotifyPropertyKey,
    ) -> HRESULT {
        S_OK
    }

    static NOTIFY_VTBL: NotifyVtbl = NotifyVtbl {
        base: IUnknownVtbl {
            QueryInterface: notify_query_interface,
            AddRef: notify_add_ref,
            Release: notify_release,
        },
        OnDeviceStateChanged: on_device_state_changed,
        OnDeviceAdded: on_device_added,
        OnDeviceRemoved: on_device_removed,
        OnDefaultDeviceChanged: on_default_device_changed,
        OnPropertyValueChanged: on_property_value_changed,
    };

    // Process-lifetime registration pointer (atomics are Send+Sync, no wrapper needed).
    static CLIENT_PTR: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    static STOP_FLAG: AtomicBool = AtomicBool::new(false);

    pub fn start() {
        if !CLIENT_PTR.load(Ordering::SeqCst).is_null() {
            return;
        }
        STOP_FLAG.store(false, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("tau-audio-notify".to_string())
            .spawn(|| {
                // SAFETY: COM is initialized/uninitialized symmetrically on
                // this thread; the enumerator and client pointers are valid
                // between creation and release; the client is intentionally
                // leaked (process-lifetime object, matching the detached-thread
                // precedent in the hook supervisor) so stop() can unregister it.
                unsafe {
                    CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED as u32);
                    let mut enumerator_raw: *mut c_void = std::ptr::null_mut();
                    let hr = CoCreateInstance(
                        &CLSID_MMDEVICE_ENUMERATOR,
                        std::ptr::null_mut(),
                        CLSCTX_ALL,
                        &IID_IMMDEVICE_ENUMERATOR,
                        &mut enumerator_raw,
                    );
                    if hr < 0 || enumerator_raw.is_null() {
                        tracing::debug!(
                            "audio endpoint notifications unavailable; lazy path owns recovery"
                        );
                        CoUninitialize();
                        return;
                    }
                    let vtbl = *(enumerator_raw as *mut *const EnumeratorVtbl);
                    let client: *mut NotifyClient = Box::leak(Box::new(NotifyClient {
                        vtbl: &NOTIFY_VTBL,
                        refcount: AtomicU32::new(1),
                    }));
                    CLIENT_PTR.store(client as *mut c_void, Ordering::SeqCst);
                    let hr = ((*vtbl).RegisterEndpointNotificationCallback)(
                        enumerator_raw,
                        client as *mut c_void,
                    );
                    if hr < 0 {
                        ((*vtbl).base.Release)(enumerator_raw);
                        tracing::debug!(
                            "audio endpoint registration failed; lazy path owns recovery"
                        );
                        CLIENT_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
                        notify_release(client as *mut c_void);
                        CoUninitialize();
                        return;
                    }
                    if STOP_FLAG.load(Ordering::SeqCst) {
                        ((*vtbl).UnregisterEndpointNotificationCallback)(
                            enumerator_raw,
                            client as *mut c_void,
                        );
                        ((*vtbl).base.Release)(enumerator_raw);
                        CLIENT_PTR.store(std::ptr::null_mut(), Ordering::SeqCst);
                        notify_release(client as *mut c_void);
                        CoUninitialize();
                        return;
                    }
                    ((*vtbl).base.Release)(enumerator_raw);
                    while !STOP_FLAG.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    CoUninitialize();
                }
            })
            .ok();
    }

    pub fn stop() {
        STOP_FLAG.store(true, Ordering::SeqCst);
        // SAFETY: the swapped pointer is either null (never started) or the
        // leaked client from start(); COM is initialized/uninitialized
        // symmetrically on this thread and every created pointer is released.
        unsafe {
            let client = CLIENT_PTR.swap(std::ptr::null_mut(), Ordering::SeqCst);
            if client.is_null() {
                return;
            }
            CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED as u32);
            let mut enumerator_raw: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_MMDEVICE_ENUMERATOR,
                std::ptr::null_mut(),
                CLSCTX_ALL,
                &IID_IMMDEVICE_ENUMERATOR,
                &mut enumerator_raw,
            );
            if hr >= 0 && !enumerator_raw.is_null() {
                let vtbl = *(enumerator_raw as *mut *const EnumeratorVtbl);
                ((*vtbl).UnregisterEndpointNotificationCallback)(enumerator_raw, client);
                ((*vtbl).base.Release)(enumerator_raw);
            }
            notify_release(client);
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_render_change_drops_sink_and_stamps() {
        on_default_endpoint_changed(E_RENDER);
        assert!(
            crate::voice::device_monitor::millis_since_device_change() < 60_000,
            "render default change must stamp the moment"
        );
        on_default_endpoint_changed(E_CAPTURE);
        assert!(
            crate::voice::device_monitor::millis_since_device_change() < 60_000,
            "capture default change stamps without touching the cue sink"
        );
    }

    #[test]
    fn endpoint_topology_change_stamps() {
        on_endpoint_topology_changed();
        assert!(
            crate::voice::device_monitor::millis_since_device_change() < 60_000,
            "add/remove/state change must stamp the moment"
        );
    }
}
