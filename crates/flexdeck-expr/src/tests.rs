use super::*;
use std::collections::HashMap;

fn world() -> HashMap<&'static str, Val> {
    HashMap::from([
        ("system.cpu.usage", Val::Number(37.46)),
        ("obs.record_time", Val::Text("00:01:05".into())),
        ("obs.recording", Val::Bool(true)),
        ("btc.price", Val::Number(61_250.5)),
        ("btc.open", Val::Number(60_000.0)),
        ("weather.code", Val::Number(1.0)),
        ("text.number", Val::Text("12".into())),
        ("media.art", Val::Text(format!("img:{}", "ab".repeat(32)))),
        ("obs.input.mic-aux.volume", Val::Number(80.0)),
        ("net.down", Val::Number(1_572_864.0)),
    ])
}

fn render_with(source: &str) -> String {
    let vars = world();
    let lookup = move |name: &str| vars.get(name).cloned().unwrap_or(Val::Missing);
    Template::parse(source)
        .unwrap_or_else(|error| panic!("{source}: {error}"))
        .render(&lookup, &Locale::default())
}

fn value_of(source: &str) -> Val {
    let vars = world();
    let lookup = move |name: &str| vars.get(name).cloned().unwrap_or(Val::Missing);
    Template::parse(source).unwrap().value(&lookup, &Locale::default())
}

#[test]
fn schema_one_templates_render_as_they_always_did() {
    assert_eq!(render_with("REC {{obs.record_time}}"), "REC 00:01:05");
    assert_eq!(render_with("REC {{ obs.record_time }}"), "REC 00:01:05");
    assert_eq!(render_with("{{obs.recording}}"), "on");
    assert_eq!(render_with("{{nobody.knows}}"), "–");
    assert_eq!(render_with("no braces"), "no braces");
    assert_eq!(render_with("{{system.cpu.usage}}"), "37.5");
}

#[test]
fn a_name_that_is_not_an_expression_is_still_a_name() {
    assert_eq!(render_with("{{obs.input.mic-aux.volume}}"), "80");
    assert_eq!(render_with("{{ btc.price - btc.open }}"), "1250.5", "with spaces it subtracts");
}

#[test]
fn filters_format_numbers() {
    assert_eq!(render_with("{{ system.cpu.usage | round }}%"), "37%");
    assert_eq!(render_with("{{ system.cpu.usage | round(1) }}"), "37.5");
    assert_eq!(render_with("{{ system.cpu.usage | pct(1) }}"), "37.5%");
    assert_eq!(render_with("{{ btc.price | number(2) }}"), "61,250.50");
    assert_eq!(render_with("{{ btc.price | currency }}"), "$61,250.50");
    assert_eq!(render_with("{{ btc.price | currency(\"EUR\", 0) }}"), "€61,251");
    assert_eq!(render_with("{{ btc.price | currency(\"CHF\") }}"), "61,250.50 CHF");
    assert_eq!(render_with("{{ net.down | bytes }}/s"), "1.5 MB/s");
    assert_eq!(render_with("{{ 3725 | duration }}"), "1:02:05");
    assert_eq!(render_with("{{ 65 | duration }}"), "1:05");
    assert_eq!(render_with("{{ -0.001 | round(1) }}"), "0.0", "no negative zero");
}

#[test]
fn arithmetic_and_a_signed_change() {
    assert_eq!(
        render_with("{{ (btc.price - btc.open) / btc.open * 100 | round(2) | sign }}%"),
        "+2.08%"
    );
    assert_eq!(render_with("{{ 2 + 3 * 4 }}"), "14");
    assert_eq!(render_with("{{ text.number + 1 }}"), "13", "text that is a number adds");
    assert_eq!(render_with("{{ \"CPU \" + (system.cpu.usage | round) }}"), "CPU 37");
    assert_eq!(render_with("{{ 1 / 0 }}"), "–", "no infinity on a key");
    assert_eq!(render_with("{{ nobody.knows * 2 }}"), "–");
}

#[test]
fn comparisons_and_logic() {
    assert_eq!(value_of("{{ system.cpu.usage > 30 }}"), Val::Bool(true));
    assert_eq!(value_of("{{ system.cpu.usage >= 80 }}"), Val::Bool(false));
    assert_eq!(value_of("{{ obs.recording == true }}"), Val::Bool(true));
    assert_eq!(value_of("{{ obs.record_time == \"00:01:05\" }}"), Val::Bool(true));
    assert_eq!(value_of("{{ obs.recording and system.cpu.usage < 50 }}"), Val::Bool(true));
    assert_eq!(value_of("{{ not obs.recording }}"), Val::Bool(false));
    // Unknown is never equal, never greater, never less.
    assert_eq!(value_of("{{ nobody.knows > 1 }}"), Val::Bool(false));
    assert_eq!(value_of("{{ nobody.knows < 1 }}"), Val::Bool(false));
    assert_eq!(value_of("{{ nobody.knows != 1 }}"), Val::Bool(false));
}

