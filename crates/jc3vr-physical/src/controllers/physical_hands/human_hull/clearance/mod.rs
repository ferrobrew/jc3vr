//! Test-only evidence checks for a proposed human-hull activation pose.
//!
//! A clear result requires complete world coverage and accepted distance outputs for every target
//! leaf. It grants no engine access or mutation authority. These checks remain test-only until a
//! native adapter can prove the owner, read-phase, dispatcher, and ordinary-GSK contracts.

use std::num::NonZeroU64;

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CandidatePose {
    pub(crate) translation_m: [f32; 3],
    pub(crate) rotation_xyzw: [f32; 4],
}

/// A nonempty sequence of finite poses. This does not prove clearance between poses.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct TypedEvidencePath<'a> {
    poses: &'a [CandidatePose],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PathError {
    Empty,
    NonFinitePose { index: usize },
    InvalidRotation { index: usize },
}

impl<'a> TypedEvidencePath<'a> {
    pub(crate) fn new(poses: &'a [CandidatePose]) -> Result<Self, PathError> {
        if poses.is_empty() {
            return Err(PathError::Empty);
        }
        for (index, pose) in poses.iter().enumerate() {
            if !pose.translation_m.iter().all(|value| value.is_finite())
                || !pose.rotation_xyzw.iter().all(|value| value.is_finite())
            {
                return Err(PathError::NonFinitePose { index });
            }
            let length_squared = pose
                .rotation_xyzw
                .iter()
                .map(|value| value * value)
                .sum::<f32>();
            if !length_squared.is_finite() || (length_squared - 1.0).abs() > 1.0e-4 {
                return Err(PathError::InvalidRotation { index });
            }
        }
        Ok(Self { poses })
    }
}

/// The adapter must retain the proposed hull and world shapes for the evidence lifetime.
#[derive(Debug)]
pub(crate) struct OwnerLifetime {
    _private: u8,
}

/// The adapter must exclude relevant readers and writers while this token is borrowed.
#[derive(Debug)]
pub(crate) struct ReadPhase<'a> {
    owner: &'a OwnerLifetime,
    _private: (),
}

/// The dispatcher must expose completion, unsupported shapes, and hidden leaf failures.
#[derive(Debug)]
pub(crate) struct DispatcherContract {
    _private: (),
}

