//! One-time changes to a stored deck, run on the first start of an agent
//! that brings them. Each is a new revision, so the editor can undo it.

use serde_json::Value;

/// Where the number of upgrades already run is kept.
pub const UPGRADES_KEY: &str = "config_upgrades";

/// How many upgrades this agent knows. Bump it with each new one.
pub const LATEST: u32 = 1;

/// Upgrade 1: keys no longer run a data source of their own. A key whose
/// `this.*` read its own run reads the shared source instead — `this.temp`
/// becomes `src.weather.temp` — and its own settings for the run are dropped.
/// A plugin's feed (`plugin/feed`) is per key by nature and is left alone.
/// `None` when nothing needed it.
pub fn shared_sources(config: &str) -> Option<String> {
    let mut doc: Value = serde_json::from_str(config).ok()?;
    let mut changed = false;
    for profile in doc.get_mut("profiles").and_then(Value::as_array_mut).into_iter().flatten() {
        for page in profile.get_mut("pages").and_then(Value::as_array_mut).into_iter().flatten() {
            for widget in page.get_mut("widgets").and_then(Value::as_array_mut).into_iter().flatten() {
                changed |= share_one(widget);
            }
        }
    }
    changed.then(|| doc.to_string())
}

fn share_one(widget: &mut Value) -> bool {
    let Some(object) = widget.as_object_mut() else { return false };
    let Some(Value::Array(uses)) = object.get("sources") else { return false };
    let is_feed = |used: &Value| used.get("source").and_then(Value::as_str).is_some_and(|s| s.contains('/'));
    let Some(source) = uses.first().filter(|used| !is_feed(used)).and_then(|used| used.get("source")?.as_str()) else {
        return false;
    };
    let base = format!("src.{source}.");
    let feeds: Vec<Value> = uses.iter().filter(|used| is_feed(used)).cloned().collect();
    if feeds.is_empty() {
        object.remove("sources");
    } else {
        object.insert("sources".into(), Value::Array(feeds));
    }
    for (key, value) in object.iter_mut() {
        if key != "id" && key != "sources" {
            rewrite(value, &base);
        }
    }
    true
}

fn rewrite(value: &mut Value, base: &str) {
    match value {
        Value::String(text) => *text = this_to(text, base),
        Value::Array(items) => items.iter_mut().for_each(|item| rewrite(item, base)),
        Value::Object(fields) => fields.values_mut().for_each(|field| rewrite(field, base)),
        _ => {}
    }
}

/// `this.` as a name's start — not inside `athis.` or `x.this.` — followed by a name.
fn this_to(text: &str, base: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("this.") {
        let before = rest[..at].chars().next_back();
        let after = rest[at + 5..].chars().next();
        let starts = !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.');
        let named = after.is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        out.push_str(&rest[..at]);
        out.push_str(if starts && named { base } else { "this." });
        rest = &rest[at + 5..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn deck(widgets: Value) -> String {
        json!({"schema": 5, "profiles": [{"id": "p", "pages": [{"id": "home", "widgets": widgets}]}]}).to_string()
    }

    #[test]
    fn a_key_with_its_own_source_reads_the_shared_one() {
        let upgraded = shared_sources(&deck(json!([{
            "id": "w",
            "sources": [{"source": "weather", "args": {"city": "Tokyo"}}],
            "face": {"background": {"graph": "this.today"}, "rules": [{"when": "this.temp > 30"}],
                     "elements": [{"template": "{{ this.temp | temp }} · {{this.sky}}"}]},
            "props": {"label": "Press this. now"}
        }])))
        .expect("changed");
        let widget = &serde_json::from_str::<Value>(&upgraded).unwrap()["profiles"][0]["pages"][0]["widgets"][0];
        assert!(widget.get("sources").is_none());
        assert_eq!(widget["face"]["background"]["graph"], "src.weather.today");
        assert_eq!(widget["face"]["rules"][0]["when"], "src.weather.temp > 30");
        assert_eq!(widget["face"]["elements"][0]["template"], "{{ src.weather.temp | temp }} · {{src.weather.sky}}");
        assert_eq!(widget["props"]["label"], "Press this. now", "not a name: left as it is");
    }

    #[test]
    fn a_plugin_feed_and_a_plain_deck_are_left_alone() {
        let feed = deck(json!([{"id": "w", "sources": [{"source": "spotify/now"}], "props": {"labelTemplate": "{{this.title}}"}}]));
        assert_eq!(shared_sources(&feed), None);
        assert_eq!(shared_sources(&deck(json!([{"id": "w", "props": {"label": "x"}}]))), None);
    }

    #[test]
    fn a_feed_beside_a_source_stays() {
        let upgraded = shared_sources(&deck(json!([{"id": "w", "sources": [{"source": "crypto"}, {"source": "spotify/now"}]}]))).unwrap();
        let widget = &serde_json::from_str::<Value>(&upgraded).unwrap()["profiles"][0]["pages"][0]["widgets"][0];
        assert_eq!(widget["sources"], json!([{"source": "spotify/now"}]));
    }
}
