//! Installing, updating and removing real plugin packages while running.
//!
//! This binary has its own `main` (`harness = false`) because it plays two
//! parts. Run normally, it is the test suite. Started by the plugin host — which
//! sets `FLEXDECK_PLUGIN_ID` — it is the *plugin*: each test package is a
//! "node" package whose runtime is this very executable, and whose entry file
//! holds `KEY=VALUE` lines that steer the fixture. So the whole install path is
//! exercised against real zips and real processes, with no Node installed.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flexdeck_plugin::{abi, Ctx, Origin, RecordingNotifier, Registry};
use flexdeck_plugin_host::{Options, Runtimes};
use flexdeck_protocol::Action;
use flexdeck_server::plugins::{PluginManager, PluginsError, Source};
use flexdeck_store::ConfigStore;
use flexdeck_vars::{Value, VariableStore};

fn main() {
    if std::env::var_os("FLEXDECK_PLUGIN_ID").is_some() {
        // The host started us as a plugin: apply the entry file's settings,
        // then speak the protocol.
        if let Some(entry) = std::env::args().nth(1) {
            for line in std::fs::read_to_string(entry).unwrap_or_default().lines() {
                if let Some((key, value)) = line.split_once('=') {
                    std::env::set_var(key.trim(), value.trim());
                }
            }
        }
        flexdeck_plugin_host::fixture::serve();
        return;
    }

    let tests: &[(&str, fn())] = &[
        (
            "an install needs consent to every permission",
            install_needs_consent,
        ),
        (
            "an installed plugin runs like a built-in",
            installed_plugin_runs,
        ),
        (
            "a recipe installs as a plugin, runs no process, and leaves with its sources",
            recipe_installs_and_leaves,
        ),
        (
            "a recipe the editor packed installs and keeps its secret secret",
            editor_recipe_installs,
        ),
        (
            "verifying passes a plugin that keeps the rules, and names each one broken",
            verify_names_what_is_broken,
        ),
        (
            "a checksum mismatch is refused",
            checksum_mismatch_is_refused,
        ),
        (
            "an entry that climbs out of the package is refused",
            traversal_is_refused,
        ),
        ("a built-in cannot be replaced", built_in_cannot_be_replaced),
        (
            "an update swaps builds and removes the old one",
            update_swaps_builds,
        ),
        (
            "a failed update keeps the old build running",
            failed_update_keeps_old,
        ),
        (
            "uninstall removes everything but the settings",
            uninstall_removes_everything,
        ),
        (
            "disable and enable without reinstalling",
            disable_and_enable,
        ),
        (
            "a restart loads what was enabled and reports what failed",
            restart_loads_enabled,
        ),
        (
            "a linked plugin reloads when its files change",
            dev_link_hot_reloads,
        ),
        (
            "the packed template installs and runs on real node",
            real_template_runs,
        ),
        (
            "a plugin installs and updates from a repository",
            repository_install_and_update,
        ),
        (
            "a repository that serves the wrong package is refused",
            repository_lies_are_refused,
        ),
        ("obs installs from a live repository", obs_from_a_repository),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let outcome = std::panic::catch_unwind(test);
        match outcome {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed += 1;
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        tests.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    registry: Arc<Mutex<Registry>>,
    vars: Arc<VariableStore>,
    store: Arc<Mutex<ConfigStore>>,
    manager: Arc<PluginManager>,
}

fn options() -> Options {
    Options {
        shutdown_grace: Duration::from_millis(300),
        restart_delay: Duration::from_millis(50),
        ..Options::default()
    }
}

fn runtimes() -> Runtimes {
    Runtimes {
        node: Some(std::env::current_exe().expect("the test executable")),
        python: None,
    }
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path().join("plugins");
    let registry = Arc::new(Mutex::new(Registry::new()));
    for plugin in
        flexdeck_plugin::builtin::builtins(flexdeck_plugin::builtin::Backends::recording())
    {
        registry.lock().unwrap().register(plugin);
    }
    let vars = Arc::new(VariableStore::new());
    let store = Arc::new(Mutex::new(
        ConfigStore::open(dir.path().join("db")).expect("a store"),
    ));
    let (manager, events) = PluginManager::new(
        root.clone(),
        Arc::clone(&registry),
        Arc::clone(&vars),
        Arc::clone(&store),
        runtimes(),
        options(),
    );
    // Drain host events so the channel never backs up.
    std::thread::spawn(move || for _ in events {});
    World {
        _dir: dir,
        root,
        registry,
        vars,
        store,
        manager: Arc::new(manager),
    }
}

impl World {
    /// The same data, a fresh manager — what an agent restart looks like.
    fn restart(&self) -> Arc<PluginManager> {
        let mut registry = self.registry.lock().unwrap();
        for id in ["fixture", "other"] {
            registry.unregister(id, &self.vars);
        }
        drop(registry);
        let (manager, events) = PluginManager::new(
            self.root.clone(),
            Arc::clone(&self.registry),
            Arc::clone(&self.vars),
            Arc::clone(&self.store),
            runtimes(),
            options(),
        );
        std::thread::spawn(move || for _ in events {});
        Arc::new(manager)
    }
}

fn package_json(id: &str, version: &str, permissions: &[&str]) -> String {
    serde_json::to_string(&abi::PackageManifest {
        abi: abi::ABI_VERSION,
        id: id.into(),
        name: "Fixture".into(),
        version: version.into(),
        description: "A test package".into(),
        author: "tests".into(),
        homepage: None,
        runtime: abi::runtime::NODE.into(),
        entry: Some("index.js".into()),
        entries: BTreeMap::new(),
        permissions: permissions.iter().map(|p| (*p).to_string()).collect(),
        recipe: None,
    })
    .unwrap()
}

/// A zip with the given files.
fn zip(files: &[(&str, &str)]) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    buffer.into_inner()
}

/// The fixture as a package. `steer` becomes the entry file's lines.
fn fixture(version: &str, steer: &str) -> Vec<u8> {
    zip(&[
        (
            "plugin.json",
            &package_json("fixture", version, &["notifications"]),
        ),
        ("index.js", steer),
    ])
}

fn consent() -> Vec<String> {
    vec!["notifications".to_string()]
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn greet(world: &World) -> Result<(), flexdeck_plugin::PluginError> {
    let mut input = flexdeck_actions::RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars: &world.vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    let action = Action {
        plugin: "fixture".into(),
        action: "greet".into(),
        args: [("name".to_string(), "ada".to_string())].into(),
    };
    world
        .registry
        .lock()
        .unwrap()
        .invoke(&action, &mut ctx)
        .map(drop)
}

fn install_needs_consent() {
    let world = world();
    let error = world
        .manager
        .install(&fixture("1.0.0", ""), None, &[], Source::File)
        .expect_err("no consent given");
    let PluginsError::Permissions {
        missing, package, ..
    } = error
    else {
        panic!("expected a consent error, got {error}");
    };
    assert_eq!(missing, vec!["notifications".to_string()]);
    assert_eq!(package.id, "fixture");
    assert!(
        !world.registry.lock().unwrap().contains("fixture"),
        "nothing ran"
    );
    assert!(
        world.manager.installed().is_empty(),
        "nothing was remembered"
    );
}

fn recipe_installs_and_leaves() {
    let world = world();
    let sources = Arc::new(flexdeck_server::sources::Sources::new(Arc::clone(&world.vars)));
    world.manager.host_recipes(Arc::clone(&sources));
    let package = r#"{"abi": 4, "id": "weather", "name": "Weather", "version": "1.0.0",
        "runtime": "recipe", "recipe": "recipe.json", "permissions": ["network"]}"#;
    let recipe = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/weather/recipe.json")).unwrap();
    let bytes = zip(&[("plugin.json", package), ("recipe.json", &recipe)]);

    let refused = world.manager.install(&bytes, None, &[], Source::File);
    assert!(matches!(refused, Err(PluginsError::Permissions { .. })), "the network permission is asked for");

    world
        .manager
        .install(&bytes, None, &["network".to_string()], Source::File)
        .expect("installs");
    let catalogue = world.registry.lock().unwrap().catalogue_json();
    let weather = catalogue["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "weather").unwrap().clone();
    assert_eq!(weather["presets"][0]["name"], "Weather", "its preset joins the gallery");
    assert_eq!(sources.recipes()["weather"][0].id, "weather");

    world.manager.uninstall("weather").expect("uninstalls");
    assert!(sources.recipes().is_empty(), "its sources leave with it");
}

/// The package `pnpm gen:recipe` writes, as the editor's "Share as a
/// plugin" makes it: its own zip, its own manifest and recipe.
fn editor_recipe_installs() {
    let world = world();
    let sources = Arc::new(flexdeck_server::sources::Sources::new(Arc::clone(&world.vars)));
    world.manager.host_recipes(Arc::clone(&sources));
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/fixtures/plugin/recipe-from-editor.fdpkg")).unwrap();
    world
        .manager
        .install(&bytes, None, &["network".to_string()], Source::File)
        .expect("the editor's package installs");
    let catalogue = world.registry.lock().unwrap().catalogue_json();
    let mine = catalogue["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "my-weather").unwrap().clone();
    assert_eq!(mine["presets"][0]["name"], "My weather");
    let source = &sources.recipes()["my-weather"][0];
    assert_eq!(source.id, "my-weather");
    let key = source.args.iter().find(|a| a.name == "key").unwrap();
    assert!(key.secret && key.default.is_empty(), "a secret travels blank");
    world.manager.uninstall("my-weather").expect("uninstalls");
}

/// `flexdeck verify-plugin`, on a folder: the fixture, behaving and not.
fn verify_names_what_is_broken() {
    use flexdeck_server::verify::{verify, VerifyOptions};
    let options = VerifyOptions { runtimes: Some(runtimes()), ..VerifyOptions::default() };
    let folder = |steer: &str| {
        let dir = std::env::temp_dir().join(format!("fd-verify-{}-{}", std::process::id(), steer.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.json"), package_json("fixture", "1.0.0", &["notifications"])).unwrap();
        std::fs::write(dir.join("index.js"), steer).unwrap();
        dir
    };
    let failed = |steer: &str| -> Vec<String> {
        let dir = folder(steer);
        let report = verify(&dir, &options);
        let _ = std::fs::remove_dir_all(&dir);
        report.checks.iter().filter(|c| !c.ok).map(|c| format!("{}: {}", c.name, c.detail)).collect()
    };

    let good = failed("");
    assert!(good.is_empty(), "a plugin that keeps the rules passes: {good:?}");

    let escapes = failed("FIXTURE_SAMPLE_ESCAPES=1");
    assert!(escapes.iter().any(|f| f.starts_with("every value it sends is declared") && f.contains("system.audio.volume")), "{escapes:?}");

    let chatty = failed("FIXTURE_CHATTY=1
");
    assert!(chatty.iter().any(|f| f.contains("stdout") && f.contains("debug: sampled")), "{chatty:?}");

    let refuses = failed("FIXTURE_REFUSE_EXAMPLES=1

");
    assert!(refuses.iter().any(|f| f.starts_with("it accepts every action's example arguments") && f.contains("refuses everything")), "{refuses:?}");

    let broken = failed("FIXTURE_CRASH_AT_START=1


");
    assert!(broken.iter().any(|f| f.starts_with("it starts and introduces itself")), "{broken:?}");
}

fn installed_plugin_runs() {
    let world = world();
    let installed = world
        .manager
        .install(&fixture("1.0.0", ""), None, &consent(), Source::File)
        .expect("installs");
    assert!(installed.path.starts_with(&world.root));
    assert_eq!(
        world.registry.lock().unwrap().origin("fixture"),
        Some(Origin::Installed)
    );
    greet(&world).expect("greet runs through the registry");
    wait_until("the published count", || {
        world.vars.get("fixture.count") == Some(Value::Number(1.0))
    });
    let listed = world.manager.installed_json();
    assert_eq!(listed["plugins"][0]["running"], true);
}

fn checksum_mismatch_is_refused() {
    let world = world();
    let error = world
        .manager
        .install(
            &fixture("1.0.0", ""),
            Some("00ff"),
            &consent(),
            Source::File,
        )
        .expect_err("wrong checksum");
    assert!(matches!(error, PluginsError::Checksum { .. }), "{error}");
}

fn traversal_is_refused() {
    let world = world();
    let bytes = zip(&[
        (
            "plugin.json",
            &package_json("fixture", "1.0.0", &["notifications"]),
        ),
        ("index.js", ""),
        ("../escaped.txt", "gotcha"),
    ]);
    let error = world
        .manager
        .install(&bytes, None, &consent(), Source::File)
        .expect_err("refused");
    assert!(error.to_string().contains("outside"), "{error}");
    assert!(!world.root.parent().unwrap().join("escaped.txt").exists());
    assert!(!world.registry.lock().unwrap().contains("fixture"));
}

fn built_in_cannot_be_replaced() {
    let world = world();
    let bytes = zip(&[
        ("plugin.json", &package_json("system", "9.9.9", &[])),
        ("index.js", ""),
    ]);
    let error = world
        .manager
        .install(&bytes, None, &[], Source::File)
        .expect_err("refused");
    assert!(matches!(error, PluginsError::BuiltIn(_)), "{error}");
}

fn update_swaps_builds() {
    let world = world();
    let first = world
        .manager
        .install(
            &fixture("1.0.0", "# build a"),
            None,
            &consent(),
            Source::File,
        )
        .expect("v1");
    let second = world
        .manager
        .install(
            &fixture("1.0.0", "# build b"),
            None,
            &consent(),
            Source::File,
        )
        .expect("a different build");
    assert_ne!(first.path, second.path);
    assert!(!first.path.exists(), "the old build is removed");
    assert!(second.path.exists());
    assert_eq!(world.manager.installed().len(), 1);
    greet(&world).expect("the new build answers");
}

fn failed_update_keeps_old() {
    let world = world();
    let good = world
        .manager
        .install(&fixture("1.0.0", ""), None, &consent(), Source::File)
        .expect("installs");
    // A build that claims the wrong ABI is refused at `initialize`.
    let error = world
        .manager
        .install(
            &fixture("1.0.0", "FIXTURE_ABI=1"),
            None,
            &consent(),
            Source::File,
        )
        .expect_err("the broken build is refused");
    assert!(error.to_string().contains("ABI"), "{error}");
    assert_eq!(
        world.manager.installed()[0].path,
        good.path,
        "the record is unchanged"
    );
    assert!(good.path.exists());
    greet(&world).expect("the old build is still running");
}

fn uninstall_removes_everything() {
    let world = world();
    world
        .manager
        .install(&fixture("1.0.0", ""), None, &consent(), Source::File)
        .expect("installs");
    world
        .registry
        .lock()
        .unwrap()
        .set_settings(
            "fixture",
            [("greeting".to_string(), "Hey".to_string())].into(),
        )
        .unwrap();

    world.manager.uninstall("fixture").expect("uninstalls");

    assert!(!world.registry.lock().unwrap().contains("fixture"));
    assert!(world.manager.installed().is_empty());
    assert!(!world.root.join("fixture").exists());
    assert_eq!(world.vars.spec("fixture.count"), None);
    assert_eq!(
        world.registry.lock().unwrap().stored_settings("fixture")["greeting"],
        "Hey",
        "settings survive, so a reinstall picks up where it left off"
    );
    assert!(matches!(
        world.manager.uninstall("fixture"),
        Err(PluginsError::NotInstalled(_))
    ));
}

fn disable_and_enable() {
    let world = world();
    world
        .manager
        .install(&fixture("1.0.0", ""), None, &consent(), Source::File)
        .expect("installs");
    world
        .manager
        .set_enabled("fixture", false)
        .expect("disables");
    assert!(!world.registry.lock().unwrap().contains("fixture"));
    assert!(!world.manager.installed()[0].enabled);
    world.manager.set_enabled("fixture", true).expect("enables");
    greet(&world).expect("runs again");
}

fn restart_loads_enabled() {
    let world = world();
    world
        .manager
        .install(&fixture("1.0.0", ""), None, &consent(), Source::File)
        .expect("installs");
    // Break the build on disk behind the manager's back.
    let path = world.manager.installed()[0].path.clone();
    let manager = world.restart();
    manager.load_all();
    assert!(
        world.registry.lock().unwrap().contains("fixture"),
        "reloaded"
    );

    std::fs::write(path.join("index.js"), "FIXTURE_ABI=1").unwrap();
    let manager = world.restart();
    manager.load_all();
    assert!(!world.registry.lock().unwrap().contains("fixture"));
    let error = manager.failure("fixture").expect("the failure is recorded");
    assert!(error.contains("ABI"), "{error}");
}

fn dev_link_hot_reloads() {
    let world = world();
    let source = world.root.parent().unwrap().join("working");
    std::fs::create_dir_all(&source).unwrap();
    write_files(
        &source,
        &[
            (
                "plugin.json",
                &package_json("fixture", "1.0.0", &["notifications"]),
            ),
            ("index.js", ""),
        ],
    );
    world.manager.link_dev(&source).expect("links");
    world.manager.watch_dev();
    greet(&world).expect("runs from the working directory");

    let before = world.registry.lock().unwrap().generation();
    std::thread::sleep(Duration::from_millis(1_100));
    std::fs::write(source.join("index.js"), "# edited").unwrap();
    wait_until("a reload", || {
        world.registry.lock().unwrap().generation() > before
    });
    wait_until("the reloaded plugin", || greet(&world).is_ok());
    assert!(
        source.exists(),
        "uninstalling a link must never delete the user's code"
    );
    world.manager.uninstall("fixture").unwrap();
    assert!(source.join("index.js").exists());
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (name, content) in files {
        std::fs::write(root.join(name), content).unwrap();
    }
}

/// The real thing: `plugins/template` packed by the TypeScript SDK, installed
/// by this manager, started by the real Node, and driven through the registry.
/// Skipped (and said so) when either is missing, since neither is committed.
fn real_template_runs() {
    let package =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/template/template-0.1.0.fdpkg");
    let node = std::process::Command::new("node").arg("--version").output();
    if !package.exists() || node.is_err() {
        println!("  (skipped: run `pnpm --filter @flexdeck/plugin-template run package` with node installed)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new()));
    let vars = Arc::new(VariableStore::new());
    let store = Arc::new(Mutex::new(
        ConfigStore::open(dir.path().join("db")).unwrap(),
    ));
    let (manager, events) = PluginManager::new(
        dir.path().join("plugins"),
        Arc::clone(&registry),
        Arc::clone(&vars),
        store,
        Runtimes { node: None, python: None },
        options(),
    );
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for event in events {
            let _ = sender.send(event);
        }
    });

    let bytes = std::fs::read(&package).unwrap();
    manager
        .install(&bytes, None, &consent(), Source::File)
        .expect("the template installs");

    let mut input = flexdeck_actions::RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars: &vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    let greet = Action {
        plugin: "template".into(),
        action: "greet".into(),
        args: [("name".to_string(), "ada".to_string())].into(),
    };
    registry
        .lock()
        .unwrap()
        .invoke(&greet, &mut ctx)
        .expect("greet runs on node");
    let blank = Action {
        args: [("name".to_string(), " ".to_string())].into(),
        ..greet.clone()
    };
    // Blank is caught by the registry before the plugin is even asked.
    let refused = registry.lock().unwrap().check(&blank).expect_err("refused");
    assert!(
        refused.to_string().contains("needs the argument 'name'"),
        "{refused}"
    );

    wait_until("the count from node", || {
        vars.get("template.greetings") == Some(Value::Number(1.0))
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_event = false;
    while Instant::now() < deadline && !saw_event {
        if let Ok(flexdeck_plugin_host::HostEvent::Event { event, payload, .. }) =
            received.recv_timeout(Duration::from_millis(200))
        {
            saw_event =
                event == "greeted" && payload.get("name").map(String::as_str) == Some("ada");
        }
    }
    assert!(saw_event, "the greeted event came back from node");
    manager.uninstall("template").unwrap();
}

/// A web server for files that a test can change while it runs.
struct Files {
    base: String,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl Files {
    fn serve() -> Self {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let files: Arc<Mutex<BTreeMap<String, Vec<u8>>>> = Arc::default();
        let served = Arc::clone(&files);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // Read the headers to their end; nothing in them matters here.
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = served.lock().unwrap().get(&path).cloned();
                let mut stream = stream;
                let _ = match body {
                    Some(body) => stream
                        .write_all(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .as_bytes(),
                        )
                        .and_then(|()| stream.write_all(&body)),
                    None => stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    ),
                };
            }
        });
        Self { base, files }
    }

    fn put(&self, path: &str, bytes: Vec<u8>) {
        self.files.lock().unwrap().insert(path.to_string(), bytes);
    }

    /// A repository listing one package, pinning `sha256` (its own, unless given).
    fn list(&self, version: &str, package: &str, sha256: Option<String>) {
        let bytes = self.files.lock().unwrap()[&format!("/{package}")].clone();
        let index = serde_json::json!({
            "schema": 1,
            "repo": { "name": "Test repo", "description": "for tests" },
            "plugins": [{
                "id": "fixture", "name": "Fixture", "version": version, "abi": 2,
                "author": "tests", "description": "A test package",
                "permissions": ["notifications"],
                "icon": "icon.png",
                "package": {
                    "url": package,
                    "sha256": sha256.unwrap_or_else(|| flexdeck_server::plugins::sha256_hex(&bytes)),
                    "size": bytes.len()
                }
            }]
        });
        self.put("/repository.min.json", index.to_string().into_bytes());
    }
}

const EMPTY_REPO: &[u8] = br#"{"schema":1,"repo":{"name":"Official"},"plugins":[]}"#;

fn repositories(world: &World, files: &Files) -> flexdeck_server::repos::Repositories {
    flexdeck_server::repos::Repositories::new(
        flexdeck_server::repos::Sources {
            official: format!("{}/official.json", files.base),
            vetted_list: None,
        },
        world.root.join(".repos"),
        Arc::clone(&world.store),
    )
}

fn repository_install_and_update() {
    let world = world();
    let files = Files::serve();
    files.put("/official.json", EMPTY_REPO.to_vec());
    files.put(
        "/icon.png",
        b"\x89PNG\r\n\x1a\nnot really an image".to_vec(),
    );
    files.put("/fixture-1.0.0.fdpkg", fixture("1.0.0", ""));
    files.put(
        "/fixture-1.1.0.fdpkg",
        fixture("1.1.0", "FIXTURE_VERSION=1.1.0"),
    );
    files.list("1.0.0", "fixture-1.0.0.fdpkg", None);
    let repos = repositories(&world, &files);

    // Added by host and port alone, the way a person would type it.
    let row = repos.add(&files.base).expect("adds");
    assert_eq!(row.name, "Test repo");
    assert_eq!(row.plugin_count, 1);
    let url = row.url.clone();
    assert!(url.ends_with("/repository.min.json"), "{url}");

    let catalog = repos.catalog("fixt", &world.manager.installed());
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0]["repo"]["kind"], "custom");
    assert!(
        catalog[0]["iconData"]
            .as_str()
            .unwrap_or("")
            .starts_with("data:image/png;base64,"),
        "{}",
        catalog[0]
    );
    assert!(catalog[0]["installedVersion"].is_null());

    let (bytes, sha256, source) = repos.fetch_package(&url, "fixture").expect("fetches");
    let installed = world
        .manager
        .install(&bytes, Some(&sha256), &consent(), source)
        .expect("installs");
    assert!(matches!(&installed.source, Source::Repository { url: from, .. } if *from == url));
    assert!(repos.updates(&world.manager.installed()).is_empty());

    files.list("1.1.0", "fixture-1.1.0.fdpkg", None);
    repos.refresh(true);
    let updates = repos.updates(&world.manager.installed());
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert_eq!(updates[0]["available"], "1.1.0");
    assert_eq!(
        repos.catalog("", &world.manager.installed())[0]["update"],
        true
    );

    let (bytes, sha256, source) = repos.fetch_package(&url, "fixture").unwrap();
    let updated = world
        .manager
        .install(&bytes, Some(&sha256), &consent(), source)
        .expect("updates");
    assert_eq!(updated.version, "1.1.0");
    assert!(repos.updates(&world.manager.installed()).is_empty());

    // Offline: the last good index is still there after a restart.
    files.put("/repository.min.json", b"gone".to_vec());
    let again = repositories(&world, &files);
    assert_eq!(again.catalog("", &[]).len(), 1, "read from disk");
    again.refresh(true);
    let row = again.list().into_iter().find(|row| row.url == url).unwrap();
    assert_eq!(row.plugin_count, 1, "the last good copy stays");
    assert!(row.error.is_some(), "and the failure is shown beside it");

    world.manager.uninstall("fixture").unwrap();
}

fn repository_lies_are_refused() {
    let world = world();
    let files = Files::serve();
    files.put("/official.json", EMPTY_REPO.to_vec());
    files.put("/fixture-1.0.0.fdpkg", fixture("1.0.0", ""));
    files.list("1.0.0", "fixture-1.0.0.fdpkg", Some("ab".repeat(32)));
    let repos = repositories(&world, &files);
    let url = repos.add(&files.base).unwrap().url;

    // The bytes are not what the index pinned.
    let (bytes, sha256, source) = repos.fetch_package(&url, "fixture").unwrap();
    let error = world
        .manager
        .install(&bytes, Some(&sha256), &consent(), source)
        .expect_err("refused");
    assert!(matches!(error, PluginsError::Checksum { .. }), "{error}");

    // The index says 2.0.0; the package says 1.0.0.
    files.list("2.0.0", "fixture-1.0.0.fdpkg", None);
    repos.refresh(true);
    let error = repos.fetch_package(&url, "fixture").expect_err("refused");
    assert!(
        error.to_string().contains("its package is fixture 1.0.0"),
        "{error}"
    );

    // A repository that is not one of ours is not installed from.
    assert!(repos
        .fetch_package("https://elsewhere.example/repository.min.json", "fixture")
        .is_err());
    assert!(world.manager.installed().is_empty());
}

/// Against a live repository: `FLEXDECK_E2E_REPO=<repository.min.json address>`.
/// Downloads OBS, checks the index's checksum, installs, and starts it on real
/// Node. With no OBS running it must come up anyway and say so.
fn obs_from_a_repository() {
    let Ok(repo) = std::env::var("FLEXDECK_E2E_REPO") else {
        println!("  (skipped: set FLEXDECK_E2E_REPO to a repository listing obs)");
        return;
    };
    let repo = flexdeck_server::repos::normalize_repo_url(&repo).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(Mutex::new(Registry::new()));
    let vars = Arc::new(VariableStore::new());
    let store = Arc::new(Mutex::new(
        ConfigStore::open(dir.path().join("db")).unwrap(),
    ));
    let (manager, events) = PluginManager::new(
        dir.path().join("plugins"),
        Arc::clone(&registry),
        Arc::clone(&vars),
        Arc::clone(&store),
        Runtimes { node: None, python: None },
        options(),
    );
    std::thread::spawn(move || for _ in events {});
    let repos = flexdeck_server::repos::Repositories::new(
        flexdeck_server::repos::Sources {
            official: repo.clone(),
            vetted_list: None,
        },
        dir.path().join("repos"),
        store,
    );

    let (bytes, sha256, source) = repos.fetch_package(&repo, "obs").expect("downloads");
    let missing = manager
        .install(&bytes, Some(&sha256), &[], source.clone())
        .expect_err("network was not accepted");
    assert!(matches!(missing, PluginsError::Permissions { .. }));
    let installed = manager
        .install(&bytes, Some(&sha256), &["network".to_string()], source)
        .expect("installs");
    assert_eq!(installed.id, "obs");

    let catalogue = registry.lock().unwrap().catalogue_json();
    let obs = catalogue["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == "obs")
        .expect("obs is in the catalogue")
        .clone();
    assert!(obs["actions"].as_array().unwrap().len() >= 15, "{obs}");
    assert_eq!(obs["origin"], "installed");

    wait_until("obs to report it is not connected", || {
        vars.get("obs.connected") == Some(Value::Bool(false))
    });
    let mut input = flexdeck_actions::RecordingExecutor::new();
    let mut notifier = RecordingNotifier::new();
    let settings = BTreeMap::new();
    let mut ctx = Ctx {
        input: &mut input,
        vars: &vars,
        notifier: &mut notifier,
        settings: &settings,
        now_ms: 0,
    };
    let record = Action {
        plugin: "obs".into(),
        action: "toggle_recording".into(),
        args: Default::default(),
    };
    let error = registry
        .lock()
        .unwrap()
        .invoke(&record, &mut ctx)
        .expect_err("no OBS is running");
    assert!(
        error.to_string().contains("OBS is not connected"),
        "{error}"
    );
    manager.uninstall("obs").unwrap();
}
