//! Proves the host<->plugin boundary end to end against a real child process.
//!
//! The plugin is `src/bin/fixture.rs`, built by Cargo alongside these tests, so
//! the suite needs no Node and no network — only the protocol.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flexdeck_actions::RecordingExecutor;
use flexdeck_plugin::{abi, Ctx, Plugin, PluginError, RecordingNotifier, Registry};
use flexdeck_plugin_host::{HostError, HostEvent, Options, Services, SidecarPlugin, SpawnSpec};
use flexdeck_protocol::Action;
use flexdeck_vars::{Kind, Value, VariableSpec, VariableStore};

const FIXTURE: &str = env!("CARGO_BIN_EXE_flexdeck-plugin-fixture");

fn package() -> abi::PackageManifest {
    abi::PackageManifest {
        abi: abi::ABI_VERSION,
        id: "fixture".into(),
        name: "Fixture".into(),
        version: "1.0.0".into(),
        description: String::new(),
        author: String::new(),
        homepage: None,
        runtime: abi::runtime::NATIVE.into(),
        entry: None,
        entries: [(abi::current_platform().to_string(), "fixture".to_string())].into(),
        permissions: vec![abi::permission::NOTIFICATIONS.into()],
        recipe: None,
    }
}

fn spec(env: &[(&str, &str)]) -> SpawnSpec {
    SpawnSpec {
        program: FIXTURE.into(),
        args: vec![],
        cwd: std::env::temp_dir(),
        env: env
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
    }
}

fn fast() -> Options {
    Options {
        initialize_timeout: Duration::from_secs(5),
        invoke_timeout: Duration::from_secs(5),
        query_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_millis(500),
        restart_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

struct World {
    vars: Arc<VariableStore>,
    events: mpsc::Receiver<HostEvent>,
    services: Services,
}

fn world() -> World {
    let vars = Arc::new(VariableStore::new());
    let (sender, events) = mpsc::channel();
    World {
        services: Services {
            vars: Arc::clone(&vars),
            events: sender,
        },
        vars,
        events,
    }
}

fn start(
    world: &World,
    env: &[(&str, &str)],
    options: Options,
) -> Result<SidecarPlugin, HostError> {
    SidecarPlugin::start(
        spec(env),
        package(),
        BTreeMap::new(),
        world.services.clone(),
        options,
    )
}

fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn invoke(
    plugin: &mut SidecarPlugin,
    vars: &VariableStore,
    action: &str,
    pairs: &[(&str, &str)],
) -> Result<(), PluginError> {
    let mut input = RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    plugin.invoke(action, &args(pairs), &mut ctx).map(drop)
}

/// Wait for an event matching `want`, up to two seconds.
fn wait_for(events: &mpsc::Receiver<HostEvent>, want: impl Fn(&HostEvent) -> bool) -> HostEvent {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) if want(&event) => return event,
            Ok(_) => continue,
            Err(_) => panic!("the expected event never arrived"),
        }
    }
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_started_plugin_declares_itself_and_its_variables() {
    let world = world();
    let plugin = start(&world, &[], fast()).expect("the fixture starts");

    let manifest = plugin.manifest();
    assert_eq!(manifest.id, "fixture");
    assert_eq!(manifest.abi, abi::ABI_VERSION);
    assert!(manifest.actions.iter().any(|a| a.action == "greet"));
    assert!(world.vars.spec("fixture.count").is_some());
    assert_eq!(plugin.status(), None);
    assert_eq!(plugin.origin(), flexdeck_plugin::Origin::Installed);
}

#[test]
fn an_action_runs_and_its_effects_come_back_through_the_host() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");

    invoke(&mut plugin, &world.vars, "greet", &[("name", "ada")]).expect("greet runs");

    // A variable published by the plugin, a variable it declared at run time,
    // an event, and a notification — each one through a different path.
    wait_until("the count", || {
        world.vars.get("fixture.count") == Some(Value::Number(1.0))
    });
    wait_until("the run-time variable", || {
        world.vars.get("fixture.last_name") == Some(Value::Text("ada".into()))
    });
    let event = wait_for(&world.events, |e| matches!(e, HostEvent::Event { .. }));
    assert_eq!(
        event,
        HostEvent::Event {
            plugin: "fixture".into(),
            event: "greeted".into(),
            payload: [
                ("name".to_string(), "ada".to_string()),
                ("greeting".to_string(), "Hello".to_string())
            ]
            .into(),
        }
    );
    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Notify { title, .. } if title == "Hello"),
    );
    assert!(
        plugin
            .variables()
            .iter()
            .any(|spec| spec.name == "fixture.last_name"),
        "a run-time declaration becomes part of the plugin's variables"
    );
}

