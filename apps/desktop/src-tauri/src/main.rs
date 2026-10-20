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
use std::time::Duration;

use flexdeck_agent::{Host, Options};
use flexdeck_server::tunnel::TunnelConfig;
use flexdeck_server::WebAccess;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

/// How long the listener waits before trying a taken port again.
///
/// Binding is cheap, so this is slow only to stay quiet: the window gets an
/// immediate attempt the moment it comes forward (see `ShellState::kick`).
const RETRY_EVERY: Duration = Duration::from_secs(5);

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
        // Registered first, as its documentation asks. One process per
        // installation: opening flexdeck while it already sits in the tray
        // brings that window forward instead of starting a second copy that
        // finds its port taken by the first — which, with start-to-tray on, is
        // the easiest way to end up with a window that says BLOCKED and a tray
        // icon that looks identical to the one actually serving. A debug build
        // has its own identifier, so it never collides with an installed one.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_window(app);
        }))
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
            commands::create_agent_token,
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
            plugins::plugin_sign_out,
            plugins::plugin_log,
        ])
        .setup(move |app| {
            let prefs = startup::load(app.handle());
            if let Err(error) = flexdeck_agent::logging::init(&options.data_dir, prefs.log_level.as_str()) {
                eprintln!("warning: could not open the log file: {error}");
            }
            // Assembled on Tauri's runtime, because enabling web access spawns
            // the tunnel task and a spawn needs a reactor to land on.
            let host = tauri::async_runtime::block_on(async { Host::assemble(options) })
                .map_err(|error| -> Box<dyn std::error::Error> { error.to_string().into() })?;

            host.report();

            // The window keeps the handles; the server gets its own copy of them
            // and runs for as long as the process does.
            let parts = host.into_parts();
            let serving = parts.clone();

            // Shared with the server task so that a listener which fails to come
            // up can be reported *in the window*, not only on a console nobody is
            // watching — and so the window can ask for another try.
            let server_error = Arc::new(std::sync::Mutex::new(None));
            let (retry, mut retries) = tokio::sync::mpsc::channel::<()>(1);

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
                retry,
            ));

            {
                let shell = app.state::<state::ShellState>();
                events::spawn(
                    app.handle().clone(),
                    Arc::clone(&shell.agent),
                    Arc::clone(&shell.web_access),
                );
            }

            // The listener, kept trying. A taken port is the common failure and
            // a passing one — the terminal agent holding it gets closed — so the
            // window reports it and tries again, rather than reporting it and
            // needing a restart to notice the port came free. The timer is slow
            // enough to cost nothing; `retry` cuts it short when the window comes
            // forward, because that is when someone has just dealt with the cause
            // and is watching for the result. One forced attempt, then the timer
            // again.
            let handle = app.handle().clone();
            let bind = serving.bind;
            tauri::async_runtime::spawn(async move {
                // Says so only when the state CHANGES. A port taken for an
                // afternoon is one message and one re-read, not one every
                // five seconds.
                let report = |error: Option<String>| {
                    let mut current = server_error
                        .lock()
                        .expect("the server lock is not poisoned");
                    if *current == error {
                        return;
                    }
                    if let Some(message) = &error {
                        tracing::error!("the agent stopped: {message}");
                        eprintln!("the agent stopped: {message}");
                    }
                    *current = error;
                    drop(current);
                    events::announce(&handle);
                };

                loop {
                    let outcome = match flexdeck_agent::bind_parts(serving.clone()).await {
                        Ok(bound) => {
                            report(None);
                            bound.serve().await
                        }
                        Err(error) => Err(error),
                    };

                    let reason = outcome
                        .err()
                        .map_or_else(|| "the listener stopped".to_string(), |error| error.to_string());
                    report(Some(explain(&reason, bind)));

                    // Kicks from before this failure are stale: drop them, so
                    // only a window coming forward from here on cuts the wait.
                    while retries.try_recv().is_ok() {}
                    tokio::select! {
                        () = tokio::time::sleep(RETRY_EVERY) => {}
                        _ = retries.recv() => {}
                    }
                }
            });

            build_tray(app)?;

            // The window is created hidden, so starting to the tray never
            // flashes it on screen first.
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
            // The window coming forward by any route — tray, taskbar, alt-tab,
            // a second launch — is the moment to try a taken port again.
            WindowEvent::Focused(true) => {
                if let Some(state) = window.try_state::<state::ShellState>() {
                    state.kick();
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

/// The dev build's port, two above the release default so both can listen: the
/// port one above is the installed app's same-PC door (`loopback_port`), and a
/// dev build one above would have fought it for that.
const DEV_PORT: u16 = flexdeck_server::settings::DEFAULT_PORT + 2;

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
/// diagnose from a window that otherwise looks fine. The listener keeps trying,
/// so the advice is to close the other one, not to restart this.
fn explain(error: &str, bind: std::net::SocketAddr) -> String {
    if error.contains("10048") || error.contains("address in use") {
        return format!(
            "Something is already using port {} — another agent may be running. Close it; this window reconnects by itself.",
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
            "show" => show_window(app),
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

/// Bring the window forward, and give a downed listener a nudge while it is
/// there.
///
/// One function for the tray, a second launch and an approval, so they cannot
/// drift: shown but still minimized, or shown but not focused, is the kind of
/// half-state each caller used to get wrong on its own. The nudge is here as
/// well as on the focus event because `set_focus` on a window that already has
/// focus raises no event — and that is still someone looking at the window.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        // Hidden while minimized, it would come back minimized.
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(state) = app.try_state::<state::ShellState>() {
        state.kick();
    }
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
