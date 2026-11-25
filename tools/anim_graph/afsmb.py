"""Decode an animation state machine (``.afsmb``, an RTPC container) into its states, transitions,
conditions, and state tasks.

The container's root has one child, the state machine. Its children are, in file order, the
conditions, the states, the transitions, the state tasks, and the transition event names; each
condition, state, transition, and task is a child object, and the tables that tie them together are
properties of the state machine, of each state, and of each transition. The tables index those
child lists by position, so the children must be read in the order the file stores them. (The
Gibbed property converter sorts children by name in its XML, which breaks the indices; this reads
the binary directly.)

The property name hashes below are ``hashlittle`` of names whose strings are not known; each is
named after what the engine's loader (``NAnimationSystem::CStateMachine::Load``,
``CStateMachine::LoadStateTransitions``, and ``CTransition::Load``) reads it into.

- ``CONDITION_CLASSES``: one class hash per condition, in condition order. A class hash is
  ``hashlittle`` of the class name (``CWeaponWieldedCondition``).
- ``STATE_CLASSES``: one class hash per state (``CState``, ``CAnimationState``, and so on).
- ``TASK_TYPES``: one type hash per state task. These are not hashes of a name; the engine's
  state-task registration table pairs each with its ``CStateTask_<NAME>`` string.
- ``TRANSITION_POINTERS``: big-endian ``u16`` transition indices. A state's
  ``STATE_TRANSITIONS`` is a (first, count) range into it, as are the machine's
  ``GLOBAL_TRANSITIONS`` and ``EXTERNAL_TRANSITIONS``.
- ``CONDITION_POINTERS``: big-endian ``u16`` condition indices. A transition's
  ``TRANSITION_CONDITIONS`` is (first, count, valid-on mask): bit ``j`` of the mask is the value
  the ``j``-th condition must evaluate to. Its ``TRANSITION_METHOD`` is zero for all of them
  (AND) and one for any (OR).
- ``TRANSITION_DESTINATIONS`` and ``TRANSITION_EVENTS``: big-endian ``u16`` per transition, the
  destination state index and the index of the event (the act) that fires it.
- A state's ``StateTasks`` property: big-endian ``u16`` task indices.
- ``INITIAL_STATE``: the index of the initial state.
"""

import struct
from dataclasses import dataclass, field

from archives import hashlittle

CONDITIONS = hashlittle("Conditions")
STATES = hashlittle("States")
TRANSITIONS = hashlittle("Transitions")
STATE_TASKS = hashlittle("StateTasks")
NAME = hashlittle("Name")

EVENT_NAMES = 0x3B3F0F91
CONDITION_CLASSES = 0x8943EE06
STATE_CLASSES = 0x8256798F
TASK_TYPES = 0x401A7EAF
TRANSITION_POINTERS = 0xA3B244FC
CONDITION_POINTERS = 0x42A539B5
TRANSITION_DESTINATIONS = 0x4C2B5478
TRANSITION_EVENTS = 0x6F151DDC
GLOBAL_TRANSITIONS = 0xD6A235DD
EXTERNAL_TRANSITIONS = 0xF709EB6D
INITIAL_STATE = 0xC69700E5
STATE_TRANSITIONS = 0x133A0D9F
TRANSITION_CONDITIONS = 0xEC7E8E68
TRANSITION_METHOD = 0x894029CF

# Property names seen on state tasks whose strings are known.
TASK_PROPERTY_NAMES = {
    hashlittle(n): n
    for n in ("Name", "Bone", "Index", "Type", "Key", "Value", "Segment", "Flags", "Permissions", "Act", "Slot", "State", "Instant", "Layer", "Time")
}

_INTEGER, _FLOAT, _STRING, _INTEGERS, _FLOATS, _BYTES = 1, 2, 3, 9, 10, 11


@dataclass
class Node:
    """An RTPC object with its properties in file order (a list, since keys may repeat)."""

    name_hash: int
    properties: list[tuple[int, object]] = field(default_factory=list)
    children: list["Node"] = field(default_factory=list)

    def get(self, key: int, default=None):
        for k, v in self.properties:
            if k == key:
                return v
        return default

    def child(self, key: int) -> "Node | None":
        return next((c for c in self.children if c.name_hash == key), None)

    @property
    def name(self) -> str:
        value = self.get(NAME)
        return value if isinstance(value, str) else f"{self.name_hash:#010x}"


@dataclass
class Transition:
    event: str
    conditions: list[tuple[str, bool]]
    any_of: bool
    destination: str

    def describe(self) -> str:
        joiner = " | " if self.any_of else " & "
        conditions = joiner.join(name if polarity else "!" + name for name, polarity in self.conditions)
        return f"[{self.event}] {conditions} -> {self.destination}"


@dataclass
class StateMachine:
    initial: str | None
    conditions: list[tuple[str, int, list[tuple[int, object]]]]
    states: list[tuple[str, int]]
    global_transitions: list[Transition]
    external_transitions: list[Transition]
    state_transitions: dict[str, list[Transition]]
    state_tasks: dict[str, list[tuple[int, list[tuple[int, object]]]]]