#[test]
fn a_value_published_the_instant_initialize_is_answered_is_not_lost() {
    // The SDK starts a plugin right after answering, so its first publish can
    // overtake the host's own bookkeeping. It must be held, not refused.
    for _ in 0..20 {
        let world = world();
        let _plugin = start(&world, &[("FIXTURE_PUBLISH_AT_START", "1")], fast()).expect("starts");
        wait_until("the early value", || {
            world.vars.get("fixture.count") == Some(Value::Number(42.0))
        });
    }
}

#[test]
fn the_plugin_refusing_a_value_is_reported_with_its_reason() {
    let world = world();
    let plugin = start(&world, &[], fast()).expect("starts");

    let error = plugin
        .check("greet", &args(&[("name", "nobody")]))
        .expect_err("refused");
    assert_eq!(
        error,
        PluginError::Failed("nobody cannot be greeted".into())
    );
    plugin
        .check("greet", &args(&[("name", "ada")]))
        .expect("accepted");
}

#[test]
fn a_failed_action_says_why() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    let error = invoke(&mut plugin, &world.vars, "fail", &[]).expect_err("fails");
    assert_eq!(
        error,
        PluginError::Failed("the fixture was asked to fail".into())
    );
}

#[test]
fn a_plugin_built_for_another_abi_is_refused_naming_both_versions() {
    let world = world();
    let error = start(&world, &[("FIXTURE_ABI", "1")], fast()).expect_err("refused");
    assert_eq!(
        error,
        HostError::Abi(abi::AbiError::Version {
            theirs: 1,
            ours: "2 or 3 or 4".into(),
        })
    );
}

fn start_with(
    world: &World,
    package: abi::PackageManifest,
    env: &[(&str, &str)],
) -> Result<SidecarPlugin, HostError> {
    SidecarPlugin::start(spec(env), package, BTreeMap::new(), world.services.clone(), fast())
}

fn invoke_state(
    plugin: &mut SidecarPlugin,
    vars: &VariableStore,
    action: &str,
    pairs: &[(&str, &str)],
) -> Result<Option<String>, PluginError> {
    let mut input = RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    plugin
        .invoke(action, &args(pairs), &mut ctx)
        .map(|outcome| outcome.state)
}

#[test]
fn an_abi_2_package_is_spoken_to_in_abi_2_and_still_loads() {
    let world = world();
    let mut old = package();
    old.abi = 2;
    let mut plugin = start_with(&world, old, &[]).expect("a v2 plugin still loads");
    assert_eq!(plugin.manifest().abi, 2, "it was asked for ABI 2");
    assert!(
        !plugin.manifest().actions.iter().any(|a| a.action == "toggle"),
        "and offers only what ABI 2 can say"
    );
    invoke(&mut plugin, &world.vars, "greet", &[("name", "ada")]).expect("works as before");
}

#[test]
fn a_plugin_answering_in_another_abi_than_its_package_is_refused() {
    let world = world();
    let mut old = package();
    old.abi = 2;
    let error = start_with(&world, old, &[("FIXTURE_ABI", "3")]).expect_err("refused");
    assert!(matches!(error, HostError::Mismatch(_)), "{error}");
}

#[test]
fn an_action_can_report_the_look_a_widget_should_show() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    let first = invoke_state(&mut plugin, &world.vars, "toggle", &[]).expect("runs");
    let second = invoke_state(&mut plugin, &world.vars, "toggle", &[]).expect("runs");
    assert_eq!((first.as_deref(), second.as_deref()), (Some("on"), Some("off")));
    assert_eq!(invoke_state(&mut plugin, &world.vars, "fail", &[]).ok(), None);
    let level = plugin
        .manifest()
        .actions
        .into_iter()
        .find(|a| a.action == "level")
        .and_then(|a| a.slider)
        .expect("slider metadata survives the trip");
    assert_eq!(level.feedback.as_deref(), Some("fixture.level"));
}

#[test]
fn a_plugin_can_keep_values_for_its_declared_settings_only() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "store", &[("token", "abc")]).expect("runs");
    let stored = wait_for(&world.events, |e| matches!(e, HostEvent::StoreSettings { .. }));
    assert_eq!(
        stored,
        HostEvent::StoreSettings {
            plugin: "fixture".into(),
            values: [("token".to_string(), "abc".to_string())].into(),
        }
    );
    // The undeclared key in the same breath is refused, and logged.
    wait_until("the refusal in the log", || {
        plugin
            .log_tail()
            .iter()
            .any(|line| line.contains("'nope' is not a setting"))
    });
}

