//! Pins the proto3 JSON mapping.
//!
//! These assertions look pedantic, and that is the point: they are the only
//! thing standing between "three renderers share one contract" and "three
//! renderers each think they share one contract". A change here means the
//! TypeScript (`@bufbuild/protobuf`) and Kotlin (`Wire`) clients would stop
//! agreeing with Rust about the shape of the same document.

use flexdeck_protocol::flexdeck::v1 as pb;
use serde_json::{json, Value};

/// A flow's blocks are automation-engine JSON; here, one "do an action".
fn hotkey_blocks() -> pbjson_types::ListValue {
    serde_json::from_value(json!([
        {"block": "doAction", "plugin": "core", "action": "hotkey",
         "args": {"keys": {"text": "ctrl+shift+m"}}}
    ]))
    .expect("a list of JSON values")
}

fn sample_config() -> pb::Config {
    pb::Config {
        schema: 2,
        rev: 42,
        profiles: vec![pb::Profile {
            id: "tab1".to_string(),
            name: "Kitchen tablet".to_string(),
            grid: Some(pb::Grid {
                cols: 8,
                rows: 4,
                gap_x: 8,
                gap_y: 8,
                usable_w: 1280,
                usable_h: 800,
            }),
            portrait_grid: None,
            screen: None,
            pages: vec![pb::Page {
                id: "home".to_string(),
                name: "Home".to_string(),
                background: None,
                widgets: vec![pb::Widget {
                    id: "mute".to_string(),
                    r#type: pb::WidgetType::Button as i32,
                    col: 0,
                    row: 0,
                    w: 2,
                    h: 1,
                    portrait: None,
                    face: None,
                    sources: Vec::new(),
                    props: Some(pb::WidgetProps {
                        label: "Mute".to_string(),
                        icon: "mic-off".to_string(),
                        bg_color: "#1e40af".to_string(),
                        fg_color: "#ffffff".to_string(),
                        label_template: "Mic {{system.audio.muted}}".to_string(),
                        icon_template: String::new(),
                        hide_dot: false,
                        no_auto_light: false,
                    }),
                    flows: vec![pb::Flow {
                        trigger: Some(pb::FlowTrigger {
                            on: Some(pb::flow_trigger::On::Gesture(
                                pb::Gesture::ShortPress as i32,
                            )),
                        }),
                        name: String::new(),
                        blocks: Some(hotkey_blocks()),
                        zone: String::new(),
                    }],
                    states: Some(pb::WidgetStates {
                        looks: vec![
                            pb::StateLook {
                                id: "live".to_string(),
                                label: "Live".to_string(),
                                ..Default::default()
                            },
                            pb::StateLook {
                                id: "muted".to_string(),
                                label: "Muted".to_string(),
                                bg_color: "#b91c1c".to_string(),
                                ..Default::default()
                            },
                        ],
                        source: Some(pb::widget_states::Source::Variable(pb::VariableSource {
                            variable: "system.audio.muted".to_string(),
                            cases: vec![pb::StateCase {
                                value: "true".to_string(),
                                state: "muted".to_string(),
                                op: pb::CompareOp::Eq as i32,
                            }],
                            otherwise: "live".to_string(),
                        })),
                        rules: vec![],
                    }),
                    slider: None,
                }],
            }],
        }],
    }
}

#[test]
fn field_names_use_lower_camel_case() {
    let json = serde_json::to_value(sample_config()).expect("serializes");

    let grid = &json["profiles"][0]["grid"];
    assert_eq!(grid["gapX"], 8, "gap_x must appear as gapX");
    assert_eq!(grid["usableW"], 1280, "usable_w must appear as usableW");
    assert!(
        grid.get("gap_x").is_none(),
        "snake_case names must not appear"
    );

    let props = &json["profiles"][0]["pages"][0]["widgets"][0]["props"];
    assert_eq!(props["bgColor"], "#1e40af");
    assert_eq!(props["fgColor"], "#ffffff");
}

#[test]
fn sixty_four_bit_integers_are_encoded_as_strings() {
    // proto3 JSON requires uint64 to be a *string*, because JavaScript numbers
    // cannot represent the full range. If this ever regresses, the TypeScript
    // client would silently lose precision on large revisions.
    let json = serde_json::to_value(sample_config()).expect("serializes");
    assert_eq!(json["rev"], Value::String("42".to_string()));
}

#[test]
fn enums_are_encoded_by_name_not_number() {
    let json = serde_json::to_value(sample_config()).expect("serializes");
    assert_eq!(
        json["profiles"][0]["pages"][0]["widgets"][0]["type"],
        Value::String("WIDGET_TYPE_BUTTON".to_string())
    );
}

#[test]
fn default_values_are_omitted_entirely() {
    // proto3 JSON omits *default* values: an empty repeated field, a zero
    // scalar. A client must therefore treat a missing key as "the default"
    // rather than assuming the key is present. This is the easiest thing for
    // the TypeScript or Kotlin client to get wrong, so it is pinned here.
    let bare = pb::Config {
        schema: 0,
        rev: 0,
        profiles: vec![],
    };
    let json = serde_json::to_value(&bare).expect("serializes");

    assert!(
        json.get("profiles").is_none(),
        "an empty repeated field must be absent, not []"
    );
    assert!(
        json.get("schema").is_none(),
        "a zero-valued scalar must be absent"
    );
    assert!(
        json.get("rev").is_none(),
        "a zero-valued scalar must be absent"
    );
    assert_eq!(
        json,
        serde_json::json!({}),
        "an all-default message serialises as an empty object"
    );
}

