# Shutdown preparation

The shutdown request path in `payload/src/lifecycle.rs` separates reversible preparation from irreversible teardown. Repeated requests coalesce. Each attempt has a request generation, an attempt generation, and a participant-registration revision. Deferred or failed preparation leaves the shutdown flag unset, preserves ordinary cleanup registrations, and returns to normal frame work. A retry interval bounds preparation work; it does not prove native callback quiescence.

Participants register before enabling work that requires shutdown preparation. Preparation runs outside the lifecycle mutex. A ready participant returns a guard that retains the required exclusion barrier and validates immediately before commit. Dropping an uncommitted guard rolls preparation back. Stale generations or a changed registry cannot authorize shutdown. After every guard validates, infallible finalizers run while the barriers remain held. Only then does the lifecycle set the shutdown flag and permit ordinary reverse-order cleanup, worker shutdown, hook removal, and module exit.

The reserved human-collider participant does not implement shape migration. A future implementation must restore all affected state transactionally and prove clearance, identity, and reader exclusion before returning a ready guard. An unavailable or blocked restoration must keep gameplay and its required hooks active. No global collider mutation is enabled by this lifecycle change.

## Inventory spawn callbacks

Inventory restore and second-pistol requests hand the engine a payload callback, and nothing the engine reports proves that a queued callback has retired. The participant in `controllers/weapons/inventory/spawn_safety.rs` latches the first such exposure for the rest of the process, but the exposure does not veto an unload: an unload that can never run cannot show whether the risk is real. Preparation and the post-unpatch check log a warning naming the risk (a callback the engine still holds would crash the game once the image unmaps) and continue. They defer only while a submission or callback is executing payload code at that moment, which is transient, and preparation closes admission so no new request starts.

## Magazine executor restriction

`hooks::activate_magazine_executor` marks the magazine binder installed. Shutdown preparation for the magazine executor runs only on the game thread. It cancels a magazine extraction in progress, then defers (`DeferReason::MagazineBusy`) while the reload executor has not drained or a native magazine callback entry is active.

The hook library's disable operation does not establish callback completion. A snapshot of suspended threads also does not prove that a callback cannot resume in removed code. A future executor adapter must retain a proven barrier through removal or provide an equivalent lifetime guarantee. This restriction does not certify the pre-existing removal behavior of every other hook.

## Fatal failures

Initialization failures and caught update panics enter a fatal lifecycle state. The module remains loaded, ordinary cleanups remain registered, and subsequent mod updates stop. This is containment, not a claim of safe continuing gameplay or safe ejection. A game restart is required. A cleanup panic or hook-removal failure after commit also retains the module rather than ejecting it.

## Verification

Isolated lifecycle tests cover coalesced requests, bounded retries, normal-frame continuation on deferral, partial preparation rollback, stale tokens, participant-generation changes, registration exclusivity, cleanup ordering, and one-shot commit. Hook-preparation tests cover busy drains and rejection of an installed callback without an entry-exclusion contract. These tests do not prove native callback exclusion, collider clearance, or live ejection and reinjection safety.
