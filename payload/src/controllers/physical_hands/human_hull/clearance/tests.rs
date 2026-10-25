use super::*;

#[test]
fn only_complete_world_evidence_returns_a_borrowed_clearance_proof() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    let decision = evaluate(fixture.request(&phase, &leaves));
    let ClearanceDecision::Clear(proof) = decision else {
        panic!("human-hull clearance: complete fixture must be clear");
    };
    assert!(std::ptr::eq(proof.owner, &fixture.owner));
    assert!(std::ptr::eq(proof.phase, &phase));
    assert!(std::ptr::eq(proof.dispatcher, &fixture.dispatcher));
    assert!(std::ptr::eq(proof.leaf_contract, &fixture.gsk));
    assert_eq!(proof.path, fixture.path());
    assert_eq!(proof.minimum_gap_m, 0.5);
}

#[test]
fn every_missing_proof_obligation_blocks_clearance() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    let baseline = fixture.request(&phase, &leaves);
    let cases = [
        (
            ClearanceRequest {
                dispatcher: None,
                ..baseline
            },
            IndeterminateReason::MissingDispatcherContract,
        ),
        (
            ClearanceRequest {
                owner: None,
                ..baseline
            },
            IndeterminateReason::MissingOwnerLifetime,
        ),
        (
            ClearanceRequest {
                phase: None,
                ..baseline
            },
            IndeterminateReason::MissingReadPhase,
        ),
        (
            ClearanceRequest {
                leaf_contract: None,
                ..baseline
            },
            IndeterminateReason::MissingLeafContract,
        ),
        (
            ClearanceRequest {
                path: None,
                ..baseline
            },
            IndeterminateReason::MissingPath,
        ),
        (
            ClearanceRequest {
                budget: None,
                ..baseline
            },
            IndeterminateReason::MissingBudget,
        ),
        (
            ClearanceRequest {
                inventory: None,
                ..baseline
            },
            IndeterminateReason::MissingTargetInventory,
        ),
    ];
    for (request, reason) in cases {
        assert_indeterminate(request, reason);
    }
    assert_indeterminate(
        ClearanceRequest::unavailable(),
        IndeterminateReason::MissingDispatcherContract,
    );
}

#[test]
fn an_empty_hit_collector_does_not_prove_clearance() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    let mut request = fixture.request(&phase, &leaves);
    request.leaves = &[];
    request.budget.as_mut().unwrap().completed_leaf_queries = 0;
    assert_indeterminate(request, IndeterminateReason::MissingLeaf(id(1)));
    request.inventory.as_mut().unwrap().targets = &[];
    assert_indeterminate(request, IndeterminateReason::EmptyTargetInventory);
}

#[test]
fn one_convex_pair_cannot_prove_general_world_clearance() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    let mut request = fixture.request(&phase, &leaves);
    request.dispatcher.as_mut().unwrap().scope = CoverageScope::ConvexPairOnly;
    request.inventory.as_mut().unwrap().targets = &fixture.targets[..1];
    request.leaves = &leaves[..1];
    request.budget.as_mut().unwrap().completed_leaf_queries = 1;
    assert_indeterminate(request, IndeterminateReason::IncompleteWorldCoverage);
}

#[test]
fn faults_and_nonexact_query_terminations_are_indeterminate_at_every_layer() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    for failure in [
        QueryFailure::Fault,
        QueryFailure::BudgetExhausted,
        QueryFailure::UnsupportedShape,
        QueryFailure::Heuristic,
        QueryFailure::MemoryLimit,
        QueryFailure::IterationLimit,
        QueryFailure::Incomplete,
    ] {
        let mut leaves = fixture.leaves(&phase);
        let mut request = fixture.request(&phase, &leaves);
        request.dispatcher.as_mut().unwrap().completion = Completion::Failed(failure);
        assert_indeterminate(request, IndeterminateReason::Dispatcher(failure));
        let mut request = fixture.request(&phase, &leaves);
        request.inventory.as_mut().unwrap().completion = Completion::Failed(failure);
        assert_indeterminate(request, IndeterminateReason::Inventory(failure));
        leaves[1].status = LeafStatus::Failed(failure);
        assert_indeterminate(
            fixture.request(&phase, &leaves),
            IndeterminateReason::Leaf {
                target: id(2),
                failure,
            },
        );
    }
}