#[test]
fn an_empty_nested_field_is_also_omitted() {
    let json = serde_json::to_value(sample_config()).expect("serializes");
    let live = &json["profiles"][0]["pages"][0]["widgets"][0]["states"]["looks"][0];

    assert_eq!(live["label"], "Live", "a populated field is present");
    assert!(
        live.get("bgColor").is_none(),
        "an empty bgColor must be absent, not \"\""
    );
}

#[test]
fn a_flow_carries_its_trigger_by_name_and_its_blocks_as_plain_json() {
    let json = serde_json::to_value(sample_config()).expect("serializes");
    let widget = &json["profiles"][0]["pages"][0]["widgets"][0];

    assert_eq!(
        widget["flows"][0]["trigger"]["gesture"],
        "GESTURE_SHORT_PRESS"
    );
    // Blocks are not wrapped in proto's Value envelope: a client reads and
    // writes the automation engine's JSON directly.
    assert_eq!(widget["flows"][0]["blocks"][0]["block"], "doAction");
    assert_eq!(
        widget["flows"][0]["blocks"][0]["args"]["keys"]["text"],
        "ctrl+shift+m"
    );
    assert_eq!(widget["states"]["variable"]["otherwise"], "live");
    assert_eq!(widget["states"]["variable"]["cases"][0]["state"], "muted");
}

#[test]
fn a_cycle_source_is_an_empty_object() {
    let states = pb::WidgetStates {
        looks: vec![],
        source: Some(pb::widget_states::Source::Cycle(pb::CycleSource {})),
        rules: vec![],
    };
    let json = serde_json::to_value(&states).expect("serializes");
    assert_eq!(json, json!({"cycle": {}}));
    let back: pb::WidgetStates = serde_json::from_value(json).expect("deserializes");
    assert_eq!(back, states);
}

#[test]
fn a_schema_one_widget_is_refused() {
    // Schema 2 dropped `actions` and `stateVariable`. Accepting them silently
    // would load a deck whose buttons do nothing, with no word of why.
    let old = json!({"id": "w1", "actions": {"press": []}});
    assert!(serde_json::from_value::<pb::Widget>(old).is_err());
    let old = json!({"label": "Mute", "stateVariable": "system.audio.muted"});
    assert!(serde_json::from_value::<pb::WidgetProps>(old).is_err());
}

#[test]
fn a_oneof_uses_the_field_name_as_its_key() {
    let hello = pb::ClientMessage {
        kind: Some(pb::client_message::Kind::Hello(pb::ClientHello {
            role: "android".to_string(),
            device_id: "tab1".to_string(),
            token: "tok".to_string(),
            device: None,
        })),
    };

    let json = serde_json::to_value(&hello).expect("serializes");
    assert_eq!(json["hello"]["role"], "android");
    assert!(
        json.get("press").is_none(),
        "the unset branch must be absent"
    );

    let press = pb::ClientMessage {
        kind: Some(pb::client_message::Kind::Press(pb::PressEvent {
            widget_id: "mute".to_string(),
            value: String::new(),
            ts: 1_700_000_000_000,
            gesture: pb::Gesture::LongPress as i32,
            zone: String::new(),
        })),
    };
    let json = serde_json::to_value(&press).expect("serializes");
    assert_eq!(json["press"]["widgetId"], "mute");
    assert_eq!(json["press"]["gesture"], "GESTURE_LONG_PRESS");
    // int64 is a string too, not just uint64.
    assert_eq!(
        json["press"]["ts"],
        Value::String("1700000000000".to_string())
    );
}

#[test]
fn a_config_survives_a_full_round_trip() {
    let original = sample_config();
    let json = serde_json::to_string(&original).expect("serializes");
    let restored: pb::Config = serde_json::from_str(&json).expect("deserializes");

    assert_eq!(original, restored);
}

