//! Windows single-instance guard.
//!
//! Uses a named mutex to detect an already-running instance, and a named auto-reset
//! event as the cross-process signal that tells the first instance to show its main
//! window when a second launch is attempted.

#![cfg(target_os = "windows")]

use std::time::Duration;

use gpui::{App, AsyncApp, WindowHandle};
use gpui_component::Root;
use tracing::error;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows::core::HSTRING;

const MUTEX_NAME: &str = "Local\\com.yunisdu.tide.single-instance.mutex";
const EVENT_NAME: &str = "Local\\com.yunisdu.tide.single-instance.event";

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

        let mutex = CreateMutexW(None, false, &mutex_name)?;
        let already_exists = GetLastError() == ERROR_ALREADY_EXISTS;

        if already_exists {
            if let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, &event_name) {
                let _ = SetEvent(event);
                let _ = CloseHandle(event);
            }
            let _ = CloseHandle(mutex);
            return Ok(Acquired::AlreadyRunning);
        }

        let event = match CreateEventW(None, false, false, &event_name) {
            Ok(h) => h,
            Err(e) => {
                let _ = CloseHandle(mutex);
                return Err(e.into());
            }
        };

        Ok(Acquired::First(Guard { mutex, event }))
    }
}

pub fn spawn_watcher(cx: &App, guard: Guard, main_window: WindowHandle<Root>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let signaled = unsafe { WaitForSingleObject(guard.event, 0) == WAIT_OBJECT_0 };
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
