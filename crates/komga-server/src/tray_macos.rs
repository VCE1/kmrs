//! macOS menu bar integration. AppKit requires the status item and its event loop
//! on the main thread, so in a GUI session the main thread runs NSApplication and
//! the server runs on a background thread instead.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{ns_string, NSActivityOptions, NSPoint, NSProcessInfo, NSURL};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MENU_BAR_ICON_PNG: &[u8] = include_bytes!("../resources/windows/tray-icon.png");

/// Set while the main thread is handed to NSApplication; unset, there is no event
/// loop to own a menu bar item (no graphic access, e.g. SSH or a LaunchDaemon).
static APP_RUNNING: AtomicBool = AtomicBool::new(false);

thread_local! {
    // TrayIcon is main-thread only; it lives here between start() and stop()
    static STATUS_ITEM: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    // the menu event handler must be Send+Sync, so the check item (Rc-based)
    // cannot move into it; muda dispatches menu events on the main thread, where
    // the item is reachable again
    static AUTOSTART_ITEM: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    static UPDATE_ITEM: RefCell<Option<MenuItem>> = const { RefCell::new(None) };
    static TRAY_ICON: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
}

pub struct Tray;

/// Folders the menu opens in Finder.
pub struct TrayPaths {
    pub config_dir: PathBuf,
    pub logs_dir: PathBuf,
}

/// Runs `server` to completion. In a GUI session the main thread is handed to
/// NSApplication for the menu bar and `server` runs on its own thread; otherwise
/// `server` runs right here, exactly as on other platforms.
pub fn run_app<F>(server: F) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    let Some(mtm) = MainThreadMarker::new() else {
        return server();
    };
    if !session_has_graphic_access() {
        return server();
    }

    let app = NSApplication::sharedApplication(mtm);
    // no Dock icon and no app menu: the menu bar item is the whole UI
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // a windowless accessory app is eligible for App Nap, which would throttle
    // timers, CPU and I/O while the server is busy; idle system sleep stays allowed
    let process = NSProcessInfo::processInfo();
    let activity = process.beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        ns_string!("KMServer"),
    );

    APP_RUNNING.store(true, Ordering::SeqCst);
    let handle = std::thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            // stops the event loop however the server ends, panics included,
            // so the main thread never waits in app.run() forever
            let _stop = StopAppOnDrop;
            server()
        })?;
    app.run();
    APP_RUNNING.store(false, Ordering::SeqCst);

    unsafe { process.endActivity(&activity) };
    match handle.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SessionGetInfo(session: u32, session_id: *mut u32, attributes: *mut u32) -> i32;
}

/// Whether this process may draw on a display. An SSH login, or a launchd job
/// outside a GUI login, lacks it even while the same user is logged in at the
/// console, where an AppKit status item would otherwise still show up.
fn session_has_graphic_access() -> bool {
    const CALLER_SECURITY_SESSION: u32 = u32::MAX;
    const SESSION_HAS_GRAPHIC_ACCESS: u32 = 0x0010;
    let mut session_id = 0;
    let mut attributes = 0;
    let status =
        unsafe { SessionGetInfo(CALLER_SECURITY_SESSION, &mut session_id, &mut attributes) };
    status == 0 && attributes & SESSION_HAS_GRAPHIC_ACCESS != 0
}

struct StopAppOnDrop;

impl Drop for StopAppOnDrop {
    fn drop(&mut self) {
        DispatchQueue::main().exec_async(|| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let app = NSApplication::sharedApplication(mtm);
            app.stop(None);
            // stop only takes effect after the run loop handles another event
            if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                NSEventType::ApplicationDefined,
                NSPoint::new(0.0, 0.0),
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                0,
            ) {
                app.postEvent_atStart(&event, true);
            }
        });
    }
}

/// Opens the web UI in the default browser.
pub fn open_in_browser(port: u16) {
    open(std::ffi::OsStr::new(&format!("http://localhost:{port}/")));
}

/// Returns None without a running NSApplication (no GUI session) or when the
/// status item cannot be created; the server runs unaffected either way.
pub fn start(
    port: u16,
    paths: TrayPaths,
    shutdown: tokio::sync::watch::Sender<bool>,
    update: crate::service::desktop_update::DesktopUpdate,
) -> Option<Tray> {
    if !APP_RUNNING.load(Ordering::SeqCst) {
        return None;
    }
    let (ready_tx, ready_rx) = mpsc::channel();
    DispatchQueue::main().exec_async(move || {
        let result = build(port, paths, shutdown, update).map(|item| {
            STATUS_ITEM.with(|slot| *slot.borrow_mut() = Some(item));
        });
        let _ = ready_tx.send(result);
    });
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Some(Tray),
        Ok(Err(e)) => {
            tracing::warn!("menu bar item disabled: {e}");
            None
        }
        Err(_) => {
            tracing::warn!("menu bar item disabled: initialization timed out");
            None
        }
    }
}

impl Tray {
    /// Removes the menu bar item as soon as serving ends, before the task drain.
    pub fn stop(self) {
        let (done_tx, done_rx) = mpsc::channel();
        DispatchQueue::main().exec_async(move || {
            TRAY_ICON.with(|slot| slot.borrow_mut().take());
            STATUS_ITEM.with(|slot| slot.borrow_mut().take());
            let _ = done_tx.send(());
        });
        let _ = done_rx.recv_timeout(Duration::from_secs(5));
    }
}

