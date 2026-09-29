#[path = "common/mod.rs"]
mod common;

use repowiki::model::Summary;
use repowiki::session::{self, files, SessionState};
use std::path::{Path, PathBuf};

fn session_handles(state: &SessionState) -> (PathBuf, String) {
    (
        Path::new(&state.repo_path).to_path_buf(),
        state.session_id.clone(),
    )
}

#[test]
fn session_files_reject_paths_that_escape_the_session() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());
    let root = state.session_dir();

    for unsafe_relative in ["../escape", "/absolute", "..", "../../outside/x.json"] {
        let error = session::session_file(&state, unsafe_relative)
            .expect_err("must refuse a path outside the session");
        assert!(
            error.to_string().contains("unsafe session path"),
            "unexpected diagnosis for {unsafe_relative}: {error}"
        );
    }

    let nested = session::session_file(&state, "reports/x.json").expect("nested session path");
    assert_eq!(nested, root.join(files::REPORTS).join("x.json"));
}

#[test]
fn a_session_holds_the_directories_its_schema_declares() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());

    for directory in [files::SOURCES, files::PROMPTS, files::HISTORY] {
        let path = session::session_file(&state, directory).expect(directory);
        assert_eq!(path, state.session_dir().join(directory));
        assert!(path.is_dir(), "{directory} was never created");
    }

    assert_eq!(
        session::session_file(&state, files::STATE).expect("state name"),
        state.session_dir().join(files::STATE)
    );
}

#[test]
fn concurrent_locked_operations_persist_every_write() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());
    let (repo, session_id) = session_handles(&state);
    let writers: Vec<_> = (0..8)
        .map(|_| {
            let (repo, session_id) = (repo.clone(), session_id.clone());
            std::thread::spawn(move || {
                session::with_locked_session(&repo, &session_id, |state| {
                    state.mark_write();
                    Ok(())
                })
            })
        })
        .collect();
    for writer in writers {
        writer.join().expect("writer").expect("locked write");
    }

    let final_state = session::peek(&repo, &session_id).expect("peek after concurrent writes");
    assert_eq!(
        final_state.docs_written, 8,
        "each acquisition reads the state the previous one persisted"
    );
}

#[test]
fn a_failing_operation_persists_nothing_and_keeps_the_session() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());
    let (repo, session_id) = session_handles(&state);

    let error = session::with_locked_session(&repo, &session_id, |state| {
        state.mark_write();
        let _ = session::session_file(state, "../refused")?;
        Ok(())
    })
    .expect_err("the operation must fail");
    assert!(
        error.to_string().contains("unsafe session path"),
        "unexpected diagnosis: {error}"
    );

    let after = session::peek(&repo, &session_id).expect("a failed operation keeps the session");
    assert_eq!(
        after.docs_written, 0,
        "an operation that failed must not persist the writes it made"
    );
}

#[test]
fn close_session_destroys_the_session_only_on_success() {
    let (_repo, state) = common::prepared_session(Vec::new(), &[], Summary::default());
    let (repo, session_id) = session_handles(&state);

    let refused = session::close_session(&repo, &session_id, |state| {
        let _ = session::session_file(state, "../refused")?;
        Ok(())
    })
    .expect_err("validation failures must keep the session");
    assert!(
        refused.to_string().contains("unsafe session path"),
        "unexpected diagnosis: {refused}"
    );
    session::peek(&repo, &session_id).expect("the session survives a refused close");

    session::close_session(&repo, &session_id, |state| Ok(state.docs_written))
        .expect("a successful close");

    let peeked = session::peek(&repo, &session_id).expect_err("the session is gone");
    assert!(
        peeked.to_string().contains("session not found"),
        "unexpected diagnosis from peek: {peeked}"
    );
    let reopened = session::with_locked_session(&repo, &session_id, |_| Ok(()))
        .expect_err("a closed session cannot be reopened");
    assert!(
        reopened.to_string().contains("session not found"),
        "unexpected diagnosis from with_locked_session: {reopened}"
    );
}
