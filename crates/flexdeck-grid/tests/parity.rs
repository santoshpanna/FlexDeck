//! Cross-renderer parity: Rust must reproduce the committed golden fixtures
//! bit-for-bit.
//!
//! The expectations in `contract/fixtures/grid/*.expected.json` are generated
//! from the TypeScript reference implementation and are also checked by the
//! Kotlin suite. If this test fails while the TypeScript one passes, the two
//! renderers have diverged — which is precisely the bug this project is most
//! exposed to. Fix the algorithm; do not edit the expected files to match.

use flexdeck_grid::{
    compute_grid, layout_page, orient_page, GridSpec, Orientation, Rect, WidgetSpan,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    spec: GridSpec,
    widgets: Vec<WidgetSpan>,
}

#[derive(Debug, Deserialize)]
struct ExpectedCells {
    x: Vec<i32>,
    w: Vec<i32>,
    y: Vec<i32>,
    h: Vec<i32>,
}

#[derive(Debug, Deserialize)]
struct Expected {
    #[allow(dead_code)]
    name: Option<String>,
    cells: ExpectedCells,
    rects: BTreeMap<String, Rect>,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/fixtures/grid")
}

fn fixture_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            name.ends_with(".json") && !name.ends_with(".expected.json")
        })
        .collect();
    files.sort();
    files
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> T {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("cannot parse {}: {err}", path.display()))
}

/// Check one oriented view of a fixture against its golden.
///
/// Factored out because landscape and portrait differ ONLY in which transform is
/// applied first, and a second hand-written copy of these assertions is exactly
/// how a parity test quietly stops covering one of the two.
fn assert_matches(
    label: &str,
    spec: &GridSpec,
    widgets: &[WidgetSpan],
    fixture: &Path,
    suffix: &str,
) {
    let stem = fixture
        .file_name()
        .expect("fixture has a file name")
        .to_string_lossy()
        .strip_suffix(".json")
        .expect("fixture is named *.json")
        .to_string();
    let expected: Expected = read_json(&fixture.with_file_name(format!("{stem}{suffix}")));

    let geometry = compute_grid(spec);
    assert_eq!(
        geometry.cols.offsets, expected.cells.x,
        "{label}: column offsets"
    );
    assert_eq!(
        geometry.cols.sizes, expected.cells.w,
        "{label}: column sizes"
    );
    assert_eq!(
        geometry.rows.offsets, expected.cells.y,
        "{label}: row offsets"
    );
    assert_eq!(geometry.rows.sizes, expected.cells.h, "{label}: row sizes");
    assert_eq!(
        layout_page(spec, widgets),
        expected.rects,
        "{label}: widget rects"
    );
}

#[test]
fn every_fixture_matches_the_golden_expectation() {
    let dir = fixtures_dir();
    let files = fixture_files(&dir);

    assert!(!files.is_empty(), "no fixtures found in {}", dir.display());

    for path in &files {
        let fixture: Fixture = read_json(path);

        assert_matches(
            &fixture.name,
            &fixture.spec,
            &fixture.widgets,
            path,
            ".expected.json",
        );

        // The same fixture on a panel held the other way up. Every one of these
        // goldens moves if the transpose changes, so it cannot drift between the
        // web preview and the renderers.
        let (spec, widgets) = orient_page(&fixture.spec, &fixture.widgets, Orientation::Portrait);
        assert_matches(
            &format!("{} (portrait)", fixture.name),
            &spec,
            &widgets,
            path,
            ".portrait.expected.json",
        );
    }

    println!(
        "verified {} fixture(s) in both orientations against golden expectations",
        files.len()
    );
}
