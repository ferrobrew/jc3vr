"""Read a vehicle's interaction graphs from its entity property file (``.epe``).

An interaction graph is an RTPC object of class ``CInteractionGraph`` inside the vehicle's
``CInteractionContext``. Its children are its nodes (``CInteractionGraphPointNode`` and the other
node classes), its entries (``CInteractionGraphEntry``), and the graph's own properties, among them
the animation modifier its users take (``modifier``). Each object carries an ``_object_id``, by
which entries name the node they lead to.

The property names are the engine's (``CInteractionGraph::SetEntryProperties``,
``SetNodeProperties``, ``SetPointNodeProperties``, and ``SetDeformablePointNodeProperties``); the
ones whose strings are not known are kept as their hashes, named here after the members the
engine reads them into.
"""

from dataclasses import dataclass, field

import rtpc
from archives import hashlittle

CLASS = hashlittle("_class")
NAME = hashlittle("name")
OBJECT_ID = hashlittle("_object_id")
MODIFIER = hashlittle("modifier")
STATE = hashlittle("state")
WORLD = hashlittle("world")
ALIGNMENT_BONE = hashlittle("alignment_bone")
ENTRY_TRANSFORM = hashlittle("entry_transform")
USER_TYPE = hashlittle("user_type")
ENTRY_NODE = hashlittle("entry_node")
# CInteractionGraphEntry: the act that starts it (m_EventId), and its identifier (m_Identifier),
# which a vehicle entry sets to the door slot of the seat it leads to.
ENTRY_ACT = 0xBA25AE52
ENTRY_IDENTIFIER = 0x463A6E5C
# CInteractionGraphPointNode: whether the point node stands in for a deformable point node
# (m_TempPretendImADeformablePoint), and the deformable point's name: the deform point of the
# vehicle whose transform places the node.
POINT_IS_DEFORMABLE = 0x1752505B
DEFORM_POINT = 0x4B7BDE5B

USER_TYPES = {0: "player", 1: "npc", 2: "anyone"}
POINT_NODE = "CInteractionGraphPointNode"
DEFORMABLE_POINT_NODE = "CInteractionGraphDeformablePointNode"


@dataclass
class Node:
    name: str
    kind: str
    world: tuple | None
    deform_point: str | None
    modifier: str | None


@dataclass
class Entry:
    name: str
    state: str | None
    act: str | None
    modifier: str | None
    alignment_bone: str | None
    transform: tuple | None
    user_type: str
    identifier: int
    node: Node | None


@dataclass
class Graph:
    modifier: str | None
    entries: list[Entry] = field(default_factory=list)


def graphs(epe: bytes) -> list[Graph]:
    """Every interaction graph in the property file."""
    out = []
    for node in _walk(rtpc.parse(epe)):
        if node.get(CLASS) == "CInteractionGraph":
            out.append(_graph(node))
    return out


def _walk(node):
    yield node
    for child in node.children:
        yield from _walk(child)


def _graph(g) -> Graph:
    nodes = {}
    for child in g.children:
        kind = child.get(CLASS) or ""
        if not kind.endswith("Node"):
            continue
        deformable = kind == DEFORMABLE_POINT_NODE or (kind == POINT_NODE and child.get(POINT_IS_DEFORMABLE))
        nodes[child.get(OBJECT_ID)] = Node(
            name=child.get(NAME) or "",
            kind=DEFORMABLE_POINT_NODE if deformable else kind,
            world=child.get(WORLD),
            deform_point=child.get(DEFORM_POINT) or None if deformable else None,
            modifier=child.get(MODIFIER) or None,
        )
    graph = Graph(modifier=g.get(MODIFIER) or None)
    for child in g.children:
        if child.get(CLASS) != "CInteractionGraphEntry":
            continue
        graph.entries.append(
            Entry(
                name=child.get(NAME) or "",
                state=child.get(STATE) or None,
                act=child.get(ENTRY_ACT) or None,
                modifier=child.get(MODIFIER) or None,
                alignment_bone=child.get(ALIGNMENT_BONE) or None,
                transform=child.get(ENTRY_TRANSFORM),
                user_type=USER_TYPES.get(child.get(USER_TYPE, 0), "unknown"),
                identifier=child.get(ENTRY_IDENTIFIER, -1),
                node=nodes.get(child.get(ENTRY_NODE)),
            )
        )
    return graph