#[test]
fn map_if_and_default() {
    assert_eq!(render_with("{{ weather.code | map(0: \"sun\", 1: \"cloud\", _: \"fog\") }}"), "cloud");
    assert_eq!(render_with("{{ 7 | map(0: \"sun\", _: \"fog\") }}"), "fog");
    assert_eq!(render_with("{{ 7 | map(0: \"sun\") }}"), "7", "no match, no fallback: itself");
    assert_eq!(render_with("{{ if(obs.recording, \"REC\", \"idle\") }}"), "REC");
    assert_eq!(render_with("{{ nobody.knows | default(\"?\") }}"), "?");
    assert_eq!(render_with("{{ obs.record_time | default(\"?\") }}"), "00:01:05");
    assert_eq!(render_with("{{ round(system.cpu.usage, 1) }}"), "37.5", "a filter called as a function");
}

#[test]
fn text_filters() {
    assert_eq!(render_with("{{ \"Live\" | upper }}"), "LIVE");
    assert_eq!(render_with("{{ \"A long song title\" | truncate(8) }}"), "A long…");
    assert_eq!(render_with("{{ \"  x  \" | trim }}"), "x");
}

#[test]
fn time_formats_iso_with_its_own_offset() {
    assert_eq!(render_with("{{ \"2026-10-11T21:05:09+05:30\" | time }}"), "21:05");
    assert_eq!(render_with("{{ \"2026-10-11T21:05:09+05:30\" | time(\"%a %d\") }}"), "Sun 11");
    let twelve = Locale { hour12: true, ..Locale::default() };
    let template = Template::parse("{{ \"2026-10-11T21:05:09+00:00\" | time }}").unwrap();
    assert_eq!(template.render(&|_: &str| Val::Missing, &twelve), "9:05 PM");
}

#[test]
fn temperatures_follow_the_desktops_unit() {
    let template = Template::parse("{{ 21.4 | temp }} / {{ 21.4 | temp(1) }}").unwrap();
    assert_eq!(template.render(&|_: &str| Val::Missing, &Locale::default()), "21° / 21.4°");
    let us = Locale { fahrenheit: true, ..Locale::default() };
    assert_eq!(template.render(&|_: &str| Val::Missing, &us), "71° / 70.5°");
}

#[test]
fn a_locale_writes_numbers_its_way() {
    let german = Locale { decimal: ',', group: '.', currency: "EUR".into(), hour12: false, fahrenheit: false };
    let template = Template::parse("{{ 61250.5 | currency }} · {{ 37.46 | round(1) }}").unwrap();
    assert_eq!(template.render(&|_: &str| Val::Missing, &german), "€61.250,50 · 37,5");
}

#[test]
fn a_single_hole_keeps_its_value() {
    assert_eq!(value_of("{{ system.cpu.usage }}"), Val::Number(37.46));
    assert_eq!(value_of("{{media.art}}"), world()["media.art"]);
    assert_eq!(value_of("CPU {{ system.cpu.usage | round }}"), Val::Text("CPU 37".into()));
    let lookup = |_: &str| Val::Missing;
    let half = Template::parse("img:{{ nobody.knows }}").unwrap();
    assert_eq!(half.value_if_known(&lookup, &Locale::default()), Val::Missing);
}

#[test]
fn variables_are_found_wherever_they_are_used() {
    let template =
        Template::parse("{{ if(a.b > 1, c, d | default(e)) }} {{ x | map(y: z) }}").unwrap();
    let names: Vec<String> = template.variables().into_iter().collect();
    assert_eq!(names, ["a.b", "c", "d", "e", "x", "y", "z"]);
    assert!(!Template::parse("plain").unwrap().is_live());
}

#[test]
fn mistakes_are_refused_with_a_reason() {
    let refused = |source: &str| Template::parse(source).unwrap_err().to_string();
    assert!(refused("{{ cpu | rond }}").contains("'rond' is not a filter"));
    assert!(refused("{{ cpu | clamp(1) }}").contains("clamp takes"));
    assert!(refused("{{ cpu | round(1, 2) }}").contains("round takes"));
    assert!(refused("{{ x | map(1, 2) }}").contains("map takes"));
    assert!(refused("{{ cpu").contains("never closed"));
    assert!(refused("{{ }}").contains("empty"));
    assert!(refused("{{ (1 + 2 }}").contains("expected ')'"));
    assert!(refused("{{ \"open }}").contains("quote"));
    assert_eq!(
        Template::parse(&"x".repeat(MAX_TEMPLATE_LEN + 1)).unwrap_err(),
        ExprError::TooLong
    );
    let deep = format!("{{{{ {}1{} }}}}", "(".repeat(40), ")".repeat(40));
    assert_eq!(Template::parse(&deep).unwrap_err(), ExprError::TooDeep);
}

#[test]
fn an_expression_alone_is_what_a_rule_reads() {
    let rule = parse_expression("system.cpu.usage > 30").unwrap();
    let vars = world();
    let lookup = move |name: &str| vars.get(name).cloned().unwrap_or(Val::Missing);
    assert_eq!(rule.eval(&lookup, &Locale::default()), Val::Bool(true));
    assert_eq!(rule.variables().into_iter().collect::<Vec<_>>(), ["system.cpu.usage"]);
}
