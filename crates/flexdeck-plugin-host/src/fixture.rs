//! A plugin written against the raw protocol, for tests.
//!
//! Public (but hidden) so that other crates' tests can run it: a test binary
//! that finds `FLEXDECK_PLUGIN_ID` in its environment calls [`serve`] and
//! becomes the plugin.
//!
//! It behaves well by default and can be told to misbehave through environment
//! variables, so each test proves one thing the host must survive:
//!
//! * `FIXTURE_ABI` — claim this ABI version instead of the one the host asked for;
//! * `FIXTURE_ID` — claim this id instead of `fixture`;
//! * `FIXTURE_PERMISSIONS` — comma-separated permissions to claim;
//! * `FIXTURE_SILENT=1` — never answer `initialize`;
//! * `FIXTURE_CRASH_AT_START=1` — exit before saying anything;
//! * `FIXTURE_CHOICES=1` — answer `choices` (without it, "no method", as a
//!   plugin built before `choices` existed does);
//! * `FIXTURE_CRASH_SAYING=<text>` — write `<text>` to stderr, then exit, the
//!   way a runtime that cannot load the plugin does;
//! * `FIXTURE_PUBLISH_AT_START=1` — publish a value the instant `initialize`
//!   is answered, before the host can possibly have declared it;
//! * `FIXTURE_SAMPLE_ESCAPES=1` — when sampled, also try to write another
//!   plugin's variable;
//! * `FIXTURE_CHATTY=1` — when sampled, print a line to stdout that is not
//!   the protocol;
//! * `FIXTURE_REFUSE_EXAMPLES=1` — refuse every `check`, examples included.
//!
//! Its actions: `greet` (counts, publishes, emits, notifies, and on the first
//! call declares a variable at run time), `crash`, `sleep`, `escape` (tries to
//! write another plugin's variable), `undeclared` (emits an event it never
//! declared), `claim` (tries to declare a variable in another plugin's
//! namespace), `env` (logs the value of the environment variable `name`, so a
//! test sees what a plugin inherits), and `fail`. Under ABI 3 also: `toggle` (reports a look), `store`
//! (keeps a token with `settings/store`), `open` (asks for a browser), and
//! `image` (hands over a picture; `image` with `bad=1` sends a wrong checksum).

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use flexdeck_plugin::abi::{self, Message};
use serde_json::json;

fn send(message: &Message) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(message.to_line().as_bytes());
    let _ = stdout.flush();
}

fn notify(method: &str, params: serde_json::Value) {
    send(&Message::notification(method, params));
}

