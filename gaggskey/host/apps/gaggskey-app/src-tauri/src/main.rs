// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use gaggskey_core::{Manager, Snapshot};
use gaggskey_proto::{Command, Output};
use serde::Deserialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager as _, State, WindowEvent};

const TRAY_ID: &str = "main";

/// Commands accepted from the UI: `{ "cmd": "layer_to", "arg": 2 }`.
#[derive(Deserialize)]
#[serde(tag = "cmd", content = "arg", rename_all = "snake_case")]
enum UiCommand {
    LayerTo(u8),
    LayerToggle(u8),
    BtSelect(u8),
    BtNext,
    BtPrev,
    BtClear,
    OutSelect(OutArg),
    OutToggle,
    Reset,
    Bootloader,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum OutArg {
    Usb,
    Ble,
}

impl From<UiCommand> for Command {
    fn from(c: UiCommand) -> Self {
        match c {
            UiCommand::LayerTo(l) => Command::LayerTo(l),
            UiCommand::LayerToggle(l) => Command::LayerToggle(l),
            UiCommand::BtSelect(p) => Command::BtSelect(p),
            UiCommand::BtNext => Command::BtNext,
            UiCommand::BtPrev => Command::BtPrev,
            UiCommand::BtClear => Command::BtClear,
            UiCommand::OutSelect(OutArg::Usb) => Command::OutSelect(Output::Usb),
            UiCommand::OutSelect(OutArg::Ble) => Command::OutSelect(Output::Ble),
            UiCommand::OutToggle => Command::OutToggle,
            UiCommand::Reset => Command::Reset,
            UiCommand::Bootloader => Command::Bootloader,
        }
    }
}

#[tauri::command]
fn get_snapshot(mgr: State<'_, Manager>) -> Snapshot {
    mgr.snapshot()
}

#[tauri::command]
fn select_device(mgr: State<'_, Manager>, name: String) {
    mgr.select(name);
}

#[tauri::command]
async fn send_command(mgr: State<'_, Manager>, command: UiCommand) -> Result<(), String> {
    mgr.command(command.into(), Duration::from_secs(3)).map(|_| ())
}

/// Short status for tray tooltip/title, e.g. "SYM · L 75% R 65%".
fn summary(s: &Snapshot) -> String {
    let Some(st) = s.state.as_ref().filter(|_| s.connected) else {
        return "GaggsKey — disconnected".into();
    };
    let kb = s.selected.as_deref().filter(|_| s.devices.len() > 1).map(|n| format!("{n} · ")).unwrap_or_default();
    let layer = s.layer_names.get(st.layer as usize).filter(|n| !n.is_empty()).cloned()
        .unwrap_or_else(|| format!("L{}", st.layer));
    let pct = |v: Option<u8>| v.map_or("--".into(), |v| format!("{v}%"));
    format!("{kb}{layer} · L {} R {}", pct(st.battery.left), pct(st.battery.right))
}

fn on_snapshot(app: &AppHandle, s: &Snapshot) {
    let _ = app.emit("snapshot", s);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let text = summary(s);
        let _ = tray.set_tooltip(Some(&text));
        #[cfg(target_os = "linux")]
        let _ = tray.set_title(Some(&text)); // appindicator shows title, not tooltip
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(Manager::spawn(move |s| on_snapshot(&handle, s)));

            let show = MenuItem::with_id(app, "show", "GaggsKey 열기", true, None::<&str>)?;
            let out = MenuItem::with_id(app, "out_toggle", "출력 전환 (USB/BLE)", true, None::<&str>)?;
            let bt_next = MenuItem::with_id(app, "bt_next", "다음 BT 프로필", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show, &sep, &out, &bt_next, &sep, &quit])?;

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().cloned().expect("bundle icon"))
                .tooltip("GaggsKey")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| {
                    let mgr = app.state::<Manager>();
                    let t = Duration::from_secs(3);
                    match e.id.as_ref() {
                        "show" => show_main(app),
                        "out_toggle" => { let _ = mgr.command(Command::OutToggle, t); }
                        "bt_next" => { let _ = mgr.command(Command::BtNext, t); }
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        // Closing the window keeps the app in the tray.
        .on_window_event(|w, e| {
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![get_snapshot, send_command, select_device])
        .run(tauri::generate_context!())
        .expect("error while running GaggsKey");
}
