//! The application menu.
//!
//! **Only attached on macOS** — `lib.rs` does not call `.menu()` on Windows,
//! where the menu would be a strip inside the window rather than in the system
//! bar. The non-macOS arms below are kept so the menu still builds correctly if
//! it is ever attached there again; today they render nothing.
//!
//! This is Tauri's default menu with one thing taken out: "Close Window". On
//! macOS its Cmd+W accelerator is a menu key equivalent, which AppKit resolves
//! before the webview ever sees the keystroke — so the window closed out from
//! under the terminals. With the item gone, Cmd+W reaches the frontend, which
//! closes the selected terminal instead (see `useCloseTerminalHotkey`).
//!
//! Everything else is kept verbatim, in particular the Edit submenu: on macOS
//! copy/paste in the webview are driven by those menu items' accelerators.

use tauri::{
    menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu},
    AppHandle, Runtime,
};

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg = app.package_info();
    let config = app.config();
    let about = AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };

    let mut items: Vec<Box<dyn tauri::menu::IsMenuItem<R>>> = Vec::new();

    #[cfg(target_os = "macos")]
    items.push(Box::new(Submenu::with_items(
        app,
        pkg.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(app, None, Some(about.clone()))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?));

    // On Windows the File submenu held only Close Window and Quit; without the
    // former, Quit alone is all that is left to put there.
    #[cfg(windows)]
    items.push(Box::new(Submenu::with_items(
        app,
        "File",
        true,
        &[&PredefinedMenuItem::quit(app, None)?],
    )?));

    items.push(Box::new(Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?));

    #[cfg(target_os = "macos")]
    items.push(Box::new(Submenu::with_items(
        app,
        "View",
        true,
        &[&PredefinedMenuItem::fullscreen(app, None)?],
    )?));

    items.push(Box::new(Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
        ],
    )?));

    #[cfg(not(target_os = "macos"))]
    items.push(Box::new(Submenu::with_items(
        app,
        "Help",
        true,
        &[&PredefinedMenuItem::about(app, None, Some(about))?],
    )?));

    let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items.iter().map(|i| i.as_ref()).collect();
    Menu::with_items(app, &refs)
}
