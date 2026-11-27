# The free hand on the line

While the grapple lock holds the line ([grapple-lock.md](grapple-lock.md)) or the snatch's line swings its target ([grapple-snatch-swing.md](grapple-snatch-swing.md)), the free hand can take hold of the line ahead of the grapple hand (`grapple::line_grab` in `jc3vr-grapple`). On the lock's line, pulling it back takes line in. On the snatch's line, the hand holding it becomes the line's anchor, so its pull draws the target in and its swing pulls the target the new way.

## The grab target

`line_grab::targets` places one target, `GrabVolume::GrappleLine`, for the hand other than the grapple hand when that hand is free and empty and its palm is near the line. The line runs from the grapple hand's wrist (offset by the aim tuning's `ray.grapple_origin_m`) to the lock's hook point or to the hook's point on the snatched target. The `line` section of the grapple's tuning (`grapple.toml`, [tuning.md](../tuning.md)) sets how near the line the palm must be (`take_m`), how far ahead of the wrist it may take hold (`min_ahead_m` to `max_ahead_m`), how far short of the far end it must stay (`far_margin_m`), and where the palm sits on the wrist. The near limit leaves the wrist to the snatch's brace. The target is ranked like any other (the nearest grab point wins) and marked with a ring on the line at the point nearest the palm. A squeeze there latches the grip on the line (`GripLatch::Grab(GrappleLine)`), and the line stays held until the grip opens, wherever the hand then goes.

While the snatch's line is out, the free hand's grip is held for the catch and takes nothing else ([grapple-snatch.md](grapple-snatch.md#the-interaction)). The line is the one exception: the reserved grip still takes it.

## On the lock's line

While the free hand holds the line, the lock measures that hand's pull along the line's direction, in the cockpit frame, the way it measures the grapple hand's own pull (`lock::model::FreeHand`, a `TakeIn` of its own). Pulling back takes line in by the distance pulled, no faster than `lock.reel_in_mps`. Reaching out pays nothing out. Each new hold measures from where it took hold, so letting go, reaching out, and taking hold again keeps taking line in, hand over hand. Both hands' pulls add, and the line still stops at `lock.min_length_m`. The free hand's swing across the line does nothing to the lock; only the grapple hand pumps the swing.

## On the snatch's line

The snatch's held line is a tension-only rope from an anchor to the target ([grapple-snatch-swing.md](grapple-snatch-swing.md)). While the free hand holds it, the anchor is that hand's palm (`snatch::brace`, `Brace::step_line`):

- Taking hold shortens the line by how far ahead of the grapple hand the hand took it, so the line from the new anchor is as long as it was.
- While held, the hand's motion moves the anchor, and the same rope step that swings the target from the grapple hand acts on it. Pulling the hand back draws the target in. Swinging the hand across the line pulls the target toward the hand's new place. A flick mostly yanks the target along the line, with a share across it; sustained swings build up its speed across the line, as whirling the line from the grapple hand does. Nothing is launched outside the rope's pull.
- Letting go hands the line back to the grapple hand, at the held length plus how far ahead of the grapple hand the hand let go. A hand pulled back past the grapple hand has taken in the difference. A hand pushed out along the line gives back at most what it took, so nothing is paid out (`line_grab::handed_back_m`). The line never goes below its least held length, and the snatch's own reel keeps taking it in toward the swing length but never pays it out, so a line pulled shorter than the swing length stays short.

Neither change of anchor is read as the hand moving (`Sling::regrip`). With two hands on the line it is as strong as a braced line, `brace_factor` times one hand's. The brace at the wrist and the hold ahead of it never run together: the brace takes a grip closed on nothing, and the hold takes a grip latched on the line. The line also lets go when the free hand loses tracking, giving back what the hold took.

The line is held at the palm, not at a point that slides along the line in the fist, so a hand drawn far off the line still anchors it. The line from the grapple hand is slack while the free hand holds it. The hold also takes the line of a weapon or a light object swung on B; if B is released while the free hand holds that line, the swing resumes into the pull to the hand, and that hand's grip, still latched on the line, does not catch until it opens and closes again.

## Feedback

The free hand feels the line's tension while it holds the line (`Sustain::GrappleTension`, as the grapple hand does). The snatch's in-world line is drawn from the anchor, so it runs from the free hand while that hand holds it. The log has `grapple lock: the free hand took hold of the line` and `let go of the line` with the held length, and the same lines under `snatch:` with the line's length from the new anchor.