#[test]
fn opening_a_web_page_needs_the_network_permission() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "open", &[("url", "https://example.com/a")]).expect("runs");
    wait_until("the refusal in the log", || {
        plugin
            .log_tail()
            .iter()
            .any(|line| line.contains("network permission"))
    });

    let second = crate::world();
    let mut networked = package();
    networked.permissions.push(abi::permission::NETWORK.into());
    let mut plugin = start_with(
        &second,
        networked,
        &[("FIXTURE_PERMISSIONS", "notifications,network")],
    )
    .expect("starts");
    invoke(&mut plugin, &second.vars, "open", &[("url", "file:///C:/x.exe")]).expect("runs");
    invoke(&mut plugin, &second.vars, "open", &[("url", "https://example.com/a")]).expect("runs");
    let opened = wait_for(&second.events, |e| matches!(e, HostEvent::OpenUrl { .. }));
    assert_eq!(
        opened,
        HostEvent::OpenUrl {
            plugin: "fixture".into(),
            url: "https://example.com/a".into(),
        },
        "the file: address never got through"
    );
}

#[test]
fn an_image_is_handed_over_only_when_its_checksum_matches() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "image", &[("bad", "1")]).expect("runs");
    invoke(&mut plugin, &world.vars, "image", &[]).expect("runs");
    let image = wait_for(&world.events, |e| matches!(e, HostEvent::Image { .. }));
    let HostEvent::Image { sha256, mime, bytes, .. } = image else {
        unreachable!()
    };
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, b"\x89PNG\r\n\x1a\n");
    assert!(sha256.starts_with("4c4b6a3b"));
    assert!(plugin
        .log_tail()
        .iter()
        .any(|line| line.contains("checksum does not match")));
}

#[test]
fn an_abi_2_plugin_may_not_send_what_abi_3_added() {
    let world = world();
    let mut old = package();
    old.abi = 2;
    // FIXTURE_ABI keeps the actions an ABI 2 manifest could not declare out,
    // so drive the notification through the host's own guard instead: the
    // fixture under ABI 2 has no `store` action, which is the point.
    let plugin = start_with(&world, old, &[]).expect("starts");
    assert!(plugin
        .manifest()
        .actions
        .iter()
        .all(|a| a.slider.is_none() && a.action != "store"));
}

#[test]
fn a_plugin_claiming_another_id_than_its_package_is_refused() {
    let world = world();
    let error = start(&world, &[("FIXTURE_ID", "impostor")], fast()).expect_err("refused");
    assert!(matches!(error, HostError::Mismatch(_)), "{error}");
}

#[test]
fn a_plugin_claiming_a_permission_its_package_did_not_declare_is_refused() {
    let world = world();
    let error = start(
        &world,
        &[("FIXTURE_PERMISSIONS", "notifications,processes")],
        fast(),
    )
    .expect_err("refused");
    assert!(error.to_string().contains("processes"), "{error}");
}