#[test]
fn raw_status_and_output_initialization_are_independent_obligations() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    for raw in [0, 1, -1, i32::MIN, i32::MAX] {
        let mut leaves = fixture.leaves(&phase);
        leaves[0].status = LeafStatus::OrdinaryGsk { raw };
        assert_indeterminate(
            fixture.request(&phase, &leaves),
            IndeterminateReason::UnknownOrdinaryGskStatus { target: id(1), raw },
        );
        leaves[0].output = DistanceOutput::Uninitialized;
        assert_indeterminate(
            fixture.request(&phase, &leaves),
            IndeterminateReason::UnknownOrdinaryGskStatus { target: id(1), raw },
        );
    }
    let mut leaves = fixture.leaves(&phase);
    leaves[0].output = DistanceOutput::Uninitialized;
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::UninitializedOutput(id(1)),
    );
}

#[test]
fn touching_and_rounded_overlap_are_obstructions() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    for (core_distance_m, expected_gap) in [(0.75, 0.0), (0.5, -0.25), (-0.25, -1.0)] {
        let mut leaves = fixture.leaves(&phase);
        leaves[0].output = initialized(core_distance_m, 0.25, 0.5);
        assert_eq!(
            evaluate(fixture.request(&phase, &leaves)).status(),
            ClearanceStatus::Obstructed(Obstruction {
                target: id(1),
                rounded_gap_m: expected_gap,
                required_gap_m: 0.0,
            }),
        );
    }
}

#[test]
fn the_required_gap_is_strict_and_has_no_floor_exception() {
    let mut fixture = Fixture::new();
    fixture.poses[0].translation_m[1] = -2.0;
    let phase = fixture.phase();
    let mut leaves = fixture.leaves(&phase);
    leaves[0].output = initialized(0.875, 0.25, 0.5);
    let mut request = fixture.request(&phase, &leaves);
    request.required_gap_m = 0.125;
    assert!(matches!(
        evaluate(request),
        ClearanceDecision::Obstructed(_)
    ));
    request.required_gap_m = 0.0;
    assert!(matches!(evaluate(request), ClearanceDecision::Clear(_)));
    leaves[0].output = initialized(0.5, 0.25, 0.5);
    assert!(matches!(
        evaluate(fixture.request(&phase, &leaves)),
        ClearanceDecision::Obstructed(_)
    ));
}

#[test]
fn nonfinite_inputs_and_invalid_radii_are_indeterminate() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for field in 0..3 {
            let mut values = [1.25, 0.25, 0.5];
            values[field] = invalid;
            let mut leaves = fixture.leaves(&phase);
            leaves[0].output = initialized(values[0], values[1], values[2]);
            assert_indeterminate(
                fixture.request(&phase, &leaves),
                IndeterminateReason::NonFiniteDistance(id(1)),
            );
        }
    }
    for (query_radius_m, target_radius_m) in [(-0.25, 0.5), (0.25, -0.5)] {
        let mut leaves = fixture.leaves(&phase);
        leaves[0].output = initialized(1.25, query_radius_m, target_radius_m);
        assert_indeterminate(
            fixture.request(&phase, &leaves),
            IndeterminateReason::InvalidRadius(id(1)),
        );
    }
    let mut leaves = fixture.leaves(&phase);
    leaves[0].output = initialized(-f32::MAX, f32::MAX, f32::MAX);
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::NonFiniteRoundedDistance(id(1)),
    );
    for required_gap_m in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1] {
        let mut request = fixture.request(&phase, &leaves);
        request.required_gap_m = required_gap_m;
        assert_indeterminate(request, IndeterminateReason::InvalidRequiredGap);
    }
}

#[test]
fn exhausted_or_inconsistent_budgets_block_clearance() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    for budget in [
        QueryBudget {
            maximum_leaf_queries: 2,
            completed_leaf_queries: 2,
            exhausted: true,
        },
        QueryBudget {
            maximum_leaf_queries: 0,
            completed_leaf_queries: 0,
            exhausted: false,
        },
        QueryBudget {
            maximum_leaf_queries: 1,
            completed_leaf_queries: 2,
            exhausted: false,
        },
    ] {
        let mut request = fixture.request(&phase, &leaves);
        request.budget = Some(budget);
        assert_indeterminate(request, IndeterminateReason::BudgetExhausted);
    }
    let mut request = fixture.request(&phase, &leaves);
    request.budget.as_mut().unwrap().completed_leaf_queries = 1;
    assert_indeterminate(request, IndeterminateReason::QueryAccountingMismatch);
}

