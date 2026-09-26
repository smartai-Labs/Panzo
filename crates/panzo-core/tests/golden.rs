use panzo_core::{
    CameraPlanner, CameraTrack, ClickEvent, MouseAction, MouseButton, PlannerInput, PlannerParams,
    TimeTick,
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_FIXTURES: [&str; 10] = [
    "single-center-click",
    "repeated-near-clicks",
    "far-left-right-clicks",
    "double-click",
    "dense-click-burst",
    "top-left-edge",
    "bottom-right-edge",
    "long-idle",
    "click-outside-capture",
    "recording-end-during-focus",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenInput {
    recording_duration_tick: TimeTick,
    parameter_set: String,
    clicks: Vec<GoldenClick>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenClick {
    id: String,
    time_tick: TimeTick,
    button: MouseButton,
    action: MouseAction,
    normalized_x: f64,
    normalized_y: f64,
    inside_capture: bool,
}

impl GoldenClick {
    fn into_event(self) -> ClickEvent {
        ClickEvent {
            schema_version: 1,
            id: self.id,
            time_tick: self.time_tick,
            button: self.button,
            action: self.action,
            desktop_x: 0,
            desktop_y: 0,
            normalized_x: self.normalized_x,
            normalized_y: self.normalized_y,
            inside_capture: self.inside_capture,
            geometry_revision: 0,
        }
    }
}

#[test]
fn all_v01_camera_golden_fixtures_match() {
    let fixture_root = fixture_root();
    let mut discovered = fs::read_dir(&fixture_root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    discovered.sort();
    let mut expected_names = EXPECTED_FIXTURES.map(str::to_owned).to_vec();
    expected_names.sort();
    assert_eq!(discovered, expected_names, "fixture inventory changed");

    for name in EXPECTED_FIXTURES {
        let fixture = fixture_root.join(name);
        let actual = plan_fixture(name, &fixture.join("input.json"));
        let expected: CameraTrack =
            serde_json::from_slice(&fs::read(fixture.join("camera.json")).unwrap()).unwrap();
        assert_eq!(actual, expected, "golden fixture {name} changed");
        actual.validate().unwrap();
    }
}

#[test]
#[ignore = "requires explicit human-reviewed Golden update"]
fn update_golden_fixtures_after_review() {
    let fixture_root = fixture_root();
    for name in EXPECTED_FIXTURES {
        let fixture = fixture_root.join(name);
        let track = plan_fixture(name, &fixture.join("input.json"));
        let mut json = serde_json::to_vec_pretty(&track).unwrap();
        json.push(b'\n');
        fs::write(fixture.join("camera.json"), json).unwrap();
    }
}

fn plan_fixture(name: &str, input_path: &Path) -> CameraTrack {
    let input: GoldenInput = serde_json::from_slice(&fs::read(input_path).unwrap()).unwrap();
    let params = PlannerParams::default();
    assert_eq!(input.parameter_set, params.parameter_set);
    let clicks = input
        .clicks
        .into_iter()
        .map(GoldenClick::into_event)
        .collect::<Vec<_>>();
    CameraPlanner::new(params, env!("CARGO_PKG_VERSION"))
        .unwrap()
        .plan(PlannerInput {
            recording_duration: input.recording_duration_tick,
            clicks: &clicks,
            click_track_hash: &format!("sha256:fixture:{name}:clicks"),
            capture_geometry_hash: "sha256:fixture:1920x1080",
        })
        .unwrap()
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/camera")
}