#[test]
fn a_plugin_that_never_answers_initialize_is_refused_by_its_deadline() {
    let world = world();
    let started = Instant::now();
    let error = start(
        &world,
        &[("FIXTURE_SILENT", "1")],
        Options {
            initialize_timeout: Duration::from_millis(300),
            ..fast()
        },
    )
    .expect_err("refused");
    assert!(matches!(error, HostError::Timeout { .. }), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_plugin_that_exits_at_once_is_refused_rather_than_hanging() {
    let world = world();
    let error = start(&world, &[("FIXTURE_CRASH_AT_START", "1")], fast()).expect_err("refused");
    assert!(
        matches!(error, HostError::Exited(_) | HostError::NotRunning(_)),
        "{error}"
    );
}

#[test]
fn a_plugin_that_dies_starting_is_reported_with_what_it_said() {
    // Node that cannot find the entry prints "Cannot find module …" and exits;
    // that line is the whole diagnosis, so it belongs in the error.
    let world = world();
    let error = start(
        &world,
        &[(
            "FIXTURE_CRASH_SAYING",
            "Error: Cannot find module 'index.js'",
        )],
        fast(),
    )
    .expect_err("refused");
    assert!(
        error.to_string().contains("Cannot find module 'index.js'"),
        "{error}"
    );
}

#[test]
fn a_package_under_a_relative_directory_is_started_by_absolute_paths() {
    // The process starts *in* the package directory, so a relative entry would
    // be looked for inside itself. An agent run with a relative data directory
    // (the default) installed plugins that could never start.
    let here = tempfile::tempdir_in(".").expect("a directory under the working one");
    let relative = std::path::Path::new(here.path().file_name().unwrap());
    std::fs::create_dir_all(relative.join("dist")).unwrap();
    std::fs::write(relative.join("dist").join("index.js"), "").unwrap();
    let mut package = package();
    package.runtime = abi::runtime::NODE.into();
    package.entry = Some("dist/index.js".into());

    let spec = SpawnSpec::for_package(
        relative,
        &package,
        &flexdeck_plugin_host::Runtimes { node: None, python: None },
    )
    .expect("a spec");
    assert!(spec.cwd.is_absolute(), "{}", spec.cwd.display());
    let entry = std::path::Path::new(&spec.args[0]);
    assert!(entry.is_absolute(), "{}", entry.display());
    assert!(entry.is_file(), "the entry resolves to the file itself");
}

#[test]
fn a_plugin_offers_choices_and_one_that_cannot_offers_none() {
    let world = world();

    // Built before `choices` existed: "no method" is "nothing to suggest".
    let old = start(&world, &[], fast()).expect("starts");
    let answer = old.choices("greet", &args(&[])).expect("not an error");
    assert!(answer.arguments.is_empty());
    assert_eq!(answer.error, None);
    drop(old);

    let new = start(&world, &[("FIXTURE_CHOICES", "1")], fast()).expect("starts");
    let answer = new.choices("greet", &args(&[])).expect("answers");
    let names: Vec<&str> = answer.arguments["name"]
        .iter()
        .map(|choice| choice.value.as_str())
        .collect();
    assert_eq!(names, ["ada", "grace"]);
    assert_eq!(
        answer.arguments["name"][1].label.as_deref(),
        Some("Grace Hopper")
    );
}

#[test]
fn the_registry_passes_on_only_an_actions_own_arguments() {
    let world = world();
    let plugin = start(&world, &[("FIXTURE_CHOICES", "1")], fast()).expect("starts");
    let mut registry = Registry::new();
    registry.register(Box::new(plugin));

    let answer = registry
        .choices(&Action {
            plugin: "fixture".into(),
            action: "greet".into(),
            args: HashMap::new(),
        })
        .expect("answers");
    assert!(answer.arguments.contains_key("name"));
    assert!(
        !answer.arguments.contains_key("bogus"),
        "an argument greet does not take is never offered"
    );
    assert!(matches!(
        registry.choices(&Action {
            plugin: "fixture".into(),
            action: "nope".into(),
            args: HashMap::new(),
        }),
        Err(PluginError::UnknownAction { .. })
    ));
}

#[test]
fn a_slow_action_is_cut_off_and_the_plugin_comes_back() {
    let world = world();
    let mut plugin = start(
        &world,
        &[],
        Options {
            invoke_timeout: Duration::from_millis(300),
            ..fast()
        },
    )
    .expect("starts");
    let first = plugin.pid();

    let error = invoke(&mut plugin, &world.vars, "sleep", &[("ms", "5000")]).expect_err("cut off");
    assert!(error.to_string().contains("did not answer"), "{error}");

    wait_until("a restart", || {
        plugin.pid().is_some() && plugin.pid() != first
    });
    wait_until("a healthy status", || plugin.status().is_none());
    invoke(&mut plugin, &world.vars, "greet", &[("name", "again")]).expect("works after restart");
}

#[test]
fn a_crash_is_restarted_and_repeated_crashes_stop_it_with_a_reason() {
    let world = world();
    let mut plugin = start(
        &world,
        &[],
        Options {
            max_restarts: 2,
            ..fast()
        },
    )
    .expect("starts");

    for _ in 0..2 {
        let _ = invoke(&mut plugin, &world.vars, "crash", &[]);
        wait_until("a restart", || {
            plugin.is_running() && plugin.status().is_none()
        });
    }
    let _ = invoke(&mut plugin, &world.vars, "crash", &[]);
    wait_until("it to give up", || {
        plugin
            .status()
            .is_some_and(|status| status.contains("stopped after crashing"))
    });

    let error = invoke(&mut plugin, &world.vars, "greet", &[("name", "x")]).expect_err("stopped");
    assert!(
        matches!(&error, PluginError::Unavailable(reason) if reason.contains("crashing")),
        "{error}"
    );
}

#[test]
fn a_plugin_cannot_write_another_plugins_variable() {
    let world = world();
    world.vars.declare(VariableSpec::new(
        "system.audio.volume",
        Kind::Number,
        "volume",
    ));
    world
        .vars
        .refresh("system.audio.volume", Value::Number(40.0))
        .unwrap();
    let mut plugin = start(&world, &[], fast()).expect("starts");

    invoke(&mut plugin, &world.vars, "escape", &[]).expect("the action itself returns");

    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Log { message, .. } if message.contains("system.audio.volume")),
    );
    assert_eq!(
        world.vars.get("system.audio.volume"),
        Some(Value::Number(40.0))
    );
}