fn build(
    port: u16,
    paths: TrayPaths,
    shutdown: tokio::sync::watch::Sender<bool>,
    update: crate::service::desktop_update::DesktopUpdate,
) -> Result<TrayIcon, String> {
    let open_item = MenuItem::with_id("open", "Open KMServer", true, None);
    let config_item = MenuItem::with_id("config", "Open configuration directory", true, None);
    let logs_item = MenuItem::with_id("logs", "Open logs folder", true, None);
    let autostart_item = crate::desktop::app_bundle_path().map(|_| {
        CheckMenuItem::with_id(
            "autostart",
            "Open at Login",
            true,
            autostart_enabled(),
            None,
        )
    });
    let quit_item = MenuItem::with_id("quit", "Quit KMServer", true, None);
    let update_item = crate::desktop::app_bundle_path().map(|_| {
        MenuItem::with_id(
            "update",
            crate::desktop::status_label(&update.status()),
            true,
            None,
        )
    });
    let menu = Menu::new();
    let _ = menu.append(&open_item);
    let _ = menu.append(&config_item);
    // the log files are where startup and scan errors end up when no terminal is
    // attached, so the menu only offers the folder when file logging came up
    let logs_dir = paths.logs_dir.is_dir().then_some(paths.logs_dir);
    if logs_dir.is_some() {
        let _ = menu.append(&logs_item);
    }
    if let Some(item) = &autostart_item {
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(item);
    }
    if let Some(item) = &update_item {
        let _ = menu.append(item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    let icon = load_icon().map_err(|e| format!("menu bar icon decode: {e}"))?;
    let item = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("KMServer")
        .with_icon_templated(icon)
        .build()
        .map_err(|e| e.to_string())?;

    if let Some(item) = autostart_item {
        AUTOSTART_ITEM.with(|slot| *slot.borrow_mut() = Some(item));
    }
    if let Some(item) = update_item {
        UPDATE_ITEM.with(|slot| *slot.borrow_mut() = Some(item));
    }
    TRAY_ICON.with(|slot| *slot.borrow_mut() = Some(item.clone()));
    let config_dir = paths.config_dir;
    let refresh_update = update.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if matches!(event, TrayIconEvent::Click { .. }) {
            refresh_autostart_item();
            refresh_update_item(&refresh_update);
        }
    }));
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        "open" => open_in_browser(port),
        "config" => open(config_dir.as_os_str()),
        "logs" => {
            if let Some(dir) = &logs_dir {
                open(dir.as_os_str());
            }
        }
        "autostart" => {
            let service = unsafe { SMAppService::mainAppService() };
            match unsafe { service.status() } {
                status if status == SMAppServiceStatus::RequiresApproval => {
                    open_login_settings();
                }
                status => {
                    let enable = status != SMAppServiceStatus::Enabled;
                    let result = unsafe {
                        if enable {
                            service.registerAndReturnError()
                        } else {
                            service.unregisterAndReturnError()
                        }
                    };
                    if let Err(e) = result {
                        tracing::warn!("login item unchanged: {e}");
                    }
                }
            }
            refresh_autostart_item();
        }
        "quit" => {
            let _ = shutdown.send(true);
        }
        "update" => {
            if !crate::desktop::busy(&update.status()) {
                let update = update.clone();
                let shutdown = shutdown.clone();
                let app = crate::desktop::app_bundle_path().unwrap();
                let config_dir = config_dir.clone();
                std::thread::spawn(move || match update.status() {
                    crate::service::desktop_update::Status::Available(_) => {
                        crate::desktop::start_update(update, app, config_dir, port, shutdown)
                    }
                    _ => {
                        let _ = update.start_check();
                    }
                });
            }
            refresh_update_item(&update);
        }
        _ => {}
    }));
    Ok(item)
}

fn load_icon() -> Result<Icon, String> {
    let image = image::load_from_memory_with_format(MENU_BAR_ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).map_err(|e| e.to_string())
}

/// Hands a URL or folder to Launch Services. Runs off the main thread so a slow
/// `open` never stalls the menu, and waits so the child is reaped.
fn open(target: &std::ffi::OsStr) {
    let target = Path::new(target).to_path_buf();
    std::thread::spawn(move || {
        match std::process::Command::new("/usr/bin/open")
            .arg(&target)
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => tracing::warn!("open {} failed: {status}", target.display()),
            Err(e) => tracing::warn!("open {} failed: {e}", target.display()),
        }
    });
}

fn autostart_enabled() -> bool {
    (unsafe { SMAppService::mainAppService().status() }) == SMAppServiceStatus::Enabled
}

fn refresh_autostart_item() {
    AUTOSTART_ITEM.with(|slot| {
        if let Some(item) = slot.borrow().as_ref() {
            item.set_checked(autostart_enabled());
        }
    });
}

fn refresh_update_item(update: &crate::service::desktop_update::DesktopUpdate) {
    UPDATE_ITEM.with(|slot| {
        if let Some(item) = slot.borrow().as_ref() {
            let status = update.status();
            item.set_text(crate::desktop::status_label(&status));
            item.set_enabled(!crate::desktop::busy(&status));
            TRAY_ICON.with(|slot| {
                if let Some(icon) = slot.borrow().as_ref() {
                    let _ = icon.set_tooltip(Some(crate::desktop::tooltip(&status)));
                }
            });
        }
    });
}

fn open_login_settings() {
    let url = NSURL::URLWithString(ns_string!(
        "x-apple.systempreferences:com.apple.LoginItems-Settings.extension"
    ))
    .unwrap();
    objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url);
}