fn manifest(asked: u32) -> abi::ManifestDto {
    let permissions = std::env::var("FIXTURE_PERMISSIONS")
        .map(|list| {
            list.split(',')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|_| vec![abi::permission::NOTIFICATIONS.to_string()]);
    let argument = |name: &str, kind: &str, example: &str| abi::ArgumentDto {
        name: name.into(),
        description: String::new(),
        example: example.into(),
        kind: kind.into(),
        choices: vec![],
        optional: false,
    };
    let action = |name: &str, arguments: Vec<abi::ArgumentDto>| abi::ActionDto {
        action: name.into(),
        description: format!("The fixture's {name}"),
        arguments,
        slider: None,
        active: None,
    };
    let abi = std::env::var("FIXTURE_ABI")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(asked);
    let mut actions = vec![
        action("greet", vec![argument("name", abi::kind::TEXT, "world")]),
        action("crash", vec![]),
        action("sleep", vec![argument("ms", abi::kind::NUMBER, "10")]),
        action("escape", vec![]),
        action("undeclared", vec![]),
        action("claim", vec![]),
        action("env", vec![argument("name", abi::kind::TEXT, "PATH")]),
        action("fail", vec![]),
    ];
    if abi >= 3 {
        let mut level = action("level", vec![argument("percent", abi::kind::NUMBER, "50")]);
        level.slider = Some(abi::SliderDto {
            argument: "percent".into(),
            min: 0,
            max: 100,
            step: 1,
            feedback: Some("fixture.level".into()),
            mute: None,
        });
        actions.extend([
            action("toggle", vec![]),
            action("store", vec![argument("token", abi::kind::TEXT, "t")]),
            action("open", vec![argument("url", abi::kind::TEXT, "https://example.com")]),
            action("image", vec![]),
            level,
        ]);
    }
    abi::ManifestDto {
        abi,
        id: std::env::var("FIXTURE_ID").unwrap_or_else(|_| "fixture".into()),
        name: "Fixture".into(),
        version: std::env::var("FIXTURE_VERSION").unwrap_or_else(|_| "1.0.0".into()),
        description: "A plugin for the host's tests".into(),
        author: "flexdeck".into(),
        actions,
        settings: vec![
            abi::SettingDto {
                key: "greeting".into(),
                description: String::new(),
                kind: abi::kind::TEXT.into(),
                choices: vec![],
                default_value: Some("Hello".into()),
                secret: false,
                internal: false,
                help: None,
            },
            abi::SettingDto {
                key: "token".into(),
                description: String::new(),
                kind: abi::kind::TEXT.into(),
                choices: vec![],
                default_value: None,
                secret: true,
                internal: false,
                help: None,
            },
        ],
        events: vec![abi::EventDto {
            name: "greeted".into(),
            description: "Someone was greeted".into(),
            fields: vec!["name".into(), "greeting".into()],
        }],
        permissions,
        uses: Vec::new(),
        presets: Vec::new(),
        feeds: Vec::new(),
        guide: None,
    }
}

/// Serve the fixture protocol on stdin/stdout until stdin closes.
pub fn serve() {
    if let Ok(text) = std::env::var("FIXTURE_CRASH_SAYING") {
        eprintln!("{text}");
        std::process::exit(1);
    }
    if std::env::var("FIXTURE_CRASH_AT_START").is_ok() {
        std::process::exit(7);
    }
    let silent = std::env::var("FIXTURE_SILENT").is_ok();

    let mut count = 0u64;
    let mut greeting = "Hello".to_string();
    let mut on = false;

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Message>(&line) else {
            eprintln!("fixture: not json: {line}");
            continue;
        };
        let (Some(id), Some(method)) = (request.id, request.method.as_deref()) else {
            continue;
        };
        let params = request.params.unwrap_or_default();

        let result = match method {
            abi::method::INITIALIZE => {
                if silent {
                    continue;
                }
                if let Some(value) = params["settings"]["greeting"].as_str() {
                    greeting = value.to_string();
                }
                eprintln!("fixture: starting");
                let asked = params["abi"].as_u64().unwrap_or(2) as u32;
                json!({
                    "manifest": manifest(asked),
                    "variables": [{
                        "name": "fixture.count",
                        "description": "How many greetings",
                        "kind": "number",
                        "readOnly": true
                    }]
                })
            }
            abi::method::CHECK => {
                let name = params["args"]["name"].as_str().unwrap_or_default();
                if std::env::var("FIXTURE_REFUSE_EXAMPLES").is_ok() {
                    json!({"ok": false, "error": "refuses everything"})
                } else if params["action"] == "greet" && name == "nobody" {
                    json!({"ok": false, "error": "nobody cannot be greeted"})
                } else {
                    json!({"ok": true})
                }
            }
            abi::method::SAMPLE => {
                if std::env::var("FIXTURE_SAMPLE_ESCAPES").is_ok() {
                    notify(abi::notify::VARS_SET, json!({"values": {"system.audio.volume": {"kind": "number", "value": 0}}}));
                }
                if std::env::var("FIXTURE_CHATTY").is_ok() {
                    let mut stdout = std::io::stdout().lock();
                    let _ = stdout.write_all(b"debug: sampled
");
                    let _ = stdout.flush();
                }
                json!({"values": {"fixture.count": {"kind": "number", "value": count}}})
            }
            abi::method::SETTINGS_CHANGED => {
                if let Some(value) = params["settings"]["greeting"].as_str() {
                    greeting = value.to_string();
                }
                json!({})
            }
            abi::method::SHUTDOWN => {
                send(&Message::success(id, json!({})));
                std::process::exit(0);
            }
            abi::method::INVOKE => {
                let args: BTreeMap<String, String> =
                    serde_json::from_value(params["args"].clone()).unwrap_or_default();
                match params["action"].as_str().unwrap_or_default() {
                    "greet" => {
                        count += 1;
                        let name = args.get("name").cloned().unwrap_or_default();
                        if count == 1 {
                            notify(
                                abi::notify::VARS_DECLARE,
                                json!({"variables": [{
                                    "name": "fixture.last_name",
                                    "kind": "text",
                                    "readOnly": true
                                }]}),
                            );
                        }
                        notify(
                            abi::notify::VARS_SET,
                            json!({"values": {
                                "fixture.count": {"kind": "number", "value": count},
                                "fixture.last_name": {"kind": "text", "value": name}
                            }}),
                        );
                        notify(
                            abi::notify::EVENT_EMIT,
                            json!({"event": "greeted", "payload": {"name": name, "greeting": greeting}}),
                        );
                        notify(
                            abi::notify::NOTIFY,
                            json!({"title": greeting, "body": name}),
                        );
                        json!({"ok": true})
                    }
                    "crash" => std::process::exit(3),
                    "sleep" => {
                        let ms = args.get("ms").and_then(|v| v.parse().ok()).unwrap_or(0);
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                        json!({"ok": true})
                    }
                    "escape" => {
                        notify(
                            abi::notify::VARS_SET,
                            json!({"values": {"system.audio.volume": {"kind": "number", "value": 0}}}),
                        );
                        json!({"ok": true})
                    }
                    "claim" => {
                        notify(
                            abi::notify::VARS_DECLARE,
                            json!({"variables": [{"name": "obs.stream_key", "kind": "text", "description": "taken"}]}),
                        );
                        json!({"ok": true})
                    }
                    "env" => {
                        let name = args.get("name").cloned().unwrap_or_default();
                        let value = std::env::var(&name).unwrap_or_else(|_| "<unset>".into());
                        notify(abi::notify::LOG, json!({"level": "info", "message": format!("env {name}={value}")}));
                        json!({"ok": true})
                    }
                    "undeclared" => {
                        notify(abi::notify::EVENT_EMIT, json!({"event": "nope"}));
                        json!({"ok": true})
                    }
                    "fail" => json!({"ok": false, "error": "the fixture was asked to fail"}),
                    "toggle" => {
                        on = !on;
                        json!({"ok": true, "state": if on { "on" } else { "off" }})
                    }
                    "store" => {
                        let token = args.get("token").cloned().unwrap_or_default();
                        notify(abi::notify::SETTINGS_STORE, json!({"values": {"token": token}}));
                        notify(abi::notify::SETTINGS_STORE, json!({"values": {"nope": "x"}}));
                        json!({"ok": true})
                    }
                    "open" => {
                        let url = args.get("url").cloned().unwrap_or_default();
                        notify(abi::notify::OPEN_URL, json!({ "url": url }));
                        json!({"ok": true})
                    }
                    "image" => {
                        // The 8-byte PNG signature, and its real sha256 — or a
                        // wrong one when asked.
                        let sha = if args.contains_key("bad") {
                            "00".repeat(32)
                        } else {
                            "4c4b6a3be1314ab86138bef4314dde022e600960d8689a2c8f8631802d20dab6".into()
                        };
                        notify(
                            abi::notify::IMAGES_PUT,
                            json!({"sha256": sha, "mime": "image/png", "data": "iVBORw0KGgo="}),
                        );
                        json!({"ok": true})
                    }
                    "level" => json!({"ok": true}),
                    other => json!({"ok": false, "error": format!("no action '{other}'")}),
                }
            }
            // Only with `FIXTURE_CHOICES` set; otherwise it is a plugin from
            // before `choices` existed, and says "no method" like one.
            abi::method::CHOICES if std::env::var("FIXTURE_CHOICES").is_ok() => json!({
                "arguments": {
                    "name": [{"value": "ada"}, {"value": "grace", "label": "Grace Hopper"}],
                    "bogus": [{"value": "not an argument of greet"}]
                }
            }),
            other => {
                send(&Message::failure(
                    id,
                    -32601,
                    format!("no method '{other}'"),
                ));
                continue;
            }
        };
        let initialized = method == abi::method::INITIALIZE;
        send(&Message::success(id, result));
        if initialized && std::env::var("FIXTURE_PUBLISH_AT_START").is_ok() {
            notify(
                abi::notify::VARS_SET,
                json!({"values": {"fixture.count": {"kind": "number", "value": 42}}}),
            );
        }
    }
}