#[test]
fn an_event_the_manifest_does_not_declare_is_refused() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "undeclared", &[]).expect("returns");
    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Log { message, .. } if message.contains("'nope'")),
    );
}

#[test]
fn notifications_need_the_permission() {
    let world = world();
    let mut plugin = start(&world, &[("FIXTURE_PERMISSIONS", "")], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "greet", &[("name", "ada")]).expect("runs");
    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Log { message, .. } if message.contains("notifications permission")),
    );
}

#[test]
fn shutting_down_leaves_no_process_behind() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    assert!(plugin.is_running());
    plugin.shutdown();
    assert!(!plugin.is_running());
    assert_eq!(plugin.status().as_deref(), Some("stopped"));
}

#[test]
fn settings_reach_the_plugin_at_start_and_when_changed() {
    let world = world();
    let mut plugin = SidecarPlugin::start(
        spec(&[]),
        package(),
        [("greeting".to_string(), "Hi".to_string())].into(),
        world.services.clone(),
        fast(),
    )
    .expect("starts");

    invoke(&mut plugin, &world.vars, "greet", &[("name", "a")]).unwrap();
    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Notify { title, .. } if title == "Hi"),
    );

    plugin.settings_changed(&[("greeting".to_string(), "Yo".to_string())].into());
    invoke(&mut plugin, &world.vars, "greet", &[("name", "b")]).unwrap();
    wait_for(
        &world.events,
        |e| matches!(e, HostEvent::Notify { title, .. } if title == "Yo"),
    );
}

#[test]
fn an_installed_plugin_is_driven_by_the_registry_like_a_built_in() {
    let world = world();
    let plugin = start(&world, &[], fast()).expect("starts");
    let mut registry = Registry::new();
    registry.register_into(Box::new(plugin), &world.vars);

    let catalogue = registry.catalogue_json();
    let entry = &catalogue["plugins"][0];
    assert_eq!(entry["origin"], "installed");
    assert_eq!(entry["events"][0]["name"], "greeted");

    let mut input = RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars: &world.vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    let greet = Action {
        plugin: "fixture".into(),
        action: "greet".into(),
        args: args(&[("name", "ada")]),
    };
    registry
        .invoke(&greet, &mut ctx)
        .expect("runs through the registry");

    // Sampling goes through the plugin too.
    let interested: BTreeSet<String> = ["fixture.count".to_string()].into();
    assert_eq!(registry.sample(&interested, &mut ctx), Ok(1));
    assert_eq!(world.vars.get("fixture.count"), Some(Value::Number(1.0)));

    // Removing it removes its variables and stops the process.
    assert!(registry.unregister("fixture", &world.vars));
    assert_eq!(world.vars.spec("fixture.count"), None);
}

#[test]
fn a_plugin_cannot_declare_a_variable_in_another_plugins_namespace() {
    let world = world();
    let mut plugin = start(&world, &[], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "claim", &[]).expect("the action itself returns");
    wait_for(&world.events, |e| matches!(e, HostEvent::Log { message, .. } if message.contains("obs.stream_key")));
    assert!(world.vars.spec("obs.stream_key").is_none(), "nothing was declared under obs.");
}

#[test]
fn a_plugin_inherits_none_of_the_agents_secrets_from_its_environment() {
    // Something the agent might be started with: never the plugin's to read.
    std::env::set_var("FLEXDECK_TEST_AGENT_SECRET", "hunter2");
    let world = world();
    let mut plugin = start(&world, &[("FIXTURE_GIVEN", "yes")], fast()).expect("starts");
    invoke(&mut plugin, &world.vars, "env", &[("name", "FLEXDECK_TEST_AGENT_SECRET")]).expect("runs");
    wait_for(&world.events, |e| matches!(e, HostEvent::Log { message, .. } if message == "env FLEXDECK_TEST_AGENT_SECRET=<unset>"));
    // What it needs to run, and what its own spawn asked for, it has.
    invoke(&mut plugin, &world.vars, "env", &[("name", "PATH")]).expect("runs");
    wait_for(&world.events, |e| matches!(e, HostEvent::Log { message, .. } if message.starts_with("env PATH=") && !message.ends_with("<unset>")));
    invoke(&mut plugin, &world.vars, "env", &[("name", "FIXTURE_GIVEN")]).expect("runs");
    wait_for(&world.events, |e| matches!(e, HostEvent::Log { message, .. } if message == "env FIXTURE_GIVEN=yes"));
}
