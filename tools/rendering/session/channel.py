"""Shared visual-protocol request channel for launched and attached sessions."""

import json
import os
import re
import time
import uuid
from pathlib import Path

from tools.rendering.protocol import (
    Acknowledgement,
    Command,
    ConfigurationAck,
    FailedAck,
    ProtocolError,
    Request,
    acknowledgement_from_wire,
)
from tools.rendering.session.errors import SessionError


# A stage's settle is bounded by `MAX_SETTLE_SECONDS`, and its teleport streams the destination in
# first; the payload gives up on a stage after seven minutes in all.
STAGE_BUDGET_SECONDS = 450

# A benchmark run waits up to 90 seconds for its configuration, then warms up and records for at
# most 60 and 120 seconds (`MAX_WARMUP_SECONDS` and `MAX_RECORD_SECONDS` in
# `crates/jc3vr-capture/src/testing/benchmark/request.rs`).
BENCHMARK_BUDGET_SECONDS = 300

# How often an outstanding request's acknowledgement is looked for. The payload looks for requests
# every frame in a harness session, so a coarser poll here would dominate short requests.
POLL_SECONDS = 0.005


class RequestChannel:
    """Issue visual-protocol requests and cancel the one this client owns."""

    config_dir: Path
    deadline: float | None
    pending_request: Request | None = None

    def assert_running(self):
        raise NotImplementedError

    def pump_menu(self):
        return False

    def issue(self, request_value: Request) -> Acknowledgement:
        self.assert_running()
        command, request_id = request_value.command, request_value.id
        if not re.fullmatch(r"[A-Za-z0-9_-]+", request_id):
            raise SessionError("session: invalid request ID")
        folder = ("captures" if command in (Command.SCREENSHOT, Command.SNAPSHOT,
                                             Command.SNAPSHOT_RELEASE, Command.BENCHMARK)
                  else "visual")
        acknowledgement = self.config_dir / folder / f"{request_id}.json"
        request = self.config_dir / "test_request.toml"
        if request.exists() or acknowledgement.exists():
            raise SessionError(f"session: outstanding request or stale acknowledgement {request_id}")
        temporary = request.with_suffix(".tmp")
        temporary.write_text(request_value.to_toml())
        temporary.replace(request)
        self.pending_request = request_value
        budget = {Command.CONFIGURE: 75, Command.SCREENSHOT: 60, Command.SNAPSHOT: 60,
                  Command.STAGE: STAGE_BUDGET_SECONDS,
                  Command.BENCHMARK: BENCHMARK_BUDGET_SECONDS}.get(command, 35)
        end = time.monotonic() + budget
        if self.deadline is not None:
            end = min(end, self.deadline)
        while not acknowledgement.exists():
            self.assert_running()
            self.pump_menu()
            if time.monotonic() >= end:
                raise SessionError(f"session: timed out waiting for {command} acknowledgement {request_id}")
            time.sleep(POLL_SECONDS)
        self.assert_running()
        try:
            result = acknowledgement_from_wire(_read_object(acknowledgement), command, request_id)
        except ProtocolError as error:
            raise SessionError(f"session: {command} {request_id}: {error}") from error
        if isinstance(result, ConfigurationAck):
            try:
                if request_value.pose is not None:
                    result.validate_pose(request_value)
                if request_value.motion is not None:
                    result.validate_motion(request_value)
            except ProtocolError as error:
                raise SessionError(f"session: {command} {request_id}: {error}") from error
        return result

    def request(self, request_value: Request) -> Acknowledgement:
        result = self.issue(request_value)
        if isinstance(result, FailedAck):
            raise SessionError(f"session: {request_value.command} {request_value.id}: {result.error}")
        return result

    def cancel_pending_request(self) -> bool:
        owned = self.pending_request
        if owned is None:
            return False
        request = self.config_dir / "test_request.toml"
        expected = owned.to_toml().encode()
        try:
            if request.is_symlink() or request.read_bytes() != expected:
                return False
            cancelled = request.with_name(f"{request.name}.{uuid.uuid4().hex}.cancelled")
            request.rename(cancelled)
        except FileNotFoundError:
            self.pending_request = None
            return False
        if cancelled.read_bytes() != expected:
            try:
                os.link(cancelled, request)
            except FileExistsError:
                pass
            else:
                cancelled.unlink()
            raise SessionError(f"session: pending request changed during cancellation; preserved at {cancelled}")
        self.pending_request = None
        return True


def _read_object(path):
    try:
        value = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise SessionError(f"session: invalid JSON artifact {path}: {error}") from error
    if not isinstance(value, dict):
        raise SessionError(f"session: expected a JSON object in {path}")
    return value