#[test]
fn every_inventory_leaf_requires_exactly_one_supported_result() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let mut leaves = fixture.leaves(&phase);
    leaves[1].target = id(1);
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::DuplicateLeaf(id(1)),
    );
    let leaves = fixture.leaves(&phase);
    let mut request = fixture.request(&phase, &leaves[..1]);
    request.budget.as_mut().unwrap().completed_leaf_queries = 1;
    assert_indeterminate(request, IndeterminateReason::MissingLeaf(id(2)));
    let extra = [
        leaves[0],
        leaves[1],
        LeafEvidence {
            target: id(3),
            ..leaves[0]
        },
    ];
    let mut request = fixture.request(&phase, &extra);
    request.budget.as_mut().unwrap().maximum_leaf_queries = 3;
    request.budget.as_mut().unwrap().completed_leaf_queries = 3;
    assert_indeterminate(request, IndeterminateReason::UnexpectedLeaf(id(3)));
    let duplicate = [fixture.targets[0], fixture.targets[0]];
    let mut request = fixture.request(&phase, &leaves);
    request.inventory.as_mut().unwrap().targets = &duplicate;
    assert_indeterminate(request, IndeterminateReason::DuplicateTarget(id(1)));
    let unsupported = [
        fixture.targets[0],
        TargetLeaf {
            id: id(2),
            shape: TargetShape::Unsupported,
        },
    ];
    request.inventory.as_mut().unwrap().targets = &unsupported;
    assert_indeterminate(request, IndeterminateReason::UnsupportedTarget(id(2)));
}

#[test]
fn an_obstruction_does_not_hide_a_later_failed_leaf() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let mut leaves = fixture.leaves(&phase);
    leaves[0].output = initialized(0.25, 0.25, 0.5);
    leaves[1].status = LeafStatus::Failed(QueryFailure::Fault);
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::Leaf {
            target: id(2),
            failure: QueryFailure::Fault,
        },
    );
    leaves.swap(0, 1);
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::Leaf {
            target: id(2),
            failure: QueryFailure::Fault,
        },
    );
}

#[test]
fn owner_and_read_phase_identity_are_required_at_every_layer() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let other_phase = fixture.phase();
    let other_owner = OwnerLifetime { _private: 0 };
    let mut leaves = fixture.leaves(&phase);
    let mut request = fixture.request(&phase, &leaves);
    request.owner = Some(&other_owner);
    assert_indeterminate(request, IndeterminateReason::OwnerReadPhaseMismatch);
    let mut request = fixture.request(&phase, &leaves);
    request.dispatcher.as_mut().unwrap().phase = &other_phase;
    assert_indeterminate(request, IndeterminateReason::StaleEvidence);
    let mut request = fixture.request(&phase, &leaves);
    request.inventory.as_mut().unwrap().phase = &other_phase;
    assert_indeterminate(request, IndeterminateReason::StaleEvidence);
    leaves[1].phase = &other_phase;
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::StaleEvidence,
    );
}

#[test]
fn evidence_for_another_pose_does_not_clear_the_requested_pose() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let other_poses = [CandidatePose {
        translation_m: [1.0, 0.0, 0.0],
        ..fixture.poses[0]
    }];
    let other_path = TypedEvidencePath::new(&other_poses).unwrap();
    let mut leaves = fixture.leaves(&phase);
    let mut request = fixture.request(&phase, &leaves);
    request.dispatcher.as_mut().unwrap().path = other_path;
    assert_indeterminate(request, IndeterminateReason::EvidencePathMismatch);
    let mut request = fixture.request(&phase, &leaves);
    request.inventory.as_mut().unwrap().path = other_path;
    assert_indeterminate(request, IndeterminateReason::EvidencePathMismatch);
    leaves[1].path = other_path;
    assert_indeterminate(
        fixture.request(&phase, &leaves),
        IndeterminateReason::EvidencePathMismatch,
    );
}

