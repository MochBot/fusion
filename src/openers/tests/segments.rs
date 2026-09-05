use fusion_engine::openers::segments::{
    clear_full_rows, derive_placements, floor_up_letters, DerivedStep,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct SegmentFixture {
    cases: Vec<SegmentCase>,
}

#[derive(Deserialize)]
struct SegmentCase {
    name: String,
    input: serde_json::Value,
    expected: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FloorUpInput {
    rows_top_down: Vec<String>,
    mirrored: bool,
}

#[derive(Deserialize)]
struct DeriveInput {
    parent: Vec<String>,
    child: Vec<String>,
}

#[test]
fn converts_top_down_letters_to_floor_up_when_the_catalog_branch_is_mirrored() {
    let fixture = segment_fixture();
    let case = fixture_case(&fixture, "floor-up-mirrored");
    let input: FloorUpInput = parse(&case.input, &case.name);
    let expected: Vec<String> = parse(&case.expected, &case.name);

    let actual = floor_up_letters(&input.rows_top_down, input.mirrored);

    assert_eq!(actual, expected, "{}", case.name);
}

#[test]
fn removes_complete_floor_up_rows_when_a_phase_clears() {
    let fixture = segment_fixture();
    let case = fixture_case(&fixture, "clear-full-rows");
    let input: Vec<String> = parse(&case.input, &case.name);
    let expected: Vec<String> = parse(&case.expected, &case.name);

    let actual = clear_full_rows(&input);

    assert_eq!(actual, expected, "{}", case.name);
}

#[test]
fn derives_whole_tetromino_placements_when_a_child_is_a_parent_superset() {
    let fixture = segment_fixture();
    let case = fixture_case(&fixture, "derive-sdpc-tree-edge");
    let input: DeriveInput = parse(&case.input, &case.name);
    let expected: DerivedStep = parse(&case.expected, &case.name);

    let actual = derive_placements(&input.parent, &input.child);

    assert_eq!(actual, Some(expected), "{}", case.name);
}

#[test]
fn returns_no_placements_when_a_child_diff_does_not_tile_into_tetrominoes() {
    let parent = Vec::new();
    let child = vec!["I_________".to_owned()];

    let actual = derive_placements(&parent, &child);

    assert_eq!(actual, None);
}

fn segment_fixture() -> SegmentFixture {
    match serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/parity-segments.json"
    ))) {
        Ok(fixture) => fixture,
        Err(error) => panic!("segment fixture should parse: {error}"),
    }
}

fn fixture_case<'a>(fixture: &'a SegmentFixture, name: &str) -> &'a SegmentCase {
    match fixture.cases.iter().find(|case| case.name == name) {
        Some(case) => case,
        None => panic!("segment fixture should include {name}"),
    }
}

fn parse<T>(value: &serde_json::Value, case_name: &str) -> T
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_value(value.clone()) {
        Ok(parsed) => parsed,
        Err(error) => panic!("{case_name} should match its typed fixture shape: {error}"),
    }
}
