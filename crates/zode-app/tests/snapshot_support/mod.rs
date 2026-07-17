mod fixture;
mod geometry;
mod golden;
mod scenes;

pub use geometry::{GeometryExpectation, LayoutRect};
pub use golden::{
    assert_case_geometry, assert_platform_snapshot, compare_reference_images, render_snapshot,
    SnapshotCase,
};
pub use scenes::{
    named_scene, reference_scenes, scene_names, ReferenceScene, REFERENCE_SCENE_NAMES,
};