#[test]
fn paths_reject_empty_nonfinite_and_invalid_rotation_inputs() {
    assert_eq!(TypedEvidencePath::new(&[]), Err(PathError::Empty));
    let fixture = Fixture::new();
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for index in 0..7 {
            let mut poses = fixture.poses;
            if index < 3 {
                poses[0].translation_m[index] = invalid;
            } else {
                poses[0].rotation_xyzw[index - 3] = invalid;
            }
            assert_eq!(
                TypedEvidencePath::new(&poses),
                Err(PathError::NonFinitePose { index: 0 })
            );
        }
    }
    for rotation_xyzw in [[0.0; 4], [0.0, 0.0, 0.0, 2.0], [f32::MAX; 4]] {
        let poses = [CandidatePose {
            rotation_xyzw,
            ..fixture.poses[0]
        }];
        assert_eq!(
            TypedEvidencePath::new(&poses),
            Err(PathError::InvalidRotation { index: 0 })
        );
    }
}

#[test]
fn finite_endpoints_do_not_prove_a_swept_path() {
    let fixture = Fixture::new();
    let phase = fixture.phase();
    let leaves = fixture.leaves(&phase);
    let poses = [
        fixture.poses[0],
        CandidatePose {
            translation_m: [1.0, 0.0, 0.0],
            ..fixture.poses[0]
        },
    ];
    let mut request = fixture.request(&phase, &leaves);
    request.path = Some(TypedEvidencePath::new(&poses).unwrap());
    assert_indeterminate(request, IndeterminateReason::UnsupportedPath);
}

fn assert_indeterminate(request: ClearanceRequest<'_>, reason: IndeterminateReason) {
    assert_eq!(
        evaluate(request).status(),
        ClearanceStatus::Indeterminate(reason)
    );
}

fn id(value: u64) -> TargetLeafId {
    TargetLeafId(NonZeroU64::new(value).unwrap())
}

fn initialized(core_distance_m: f32, query_radius_m: f32, target_radius_m: f32) -> DistanceOutput {
    DistanceOutput::Initialized {
        core_distance_m,
        query_radius_m,
        target_radius_m,
    }
}

struct Fixture {
    owner: OwnerLifetime,
    dispatcher: DispatcherContract,
    gsk: OrdinaryGskContract,
    poses: [CandidatePose; 1],
    targets: [TargetLeaf; 2],
}

impl Fixture {
    fn new() -> Self {
        Self {
            owner: OwnerLifetime { _private: 0 },
            dispatcher: DispatcherContract { _private: () },
            // This synthetic fixture value is not an engine status mapping.
            gsk: OrdinaryGskContract {
                accepted_raw_status: 73,
            },
            poses: [CandidatePose {
                translation_m: [0.0; 3],
                rotation_xyzw: [0.0, 0.0, 0.0, 1.0],
            }],
            targets: [
                TargetLeaf {
                    id: id(1),
                    shape: TargetShape::Convex,
                },
                TargetLeaf {
                    id: id(2),
                    shape: TargetShape::Convex,
                },
            ],
        }
    }

    fn phase(&self) -> ReadPhase<'_> {
        ReadPhase {
            owner: &self.owner,
            _private: (),
        }
    }

    fn path(&self) -> TypedEvidencePath<'_> {
        TypedEvidencePath::new(&self.poses).unwrap()
    }

    fn leaves<'a>(&'a self, phase: &'a ReadPhase<'a>) -> [LeafEvidence<'a>; 2] {
        self.targets.map(|target| LeafEvidence {
            phase,
            path: self.path(),
            target: target.id,
            status: LeafStatus::OrdinaryGsk {
                raw: self.gsk.accepted_raw_status,
            },
            output: initialized(1.25, 0.25, 0.5),
        })
    }

    fn request<'a>(
        &'a self,
        phase: &'a ReadPhase<'a>,
        leaves: &'a [LeafEvidence<'a>],
    ) -> ClearanceRequest<'a> {
        ClearanceRequest {
            path: Some(self.path()),
            required_gap_m: 0.0,
            owner: Some(&self.owner),
            phase: Some(phase),
            dispatcher: Some(DispatcherEvidence {
                contract: &self.dispatcher,
                phase,
                path: self.path(),
                scope: CoverageScope::GeneralWorld,
                completion: Completion::Complete,
            }),
            leaf_contract: Some(&self.gsk),
            inventory: Some(TargetInventory {
                phase,
                path: self.path(),
                completion: Completion::Complete,
                targets: &self.targets,
            }),
            leaves,
            budget: Some(QueryBudget {
                maximum_leaf_queries: 2,
                completed_leaf_queries: leaves.len(),
                exhausted: false,
            }),
        }
    }
}