#[test]
fn the_documented_example_decodes() {
    // The exact shape the portal will PUT, matching contract/README.md and the
    // widget example in the PRD (section 5).
    let rendered = r##"{
      "schema": 2,
      "rev": "42",
      "profiles": [{
        "id": "tab1",
        "name": "Kitchen tablet",
        "grid": {"cols": 6, "rows": 4, "gapX": 8, "gapY": 8, "usableW": 1280, "usableH": 800},
        "pages": [{
          "id": "home",
          "widgets": [{
            "id": "w1",
            "type": "WIDGET_TYPE_BUTTON",
            "col": 0, "row": 0, "w": 2, "h": 1,
            "props": {"label": "Mute", "icon": "mic-off"},
            "flows": [{
              "trigger": {"gesture": "GESTURE_SHORT_PRESS"},
              "blocks": [{"block": "doAction", "plugin": "core", "action": "hotkey",
                          "args": {"keys": {"text": "ctrl+shift+m"}}}]
            }, {
              "name": "Live goes red",
              "trigger": {"event": {"plugin": "obs", "event": "scene_changed", "filter": {"scene": "Live"}}},
              "blocks": []
            }],
            "states": {"looks": [{"id": "on", "bgColor": "#b91c1c"}], "cycle": {}}
          }]
        }]
      }]
    }"##;

    let config: pb::Config =
        serde_json::from_str(rendered).expect("the documented example must decode");

    assert_eq!(config.schema, 2);
    assert_eq!(config.rev, 42);
    let grid = config.profiles[0].grid.as_ref().expect("grid present");
    assert_eq!(grid.cols, 6);
    assert_eq!(grid.gap_x, 8);

    let widget = &config.profiles[0].pages[0].widgets[0];
    assert_eq!(widget.col, 0);
    assert_eq!(widget.w, 2);
    assert_eq!(widget.r#type, pb::WidgetType::Button as i32);
    assert_eq!(widget.props.as_ref().unwrap().label, "Mute");

    let press = &widget.flows[0];
    assert_eq!(
        press.trigger.as_ref().unwrap().on,
        Some(pb::flow_trigger::On::Gesture(
            pb::Gesture::ShortPress as i32
        ))
    );
    assert_eq!(press.blocks.as_ref().unwrap().values.len(), 1);

    let Some(pb::flow_trigger::On::Event(event)) = &widget.flows[1].trigger.as_ref().unwrap().on
    else {
        panic!("the second flow is event-triggered");
    };
    assert_eq!(event.event, "scene_changed");
    assert_eq!(event.filter.get("scene").map(String::as_str), Some("Live"));

    let states = widget.states.as_ref().unwrap();
    assert_eq!(states.looks[0].bg_color, "#b91c1c");
    assert!(matches!(
        states.source,
        Some(pb::widget_states::Source::Cycle(_))
    ));
}

#[test]
fn a_slider_widget_and_a_value_event_have_the_documented_wire_shape() {
    let widget: pb::Widget = serde_json::from_value(serde_json::json!({
        "id": "vol",
        "type": "WIDGET_TYPE_SLIDER",
        "col": 0, "row": 0, "w": 1, "h": 3,
        "slider": {"orientation": "SLIDER_ORIENTATION_VERTICAL", "min": 0, "max": 100, "step": 1,
                   "variable": "system.audio.volume", "throttleMs": 40},
        "flows": [{"trigger": {"value": {}}, "blocks": []}]
    }))
    .expect("parses");
    assert_eq!(widget.r#type, pb::WidgetType::Slider as i32);
    let slider = widget.slider.expect("slider props");
    assert_eq!(slider.orientation, pb::SliderOrientation::Vertical as i32);
    assert_eq!(slider.throttle_ms, 40);
    assert!(matches!(
        widget.flows[0].trigger.as_ref().and_then(|t| t.on.as_ref()),
        Some(pb::flow_trigger::On::Value(_))
    ));

    let message: pb::ClientMessage = serde_json::from_value(serde_json::json!({
        "value": {"widgetId": "vol", "value": 42.5, "seq": 3, "final": true, "ts": "1"}
    }))
    .expect("parses");
    let Some(pb::client_message::Kind::Value(value)) = message.kind else {
        panic!("a value event")
    };
    assert_eq!((value.value, value.seq, value.r#final), (42.5, 3, true));

    // An unknown value is absent, not zero: the thumb must not jump to 0.
    let state = pb::StateEvent {
        widget_id: "vol".into(),
        ok: true,
        ..Default::default()
    };
    let json = serde_json::to_value(&state).unwrap();
    assert!(json.get("value").is_none(), "{json}");
    let zero = pb::StateEvent {
        value: Some(0.0),
        ..state
    };
    assert_eq!(serde_json::to_value(&zero).unwrap()["value"], 0.0);
}

/// The portrait grid and a pinned portrait placement survive the round trip,
/// spelled the way the portal writes them.
#[test]
fn portrait_layout_round_trips() {
    let json = r#"{"schema":3,"profiles":[{"id":"p","grid":{"cols":6,"rows":4},"portraitGrid":{"cols":3,"rows":9},"pages":[{"id":"g","widgets":[{"id":"w","col":1,"w":1,"h":1,"portrait":{"col":2,"row":5,"w":1,"h":1}}]}]}]}"#;
    let config: pb::Config = serde_json::from_str(json).expect("parses");
    let profile = &config.profiles[0];
    assert_eq!(profile.portrait_grid.as_ref().map(|grid| (grid.cols, grid.rows)), Some((3, 9)));
    let placement = profile.pages[0].widgets[0].portrait.as_ref().expect("pinned");
    assert_eq!((placement.col, placement.row, placement.w, placement.h), (2, 5, 1, 1));
    let back = serde_json::to_value(&config).expect("serialises");
    assert_eq!(back["profiles"][0]["portraitGrid"]["rows"], 9);
    assert_eq!(back["profiles"][0]["pages"][0]["widgets"][0]["portrait"]["row"], 5);
}