/// The accepted raw status and usable output fields require a verified ordinary-GSK contract.
#[derive(Debug)]
pub(crate) struct OrdinaryGskContract {
    accepted_raw_status: i32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TargetLeafId(pub(crate) NonZeroU64);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TargetShape {
    Convex,
    Unsupported,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TargetLeaf {
    pub(crate) id: TargetLeafId,
    pub(crate) shape: TargetShape,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum QueryFailure {
    Fault,
    BudgetExhausted,
    UnsupportedShape,
    Heuristic,
    MemoryLimit,
    IterationLimit,
    Incomplete,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Completion {
    Complete,
    Failed(QueryFailure),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CoverageScope {
    GeneralWorld,
    ConvexPairOnly,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct DispatcherEvidence<'a> {
    pub(crate) contract: &'a DispatcherContract,
    pub(crate) phase: &'a ReadPhase<'a>,
    pub(crate) path: TypedEvidencePath<'a>,
    pub(crate) scope: CoverageScope,
    pub(crate) completion: Completion,
}

/// The inventory includes every potentially intersecting leaf, not only collector hits.
#[derive(Debug, Copy, Clone)]
pub(crate) struct TargetInventory<'a> {
    pub(crate) phase: &'a ReadPhase<'a>,
    pub(crate) path: TypedEvidencePath<'a>,
    pub(crate) completion: Completion,
    pub(crate) targets: &'a [TargetLeaf],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum LeafStatus {
    OrdinaryGsk { raw: i32 },
    Failed(QueryFailure),
}

/// Initialization is independent of whether the query returned an accepted status.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum DistanceOutput {
    Uninitialized,
    Initialized {
        core_distance_m: f32,
        query_radius_m: f32,
        target_radius_m: f32,
    },
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct LeafEvidence<'a> {
    pub(crate) phase: &'a ReadPhase<'a>,
    pub(crate) path: TypedEvidencePath<'a>,
    pub(crate) target: TargetLeafId,
    pub(crate) status: LeafStatus,
    pub(crate) output: DistanceOutput,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct QueryBudget {
    pub(crate) maximum_leaf_queries: usize,
    pub(crate) completed_leaf_queries: usize,
    pub(crate) exhausted: bool,
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct ClearanceRequest<'a> {
    pub(crate) path: Option<TypedEvidencePath<'a>>,
    pub(crate) required_gap_m: f32,
    pub(crate) owner: Option<&'a OwnerLifetime>,
    pub(crate) phase: Option<&'a ReadPhase<'a>>,
    pub(crate) dispatcher: Option<DispatcherEvidence<'a>>,
    pub(crate) leaf_contract: Option<&'a OrdinaryGskContract>,
    pub(crate) inventory: Option<TargetInventory<'a>>,
    pub(crate) leaves: &'a [LeafEvidence<'a>],
    pub(crate) budget: Option<QueryBudget>,
}

impl ClearanceRequest<'_> {
    pub(crate) const fn unavailable() -> Self {
        Self {
            path: None,
            required_gap_m: 0.0,
            owner: None,
            phase: None,
            dispatcher: None,
            leaf_contract: None,
            inventory: None,
            leaves: &[],
            budget: None,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum IndeterminateReason {
    MissingDispatcherContract,
    MissingOwnerLifetime,
    MissingReadPhase,
    OwnerReadPhaseMismatch,
    StaleEvidence,
    EvidencePathMismatch,
    IncompleteWorldCoverage,
    Dispatcher(QueryFailure),
    MissingLeafContract,
    MissingPath,
    UnsupportedPath,
    InvalidRequiredGap,
    MissingBudget,
    BudgetExhausted,
    QueryAccountingMismatch,
    MissingTargetInventory,
    Inventory(QueryFailure),
    EmptyTargetInventory,
    DuplicateTarget(TargetLeafId),
    UnsupportedTarget(TargetLeafId),
    MissingLeaf(TargetLeafId),
    DuplicateLeaf(TargetLeafId),
    UnexpectedLeaf(TargetLeafId),
    Leaf {
        target: TargetLeafId,
        failure: QueryFailure,
    },
    UnknownOrdinaryGskStatus {
        target: TargetLeafId,
        raw: i32,
    },
    UninitializedOutput(TargetLeafId),
    NonFiniteDistance(TargetLeafId),
    InvalidRadius(TargetLeafId),
    NonFiniteRoundedDistance(TargetLeafId),
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Obstruction {
    pub(crate) target: TargetLeafId,
    pub(crate) rounded_gap_m: f32,
    pub(crate) required_gap_m: f32,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum ClearanceStatus {
    Clear { minimum_gap_m: f32 },
    Obstructed(Obstruction),
    Indeterminate(IndeterminateReason),
}

/// This proof only records the pure policy result while its evidence remains borrowed.
#[derive(Debug)]
pub(crate) struct ClearanceProof<'a> {
    owner: &'a OwnerLifetime,
    phase: &'a ReadPhase<'a>,
    dispatcher: &'a DispatcherContract,
    leaf_contract: &'a OrdinaryGskContract,
    path: TypedEvidencePath<'a>,
    minimum_gap_m: f32,
}

#[derive(Debug)]
pub(crate) enum ClearanceDecision<'a> {
    Clear(ClearanceProof<'a>),
    Obstructed(Obstruction),
    Indeterminate(IndeterminateReason),
}

impl ClearanceDecision<'_> {
    pub(crate) fn status(&self) -> ClearanceStatus {
        match self {
            Self::Clear(proof) => ClearanceStatus::Clear {
                minimum_gap_m: proof.minimum_gap_m,
            },
            Self::Obstructed(obstruction) => ClearanceStatus::Obstructed(*obstruction),
            Self::Indeterminate(reason) => ClearanceStatus::Indeterminate(*reason),
        }
    }
}

/// Evaluates evidence only. No result from this function activates or restores a collider.
pub(crate) fn evaluate<'a>(request: ClearanceRequest<'a>) -> ClearanceDecision<'a> {
    match check(request) {
        Ok(proof) => ClearanceDecision::Clear(proof),
        Err(CheckFailure::Obstructed(obstruction)) => ClearanceDecision::Obstructed(obstruction),
        Err(CheckFailure::Indeterminate(reason)) => ClearanceDecision::Indeterminate(reason),
    }
}

#[derive(Debug)]
enum CheckFailure {
    Obstructed(Obstruction),
    Indeterminate(IndeterminateReason),
}

impl From<IndeterminateReason> for CheckFailure {
    fn from(reason: IndeterminateReason) -> Self {
        Self::Indeterminate(reason)
    }
}

fn check<'a>(request: ClearanceRequest<'a>) -> Result<ClearanceProof<'a>, CheckFailure> {
    let dispatcher = request
        .dispatcher
        .ok_or(IndeterminateReason::MissingDispatcherContract)?;
    let owner = request
        .owner
        .ok_or(IndeterminateReason::MissingOwnerLifetime)?;
    let phase = request.phase.ok_or(IndeterminateReason::MissingReadPhase)?;
    if !std::ptr::eq(owner, phase.owner) {
        return Err(IndeterminateReason::OwnerReadPhaseMismatch.into());
    }
    require_phase(phase, dispatcher.phase)?;
    if dispatcher.scope != CoverageScope::GeneralWorld {
        return Err(IndeterminateReason::IncompleteWorldCoverage.into());
    }
    if let Completion::Failed(failure) = dispatcher.completion {
        return Err(IndeterminateReason::Dispatcher(failure).into());
    }
    let leaf_contract = request
        .leaf_contract
        .ok_or(IndeterminateReason::MissingLeafContract)?;
    let path = request.path.ok_or(IndeterminateReason::MissingPath)?;
    if path.poses.len() != 1 {
        return Err(IndeterminateReason::UnsupportedPath.into());
    }
    if dispatcher.path != path {
        return Err(IndeterminateReason::EvidencePathMismatch.into());
    }
    if !request.required_gap_m.is_finite() || request.required_gap_m < 0.0 {
        return Err(IndeterminateReason::InvalidRequiredGap.into());
    }
    let budget = request.budget.ok_or(IndeterminateReason::MissingBudget)?;
    if budget.exhausted
        || budget.maximum_leaf_queries == 0
        || budget.completed_leaf_queries > budget.maximum_leaf_queries
    {
        return Err(IndeterminateReason::BudgetExhausted.into());
    }
    if budget.completed_leaf_queries != request.leaves.len() {
        return Err(IndeterminateReason::QueryAccountingMismatch.into());
    }
    let inventory = request
        .inventory
        .ok_or(IndeterminateReason::MissingTargetInventory)?;
    require_phase(phase, inventory.phase)?;
    if inventory.path != path {
        return Err(IndeterminateReason::EvidencePathMismatch.into());
    }
    if let Completion::Failed(failure) = inventory.completion {
        return Err(IndeterminateReason::Inventory(failure).into());
    }
    if inventory.targets.is_empty() {
        return Err(IndeterminateReason::EmptyTargetInventory.into());
    }
    for (index, target) in inventory.targets.iter().enumerate() {
        if inventory.targets[..index]
            .iter()
            .any(|earlier| earlier.id == target.id)
        {
            return Err(IndeterminateReason::DuplicateTarget(target.id).into());
        }
        if target.shape != TargetShape::Convex {
            return Err(IndeterminateReason::UnsupportedTarget(target.id).into());
        }
        let count = request
            .leaves
            .iter()
            .filter(|leaf| leaf.target == target.id)
            .count();
        match count {
            0 => return Err(IndeterminateReason::MissingLeaf(target.id).into()),
            1 => {}
            _ => return Err(IndeterminateReason::DuplicateLeaf(target.id).into()),
        }
    }
    let mut minimum_gap_m = f32::INFINITY;
    let mut obstruction = None;
    for leaf in request.leaves {
        require_phase(phase, leaf.phase)?;
        if leaf.path != path {
            return Err(IndeterminateReason::EvidencePathMismatch.into());
        }
        if !inventory
            .targets
            .iter()
            .any(|target| target.id == leaf.target)
        {
            return Err(IndeterminateReason::UnexpectedLeaf(leaf.target).into());
        }
        let gap_m = rounded_gap(leaf, leaf_contract)?;
        minimum_gap_m = minimum_gap_m.min(gap_m);
        if gap_m <= request.required_gap_m && obstruction.is_none() {
            obstruction = Some(Obstruction {
                target: leaf.target,
                rounded_gap_m: gap_m,
                required_gap_m: request.required_gap_m,
            });
        }
    }
    if let Some(obstruction) = obstruction {
        return Err(CheckFailure::Obstructed(obstruction));
    }
    Ok(ClearanceProof {
        owner,
        phase,
        dispatcher: dispatcher.contract,
        leaf_contract,
        path,
        minimum_gap_m,
    })
}

fn require_phase(
    expected: &ReadPhase<'_>,
    observed: &ReadPhase<'_>,
) -> Result<(), IndeterminateReason> {
    if std::ptr::eq(expected, observed) {
        Ok(())
    } else {
        Err(IndeterminateReason::StaleEvidence)
    }
}

fn rounded_gap(
    leaf: &LeafEvidence<'_>,
    contract: &OrdinaryGskContract,
) -> Result<f32, IndeterminateReason> {
    match leaf.status {
        LeafStatus::OrdinaryGsk { raw } if raw == contract.accepted_raw_status => {}
        LeafStatus::OrdinaryGsk { raw } => {
            return Err(IndeterminateReason::UnknownOrdinaryGskStatus {
                target: leaf.target,
                raw,
            });
        }
        LeafStatus::Failed(failure) => {
            return Err(IndeterminateReason::Leaf {
                target: leaf.target,
                failure,
            });
        }
    }
    let DistanceOutput::Initialized {
        core_distance_m,
        query_radius_m,
        target_radius_m,
    } = leaf.output
    else {
        return Err(IndeterminateReason::UninitializedOutput(leaf.target));
    };
    if !core_distance_m.is_finite() || !query_radius_m.is_finite() || !target_radius_m.is_finite() {
        return Err(IndeterminateReason::NonFiniteDistance(leaf.target));
    }
    if query_radius_m < 0.0 || target_radius_m < 0.0 {
        return Err(IndeterminateReason::InvalidRadius(leaf.target));
    }
    let gap_m = core_distance_m - query_radius_m - target_radius_m;
    if !gap_m.is_finite() {
        return Err(IndeterminateReason::NonFiniteRoundedDistance(leaf.target));
    }
    Ok(gap_m)
}

#[cfg(test)]
mod tests;
