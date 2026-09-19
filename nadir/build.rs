use shadow_rs::{BuildPattern, ShadowBuilder};

fn main() {
    // The default `Lazy` pattern caches git state across debug builds, so a debug binary would
    // report a commit it was not built from.
    ShadowBuilder::builder()
        .build_pattern(BuildPattern::Custom {
            if_path_changed: vec!["../.git/HEAD".into(), "../.git/index".into()],
            if_env_changed: vec![],
        })
        .build()
        .unwrap();
}
