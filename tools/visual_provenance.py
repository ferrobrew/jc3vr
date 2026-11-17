"""Pose provenance checks shared by the visual harness and comparator."""

from dataclasses import dataclass, field

if __package__:
    from .visual_motion_protocol import ExpectedMotion, MotionEvidence
    from .visual_protocol import (
        Camera,
        Mode,
        OrientationOffset,
        PoseEvidence,
        PoseRequest,
        ProtocolError,
        cameras_match,
        offsets_match,
        valid_request_id,
    )
else:
    from visual_motion_protocol import ExpectedMotion, MotionEvidence
    from visual_protocol import (
        Camera,
        Mode,
        OrientationOffset,
        PoseEvidence,
        PoseRequest,
        ProtocolError,
        cameras_match,
        offsets_match,
        valid_request_id,
    )


@dataclass
class PoseProvenance:
    expected: PoseRequest | None
    anchor: Camera | None = None
    case_id: str | None = None
    offset: OrientationOffset | None = None
    evidence_count: int = 0
    absent_count: int = 0

    def add(self, evidence: PoseEvidence | None, camera=None):
        if evidence is None:
            self.absent_count += 1
            if self.expected is not None:
                raise ValueError(f"pose evidence: requested case {self.expected.case_id} has no pose evidence")
            return
        try:
            if camera is not None:
                evidence.validate_applied(camera)
            if self.expected is not None:
                evidence.validate(self.expected, self.anchor)
            else:
                evidence.validate_composition()
                if not valid_request_id(evidence.case_id):
                    raise ProtocolError("pose evidence: case_id is not an ASCII request ID")
                if self.case_id is None:
                    self.case_id = evidence.case_id
                elif self.case_id != evidence.case_id:
                    raise ProtocolError("pose evidence: case_id changes within a case")
                if self.offset is None:
                    self.offset = evidence.offset
                elif not offsets_match(self.offset, evidence.offset):
                    raise ProtocolError("pose evidence: offset changes within a case")
                if self.anchor is not None and not cameras_match(self.anchor, evidence.anchor):
                    raise ProtocolError("pose evidence: anchor changes within a case")
            if self.anchor is None:
                self.anchor = evidence.anchor
        except ProtocolError as error:
            raise ValueError(f"pose evidence: {error}") from error
        self.evidence_count += 1

    def finish(self):
        if self.expected is not None and self.evidence_count == 0:
            raise ValueError(f"pose evidence: requested case {self.expected.case_id} has no evidence")
        if self.evidence_count and self.absent_count:
            raise ValueError("pose evidence: a case mixes posed and unposed captures")


@dataclass
class MotionProvenance:
    expected: ExpectedMotion | None
    anchor: Camera | None = None
    evidence_count: int = 0
    absent_count: int = 0
    cycles: dict[Mode, set[int]] = field(default_factory=dict)

    def add(self, evidence: MotionEvidence | None, pose, camera, predicted_display_time_ns, mode=None):
        if evidence is None:
            self.absent_count += 1
            if self.expected is not None:
                raise ValueError(f"motion evidence: requested case {self.expected.request.case_id} has no motion evidence")
            return
        if self.expected is None:
            raise ValueError("motion evidence: motion evidence is present without expected_motion")
        try:
            if pose is None:
                raise ProtocolError("motion evidence: capture has no pose evidence")
            anchor = evidence.validate_capture(self.expected.request, self.expected.phase,
                                               self.anchor, pose, camera, predicted_display_time_ns)
        except ProtocolError as error:
            raise ValueError(f"motion evidence: {error}") from error
        if self.anchor is None:
            self.anchor = anchor
        self.evidence_count += 1
        if mode is not None:
            self.cycles.setdefault(mode, set()).add(evidence.cycle)

    def finish(self):
        if self.expected is None:
            return
        if self.evidence_count == 0:
            raise ValueError(f"motion evidence: requested case {self.expected.request.case_id} has no evidence")
        if self.evidence_count < 3:
            raise ValueError("motion evidence: a phase requires at least three independent cycles")
        for mode, cycles in self.cycles.items():
            if len(cycles) < 3:
                raise ValueError(f"motion evidence: {mode} has fewer than three independent cycles")


@dataclass
class RunAnchor:
    anchor: Camera | None = None

    def require(self, candidate: Camera):
        if self.anchor is None:
            self.anchor = candidate
            return
        try:
            same = cameras_match(self.anchor, candidate)
        except ProtocolError as error:
            raise ValueError(f"pose evidence: {error}") from error
        if not same:
            raise ValueError("pose evidence: anchor changes between orientation cases")
