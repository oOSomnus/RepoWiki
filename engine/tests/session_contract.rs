#[path = "common/mod.rs"]
mod common;

use repowiki::model::Summary;
use repowiki::session;
use std::path::Path;

#[test]
fn session_files_reject_paths_that_escape_the_session() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());
    let root = session::session_root(Path::new(&state.repo_path), &state.session_id);

    for unsafe_relative in ["../escape", "/absolute", "..", "../../outside/x.json"] {
        let error = session::session_file(&state, unsafe_relative)
            .expect_err("must refuse a path outside the session");
        assert!(
            error.to_string().contains("unsafe session path"),
            "unexpected diagnosis for {unsafe_relative}: {error}"
        );
    }

    let nested = session::session_file(&state, "reports/x.json").expect("nested session path");
    assert_eq!(nested, root.join("reports/x.json"));
}
