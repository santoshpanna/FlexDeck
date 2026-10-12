//! The flexdeck desktop window.
//!
//! This process *is* the agent. It runs the same server the command-line binary
//! runs — same identity, same store, same listener — and adds a window and a tray
//! icon on top. That is why the start-up sequence lives in `flexdeck-agent`
//! rather than here: two copies of it would drift, and the failure that produces
//! is a regenerated identity, after which every paired tablet rejects the agent
//! while the agent itself looks perfectly healthy.
//!
//! Release builds are a GUI-subsystem program on Windows: a double-clicked app
//! must not drag a console window along with it. Debug builds keep the console,
//! because there the banner — address, fingerprint, any start-up warning — is
//! the quickest way to see a control surface that silently failed to bind. When
//! a release build is started from a terminal it attaches to that terminal, so
//! `verify-plugin` and a mistyped flag still print where the user is looking.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod plugins;
mod startup;
mod state;

use std::path::PathBuf;
use std::sync::Arc;

use flexdeck_agent::{Host, HostParts, Options};
use flexdeck_server::tunnel::TunnelConfig;
use flexdeck_server::WebAccess;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

fn main() {
    attach_parent_console();
    // `flexdeck verify-plugin …`: the plugin SDKs ask the installed app whether
    // a plugin will work in it. No window for that.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("verify-plugin") {
        std::process::exit(flexdeck_server::verify::cli(&raw[1..]));
    }
    // Parsed before the window exists, so a mistyped flag is a message on the
    // console rather than a window that opens and cannot do the one thing it was
    // started for.
    let options = match options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::begin_pairing,
            commands::cancel_pairing,
            commands::begin_browser_pairing,
            commands::cancel_browser_pairing,
            commands::answer_approval,
            commands::set_web_access,
            commands::set_portal,
            commands::revoke_device,
            commands::revoke_all_devices,
            commands::open_link,
            startup::window_prefs,
            startup::set_window_prefs,
            plugins::plugin_inspect,
            plugins::plugin_install_file,
            plugins::plugin_install_repo,
            plugins::plugin_catalog,
            plugins::repos_refresh,
            plugins::repo_preview,
            plugins::repo_add,
            plugins::repo_remove,
            plugins::repo_toggle,
            plugins::plugin_uninstall,
            plugins::plugin_enable,
            plugins::plugin_reload,
            plugins::plugin_link,
            plugins::plugin_settings,
            plugins::plugin_log,
        ])
        .setup(move |app| {
            // Assembled on Tauri's runtime, because enabling web access spawns
            // the tunnel task and a spawn needs a reactor to land on.
            let host = tauri::async_runtime::block_on(async { Host::assemble(options) })
                .map_err(|error| -> Box<dyn std::error::Error> { error.to_string().into() })?;

            host.report();

            // The window keeps the handles; the server gets its own copy of them
            // and runs for as long as the process does.
            let parts = host.into_parts();
            let serving = HostParts {
                agent: Arc::clone(&parts.agent),
                settings: parts.settings.clone(),
                bind: parts.bind,
                web_access: parts.web_access.clone(),
            };

            // Shared with the server task so that a listener which fails to come
            // up can be reported *in the window*, not only on a console nobody is
            // watching.
            let server_error = Arc::new(std::sync::Mutex::new(None));

            // The window ALWAYS has a web-access handle, even where the host had
            // none because no portal is configured. It is what lets the window
            // set the address — without it, the only way to correct a wrong or
            // missing portal is a command line, which is the dead end this
            // replaces. The host's own is reused when it has one, so there is
            // never a second tunnel.
            //
            // The server keeps whatever the host had: a portal-less installation
            // does not silently grow a `/api/web-access` route on its LAN
            // listener just because the window has a form.
            let web_access = parts.web_access.clone().unwrap_or_else(|| {
                Arc::new(WebAccess::new(
                    Arc::clone(&parts.agent),
                    flexdeck_server::http::router(Arc::clone(&parts.agent)),
                    TunnelConfig {
                        url: parts.settings.cloud_url.clone(),
                        name: flexdeck_agent::desktop_name(),
                    },
                ))
            });

            // This window can ask a person, so a browser elsewhere may request
            // the actions that need a yes. The headless agent never does this,
            // and so refuses them.
            parts.agent.approvals().enable();

            app.manage(state::ShellState::new(
                parts,
                Arc::clone(&web_access),
                Arc::clone(&server_error),
            ));

            {
                let shell = app.state::<state::ShellState>();
                events::spawn(
                    app.handle().clone(),
                    Arc::clone(&shell.agent),
                    Arc::clone(&shell.web_access),
                );
            }

            let handle = app.handle().clone();
            let bind = serving.bind;
            tauri::async_runtime::spawn(async move {
                if let Err(error) = flexdeck_agent::serve_parts(serving).await {
                    let message = explain(&error.to_string(), bind);
                    eprintln!("the agent stopped: {message}");

                    *server_error
                        .lock()
                        .expect("the server lock is not poisoned") = Some(message);
                    events::announce(&handle);
                }
            });

            build_tray(app)?;

            // The window is created hidden, so starting to the tray never
            // flashes it on screen first.
            let prefs = startup::load(app.handle());
            app.manage(startup::PrefsState(std::sync::Mutex::new(prefs)));
            if !prefs.start_minimized {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                // By default closing the window hides it rather than ending the
                // process: the agent is serving tablets, and exiting because
                // someone tidied a window away would take their deck down.
                // Quitting is the tray's explicit, separate action — unless the
                // person turned "close to tray" off, in which case close means quit.
                let close_to_tray = window
                    .try_state::<startup::PrefsState>()
                    .is_none_or(|prefs| prefs.get().close_to_tray);
                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
            // Minimizing arrives as a resize. With "minimize to tray" on, the
            // window leaves the taskbar too; the tray brings it back restored.
            WindowEvent::Resized(_) => {
                let minimize_to_tray = window
                    .try_state::<startup::PrefsState>()
                    .is_some_and(|prefs| prefs.get().minimize_to_tray);
                if minimize_to_tray && window.is_minimized().unwrap_or(false) {
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .run(context())
        .expect("the window could not start");
}

/// Debug builds are their own app, so `pnpm dev:desktop` runs beside an
/// installed release instead of fighting it: a different identifier (its own
/// WebView profile and app-data folder), a name that says "dev" in the title
/// bar and tray, and its own port. Release builds keep `tauri.conf.json` as is.
const DEV_BUILD: bool = cfg!(debug_assertions);

/// The dev build's port, one above the release default so both can listen.
const DEV_PORT: u16 = flexdeck_server::settings::DEFAULT_PORT + 1;

/// What the window calls itself: the product name, marked in a dev build.
const APP_NAME: &str = if DEV_BUILD { "flexdeck dev" } else { "flexdeck" };

fn context() -> tauri::Context<tauri::Wry> {
    let mut context = tauri::generate_context!();

    if DEV_BUILD {
        let config = context.config_mut();
        config.identifier.push_str(".dev");
        config.product_name = Some(APP_NAME.to_string());
        for window in &mut config.app.windows {
            window.title = APP_NAME.to_string();
        }
        context.package_info_mut().name = APP_NAME.to_string();
    }

    context
}

/// The flags the window honours.
///
/// Two, and both because the window cannot work them out for itself. Where the
/// database and identity live must match the command-line binary; where the
/// portal lives decides whether web access is offered at all — a desktop that
/// cannot name a portal opens no tunnel, so the toggle would be a button that
/// only ever fails.
///
/// A malformed portal is refused here, at the moment it was typed, rather than
/// persisted and then surfacing as a tunnel that quietly never connects — the
/// same rule the command-line binary applies.
fn options() -> Result<Options, String> {
    let mut options = Options::default();
    if DEV_BUILD {
        options.port = Some(DEV_PORT);
    }
    let mut args = std::env::args().skip(1);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--data-dir" => {
                if let Some(value) = args.next() {
                    options.data_dir = PathBuf::from(value);
                }
            }

            "--cloud" => {
                let value = args.next().ok_or("--cloud needs a URL")?;

                flexdeck_server::net::check_portal_url(&value)?;

                options.cloud = Some(value);
            }

            // Unknown flags are ignored on purpose: the window is not a CLI, and
            // failing to open because a launcher passed something extra would be
            // worse than ignoring it.
            _ => {}
        }
    }

    Ok(options)
}

/// Turn a listener failure into something worth showing a person.
///
/// The common case by far is the port already being taken — usually by this same
/// agent run from a terminal, which is easy to do by accident and impossible to
/// diagnose from a window that otherwise looks fine.
fn explain(error: &str, bind: std::net::SocketAddr) -> String {
    if error.contains("10048") || error.contains("address in use") {
        return format!(
            "Something is already using port {} — another agent may be running. Close it and reopen this window.",
            bind.port()
        );
    }

    format!("This computer could not be reached: {error}")
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", format!("Show {APP_NAME}"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .tooltip(APP_NAME)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    // Hidden while minimized, it would come back minimized.
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });

    // The window icon is embedded by tauri-build from `bundle.icon`, so there is
    // no second copy of the image to keep in step. If it is somehow absent the
    // tray still works, just without a picture.
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }

    tray.build(app)?;
    Ok(())
}

/// A GUI-subsystem process starts with no console. If it was launched from a
/// terminal, borrow that one so `eprintln!` and the banner still reach it;
/// launched from Explorer there is no parent console and this does nothing.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    // SAFETY: AttachConsole takes a plain integer and fails harmlessly when
    // there is no parent console or one is already attached.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}
