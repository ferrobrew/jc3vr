use super::*;

fn failure(kind: QueryFailureKind, diagnostic: &str) -> Result<Vec<BoundSource>, QueryFailure> {
    Err(QueryFailure {
        kind,
        diagnostic: diagnostic.to_owned(),
    })
}

fn cache(action: ActionSources) -> Vec<SetSources> {
    vec![SetSources {
        set: SetId::Hands,
        refreshed_at: Instant::now(),
        active: true,
        actions: vec![action],
    }]
}

fn paths(path: &str, localized: &str) -> Result<Vec<BoundSource>, QueryFailure> {
    identify_sources(
        vec![0],
        |_| Ok(path.to_owned()),
        |_| Ok(localized.to_owned()),
    )
}

#[test]
fn query_states_do_not_turn_failure_diagnostics_into_path_evidence() {
    let mut published = ObservationSources::default();
    assert_eq!(
        published.query(SetId::Hands, "control"),
        SourceQuery::NotEnumerated
    );
    let empty = enumerate_with(|buffer| {
        assert!(buffer.is_empty());
        (sys::Result::SUCCESS, 0)
    })
    .unwrap();
    let identified = identify_sources(
        empty,
        |_| panic!("empty has no paths"),
        |_| panic!("empty has no names"),
    );
    published.refresh(&cache(ActionSources::from_query("control", identified)));
    assert_eq!(
        published.query(SetId::Hands, "control"),
        SourceQuery::Paths(Vec::new().into())
    );
    let failed = enumerate_with(|_| (sys::Result::ERROR_RUNTIME_FAILURE, 0));
    assert_eq!(
        failed.as_ref().unwrap_err().kind,
        QueryFailureKind::Enumeration
    );
    let identified = failed.and_then(|sources| {
        identify_sources(
            sources,
            |_| panic!("failed enumeration has no paths"),
            |_| panic!("failed enumeration has no names"),
        )
    });
    published.refresh(&cache(ActionSources::from_query("control", identified)));
    assert_eq!(
        published.query(SetId::Hands, "control"),
        SourceQuery::Failed
    );
    let revision = published.revision;
    let failed = identify_sources(
        vec![0, 1],
        |index| {
            if index == 0 {
                Ok("/user/hand/left/input/a/click".into())
            } else {
                Err("new diagnostic".into())
            }
        },
        |_| Ok("label".into()),
    );
    assert_eq!(
        failed.as_ref().unwrap_err().kind,
        QueryFailureKind::PathConversion
    );
    published.refresh(&cache(ActionSources::from_query("control", failed)));
    assert_eq!(
        published.query(SetId::Hands, "control"),
        SourceQuery::Failed
    );
    assert_eq!(published.revision, revision);
    for diagnostic in ["runtime text one", "runtime text two"] {
        published.refresh(&cache(ActionSources::from_query(
            "control",
            failure(QueryFailureKind::PathConversion, diagnostic),
        )));
        assert_eq!(published.revision, revision);
    }
}

#[test]
fn count_path_limits_and_retry_exhaustion_keep_typed_outcomes() {
    let count = enumerate_with(|_| (sys::Result::SUCCESS, MAX_SOURCES as u32 + 1)).unwrap_err();
    assert_eq!(count.kind, QueryFailureKind::Limit);
    assert_eq!(count.observation(), SourceQuery::LimitExceeded);
    let path = identify_sources(
        vec![0],
        |_| Ok("x".repeat(MAX_SOURCE_PATH + 1)),
        |_| Ok(String::new()),
    )
    .unwrap_err();
    assert_eq!(path.observation(), SourceQuery::LimitExceeded);
    let mut calls = 0;
    let retries = enumerate_with(|buffer| {
        calls += 1;
        if buffer.is_empty() {
            (sys::Result::SUCCESS, 1)
        } else {
            (sys::Result::ERROR_SIZE_INSUFFICIENT, 2)
        }
    })
    .unwrap_err();
    assert_eq!(calls, 6);
    assert_eq!(retries.kind, QueryFailureKind::RetryExhausted);
    assert_eq!(retries.observation(), SourceQuery::Failed);
    let returned_limit = enumerate_with(|buffer| {
        if buffer.is_empty() {
            (sys::Result::SUCCESS, 1)
        } else {
            (sys::Result::ERROR_SIZE_INSUFFICIENT, MAX_SOURCES as u32 + 1)
        }
    })
    .unwrap_err();
    assert_eq!(returned_limit.observation(), SourceQuery::LimitExceeded);
}

#[test]
fn refresh_revision_tracks_evidence_not_localized_or_error_text() {
    let mut published = ObservationSources::default();
    published.refresh(&cache(ActionSources::from_query(
        "control",
        paths("/user/hand/left/input/a/click", "first label"),
    )));
    let first = published.revision;
    published.refresh(&cache(ActionSources::from_query(
        "control",
        paths("/user/hand/left/input/a/click", "other label"),
    )));
    assert_eq!(published.revision, first);
    published.refresh(&cache(ActionSources::from_query(
        "control",
        paths("/user/hand/right/input/b/click", "other label"),
    )));
    assert_eq!(published.revision, first + 1);
    published.refresh(&cache(ActionSources::from_query(
        "control",
        failure(QueryFailureKind::Enumeration, "one"),
    )));
    let failed = published.revision;
    published.refresh(&cache(ActionSources::from_query(
        "control",
        failure(QueryFailureKind::RetryExhausted, "two"),
    )));
    assert_eq!(published.revision, failed);
    let mut generation = (0, None);
    let key = |revision| Interpretation {
        profiles: [InteractionProfile::OculusTouch; 2],
        dominant: Hand::Right,
        revision,
    };
    let before = advance_interpretation(&mut generation, key(published.revision));
    published.refresh(&cache(ActionSources::from_query(
        "control",
        failure(QueryFailureKind::PathConversion, "changed text"),
    )));
    assert_eq!(
        advance_interpretation(&mut generation, key(published.revision)),
        before
    );
    published.refresh(&cache(ActionSources::from_query(
        "control",
        paths("/user/hand/right/input/b/click", "restored"),
    )));
    assert_eq!(published.revision, failed + 1);
    assert_eq!(
        advance_interpretation(&mut generation, key(published.revision)),
        before + 1
    );
}
