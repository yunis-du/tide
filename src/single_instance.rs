#![cfg(target_os = "windows")]

use std::sync::OnceLock;
use std::time::Duration;

use gpui::{App, AsyncApp, WindowHandle};
use gpui_component::Root;
use tracing::{error, info, warn};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, OpenMutexW,
    SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
};
use windows::core::HSTRING;

const MUTEX_NAME: &str = "Local\\com.yunisdu.tide.single-instance.mutex";
const EVENT_NAME: &str = "Local\\com.yunisdu.tide.single-instance.event";

static INSTANCE_GUARD: OnceLock<Guard> = OnceLock::new();

pub struct Guard {
    mutex: HANDLE,
    event: HANDLE,
}

// Win32 kernel HANDLEs are stable pointers safe to use from any thread as long as
// the APIs we call (WaitForSingleObject/SetEvent/CloseHandle) are themselves thread-safe.
unsafe impl Send for Guard {}
unsafe impl Sync for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.event);
            let _ = CloseHandle(self.mutex);
        }
    }
}

pub enum Acquired {
    First(Guard),
    AlreadyRunning,
}

pub fn acquire() -> anyhow::Result<Acquired> {
    unsafe {
        let mutex_name = HSTRING::from(MUTEX_NAME);
        let event_name = HSTRING::from(EVENT_NAME);

        if let Ok(existing) = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &mutex_name) {
            info!("detected existing Tide instance via OpenMutexW");
            let _ = CloseHandle(existing);
            notify_existing_instance(&event_name);
            return Ok(Acquired::AlreadyRunning);
        }

        let mutex_result = CreateMutexW(None, false, &mutex_name);
        let last_error = GetLastError();
        let mutex = mutex_result?;

        if last_error == ERROR_ALREADY_EXISTS {
            info!("detected existing Tide instance via CreateMutexW race fallback");
            let _ = CloseHandle(mutex);
            notify_existing_instance(&event_name);
            return Ok(Acquired::AlreadyRunning);
        }

        let event = match CreateEventW(None, false, false, &event_name) {
            Ok(h) => h,
            Err(e) => {
                let _ = CloseHandle(mutex);
                return Err(e.into());
            }
        };

        info!("Tide started as the first instance");
        Ok(Acquired::First(Guard { mutex, event }))
    }
}

unsafe fn notify_existing_instance(event_name: &HSTRING) {
    unsafe {
        match OpenEventW(EVENT_MODIFY_STATE, false, event_name) {
            Ok(event) => {
                if let Err(e) = SetEvent(event) {
                    warn!(error = %e, "failed to signal existing instance");
                }
                let _ = CloseHandle(event);
            }
            Err(e) => {
                warn!(error = %e, "failed to open existing instance event");
            }
        }
    }
}

pub fn spawn_watcher(cx: &App, guard: Guard, main_window: WindowHandle<Root>) {
    let event_handle = guard.event;
    if let Err(guard) = INSTANCE_GUARD.set(guard) {
        warn!("spawn_watcher called more than once; leaking guard to keep mutex alive");
        std::mem::forget(guard);
    }

    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let signaled = unsafe { WaitForSingleObject(event_handle, 0) == WAIT_OBJECT_0 };
            if signaled && let Err(e) = cx.update(|cx| activate(cx, main_window)) {
                error!(error = %e, "failed to handle single-instance activation");
            }
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
        }
    })
    .detach();
}

fn activate(cx: &mut App, main_window: WindowHandle<Root>) {
    cx.activate(true);
    let _ = main_window.update(cx, |_, w, _| {
        crate::show_on_windows(w);
        w.activate_window();
    });
}
