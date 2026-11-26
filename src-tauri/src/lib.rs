pub mod modules;

use modules::{agent, backup, fonts, pty, storage, variables, workspace};
// `Manager` is what brings `get_webview_window` into scope, and the only call
// is the Windows-only `set_decorations` below — so the import has to carry the
// same `cfg` or macOS builds warn about it being unused. Dropping it instead of
// gating it compiles cleanly here and breaks the Windows build, which no amount
// of `cargo check` on macOS will tell you.
#[cfg(windows)]
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Storage opens before the builder so `manage` still happens up front —
    // a command must never find the state missing. When it fails there is
    // nothing to manage and the app exists only to show the reason.
    let db = storage::init();
    let failure = db.as_ref().err().cloned();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(pty::PtyManager::default())
        .manage(agent::AgentManager::default());

    if let Ok(db) = db {
        builder = builder.manage(db);
    }

    // The menu is macOS-only, and not for want of one on Windows: there it is a
    // strip drawn *inside* the window, between the title bar and the app's own
    // header, which is a row of chrome spent on nothing. Everything in it is
    // already reachable — copy/paste and select-all are WebView2's own bindings,
    // minimise/maximise are the title bar buttons, quit is closing the window,
    // and the version lives in Settings. macOS is the opposite case: the menu
    // costs no window space at all, and the Edit items' accelerators are what
    // drive copy/paste in the webview there (see modules::menu).
    //
    // Leaving it unset is enough. Tauri only substitutes its default menu under
    // `#[cfg(target_os = "macos")]`, so Windows gets no menu rather than the
    // stock one.
    #[cfg(target_os = "macos")]
    {
        builder = builder.menu(modules::menu::build);
    }

    builder
        .setup(move |app| {
            if let Some(message) = failure {
                eprintln!("{message}");
                app.dialog()
                    .message(message)
                    .title("Kelyfos cannot open its database")
                    .kind(MessageDialogKind::Error)
                    .show(|_| std::process::exit(1));
            }
            // Drop the native title bar on Windows, so the app's own strip is
            // the only one — the same shape macOS already has through
            // titleBarStyle "Overlay". The frontend draws the minimise,
            // maximise and close buttons there (see TitleBar.tsx); tao keeps
            // hit-testing an undecorated resizable window, so edge resizing,
            // aero snap and the drop shadow all survive.
            //
            // Done here rather than in tauri.conf.json because `decorations`
            // there is shared with macOS, where it would take the traffic
            // lights with it. A tauri.windows.conf.json is the other option and
            // is worse: platform config is merged with RFC 7386 semantics,
            // which replaces the whole `windows` array, so the title, size and
            // dragDropEnabled would have to be duplicated and kept in step.
            #[cfg(windows)]
            if let Some(window) = app.get_webview_window("main") {
                window.set_decorations(false).ok();
            }
            // Claude Code awareness. Starts unconditionally and finds nothing on
            // a machine without Claude, which is the point: the terminal must
            // work identically for someone who has never heard of it.
            agent::start(app.handle().clone());
            // The busy indicator's fallback. On unix each session polls its own
            // PTY for the foreground process group; Windows has no such thing,
            // so one app-global task walks the process tree for every terminal
            // at once — see modules::process.
            #[cfg(windows)]
            pty::start_busy_poller(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            storage::connections_list,
            storage::connections_save,
            storage::connections_delete,
            storage::workspaces_list,
            storage::workspaces_save,
            storage::workspaces_delete,
            storage::terminals_list,
            storage::terminals_save,
            storage::terminals_delete,
            storage::presets_list,
            storage::presets_save,
            storage::presets_delete,
            storage::preset_copy_variables,
            storage::variables_list,
            storage::variables_set,
            storage::variables_delete,
            storage::settings_get_all,
            storage::settings_set,
            storage::state_get,
            storage::state_set,
            variables::variables_resolve,
            workspace::workspace_export,
            workspace::workspace_import,
            workspace::workspace_duplicate,
            pty::pty_open,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            pty::pty_pause,
            pty::pty_resume,
            pty::pty_cwd,
            pty::shells_list,
            fonts::fonts_list,
            backup::backup_create,
            agent::claude_health,
            agent::claude_connect,
            agent::claude_disconnect,
            agent::claude_theme_install,
            agent::claude_theme_remove,
            agent::claude_states,
            agent::claude_test,
            agent::stats::claude_sessions,
            agent::stats::claude_session_stats,
            agent::stats::claude_daily_stats,
            agent::stats::git_branch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