def parse(data: bytes) -> StateMachine:
    magic, version = struct.unpack_from("<4sI", data, 0)
    if magic != b"RTPC" or version != 1:
        raise ValueError(f"state machine: not a version 1 RTPC ({magic!r}, {version})")
    root = _node(data, 8)
    if not root.children:
        raise ValueError("state machine: the container has no state machine object")
    machine = root.children[0]

    condition_nodes = _children(machine, CONDITIONS, "conditions")
    state_nodes = _children(machine, STATES, "states")
    transition_nodes = _children(machine, TRANSITIONS, "transitions")
    task_node = machine.child(STATE_TASKS)
    task_nodes = task_node.children if task_node else []
    events_node = machine.child(EVENT_NAMES)
    events = [v for _, v in events_node.properties] if events_node else []

    condition_names = [c.name for c in condition_nodes]
    state_names = [s.name for s in state_nodes]
    transition_pointers = _u16s(machine.get(TRANSITION_POINTERS, b""))
    condition_pointers = _u16s(machine.get(CONDITION_POINTERS, b""))
    destinations = _u16s(machine.get(TRANSITION_DESTINATIONS, b""))
    event_indices = _u16s(machine.get(TRANSITION_EVENTS, b""))

    def transition(index: int) -> Transition:
        node = transition_nodes[index]
        conditions = []
        ranged = node.get(TRANSITION_CONDITIONS)
        if ranged:
            first, count, valid_on = ranged
            for j in range(count):
                conditions.append((condition_names[condition_pointers[first + j]], bool((valid_on >> j) & 1)))
        event = events[event_indices[index]] if events and index < len(event_indices) else "?"
        return Transition(event, conditions, node.get(TRANSITION_METHOD, 0) == 1, state_names[destinations[index]])

    def ranged_transitions(node: Node, key: int) -> list[Transition]:
        ranged = node.get(key)
        if not ranged:
            return []
        first, count = ranged[:2]
        return [transition(transition_pointers[first + i]) for i in range(count)]

    condition_classes = [c & 0xFFFFFFFF for c in machine.get(CONDITION_CLASSES, ())]
    state_classes = [c & 0xFFFFFFFF for c in machine.get(STATE_CLASSES, ())]
    task_types = [t & 0xFFFFFFFF for t in machine.get(TASK_TYPES, ())]
    if len(condition_classes) != len(condition_nodes):
        raise ValueError(f"state machine: {len(condition_classes)} condition classes for {len(condition_nodes)} conditions")

    initial = machine.get(INITIAL_STATE)
    return StateMachine(
        initial=state_names[initial] if initial is not None else None,
        conditions=[(c.name, cls, [(k, v) for k, v in c.properties if k != NAME]) for c, cls in zip(condition_nodes, condition_classes)],
        states=list(zip(state_names, state_classes + [0] * (len(state_names) - len(state_classes)))),
        global_transitions=ranged_transitions(machine, GLOBAL_TRANSITIONS),
        external_transitions=ranged_transitions(machine, EXTERNAL_TRANSITIONS),
        state_transitions={s.name: ranged_transitions(s, STATE_TRANSITIONS) for s in state_nodes},
        state_tasks={
            s.name: [(task_types[i], task_nodes[i].properties) for i in _u16s(s.get(STATE_TASKS, b""))] for s in state_nodes
        },
    )


def _children(machine: Node, key: int, what: str) -> list[Node]:
    node = machine.child(key)
    if node is None:
        raise ValueError(f"state machine: no {what} object")
    return node.children


def _u16s(data) -> list[int]:
    if not isinstance(data, bytes):
        return []
    return [int.from_bytes(data[i : i + 2], "big") for i in range(0, len(data) - 1, 2)]


def _node(data: bytes, offset: int) -> Node:
    name_hash, data_offset, property_count, child_count = struct.unpack_from("<IIHH", data, offset)
    node = Node(name_hash)
    for i in range(property_count):
        key, raw, kind = struct.unpack_from("<I4sB", data, data_offset + 9 * i)
        node.properties.append((key, _value(data, raw, kind)))
    children = (data_offset + 9 * property_count + 3) & ~3
    node.children = [_node(data, children + 12 * i) for i in range(child_count)]
    return node


def _value(data: bytes, raw: bytes, kind: int):
    if kind == _INTEGER:
        return struct.unpack("<i", raw)[0]
    if kind == _FLOAT:
        return struct.unpack("<f", raw)[0]
    (offset,) = struct.unpack("<I", raw)
    if kind == _STRING:
        return data[offset : data.index(b"\0", offset)].decode("ascii", errors="replace")
    if kind in (_INTEGERS, _FLOATS):
        (count,) = struct.unpack_from("<I", data, offset)
        return struct.unpack_from(f"<{count}{'i' if kind == _INTEGERS else 'f'}", data, offset + 4)
    if kind == _BYTES:
        (count,) = struct.unpack_from("<I", data, offset)
        return data[offset + 4 : offset + 4 + count]
    # Vectors, matrices, and the other variant types are not needed to read the graph.
    return f"<type {kind}>"
