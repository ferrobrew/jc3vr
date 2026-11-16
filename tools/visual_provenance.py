"""Pose provenance checks shared by the visual harness and comparator."""

from dataclasses import dataclass

if __package__:
    from .visual_protocol import (
        Camera,
        OrientationOffset,
        ProtocolError,
        PoseEvidence,
        PoseRequest,
        cameras_match,
        offsets_match,
        valid_request_id,
    )
else:
    from visual_protocol import (
        Camera,
        OrientationOffset,
        ProtocolError,
        PoseEvidence,
        PoseRequest,
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
