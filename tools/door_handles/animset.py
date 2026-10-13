"""Read the animation nodes of an animation set (``.asb``, an RTPC container).

Each leaf animation node is an object whose name property (``NODE_NAME``, a property whose name
string is not known) is its node name, ``ANIM_<...>``. Its clip paths are in a child object
(``.ban`` strings), its segments in a child named ``segments`` (each with ``start``, ``end``, and
the segment's name, ``SEGMENT_NAME``), and its track messages in another child: each message has a
``time``, a ``message`` channel, and its arguments, the first of which (``MESSAGE_NAME``) is the
message's name, such as ``open_door``, and the second (``MESSAGE_ARGUMENT``) its argument.
"""

from dataclasses import dataclass, field

import rtpc
from archives import hashlittle

NODE_NAME = 0x0703FC2D
SEGMENT_NAME = 0xBD68B4E8
MESSAGE_NAME = 0xC1E6FAC4
MESSAGE_ARGUMENT = 0xC769E43B
SEGMENTS = hashlittle("segments")
TIME = hashlittle("time")
START = hashlittle("start")
END = hashlittle("end")


@dataclass
class Message:
    time: float
    name: str
    argument: object


@dataclass
class AnimNode:
    name: str
    clips: list[str]
    segments: dict[str, tuple[float, float]] = field(default_factory=dict)
    messages: list[Message] = field(default_factory=list)


def anim_nodes(asb: bytes) -> dict[str, AnimNode]:
    """Every leaf animation node with a clip, by name. A name that appears more than once keeps
    its first node."""
    out: dict[str, AnimNode] = {}
    stack = [rtpc.parse(asb)]
    while stack:
        node = stack.pop()
        stack.extend(reversed(node.children))
        name = node.properties.get(NODE_NAME)
        if not isinstance(name, str) or name in out:
            continue
        anim = AnimNode(name, [])
        for child in node.children:
            if child.name_hash == SEGMENTS:
                for s in child.children:
                    anim.segments[s.properties.get(SEGMENT_NAME, "")] = (s.properties.get(START, 0.0), s.properties.get(END, 0.0))
                continue
            for v in child.properties.values():
                if isinstance(v, str) and v.endswith(".ban"):
                    anim.clips.append(v)
            for m in child.children:
                if TIME in m.properties and MESSAGE_NAME in m.properties:
                    anim.messages.append(Message(m.properties[TIME], m.properties[MESSAGE_NAME], m.properties.get(MESSAGE_ARGUMENT)))
        if anim.clips:
            anim.messages.sort(key=lambda m: m.time)
            out[name] = anim
    return out
