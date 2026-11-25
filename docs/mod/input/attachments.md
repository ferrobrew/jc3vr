# Body attachment interaction

## Belt magazine availability and placement

The reload owner selects the gun that receives a belt magazine before placing the pouch. The pouch stays on the opposite side of that gun while the free hand becomes occupied. The displayed source remains available while the gun has compatible reserve ammunition or unlimited ammunition. A hand take requires a free tracked hand and a gun that can accept the round. An inserted magazine cannot be borrowed as a replacement part before ejection.

The displayed magazine's configured offset also moves the hand-take and belt-push center. The conversion applies world scale once. The model provider receives the original pouch base and applies the model placement once. Disabling pouch models restores the unshifted interaction center. The target radius and the existing reach margin remain configured separately.

An owned pouch instance keeps its model reference through a temporary source-model gap for the same weapon. Stock loss hides the instance. A different weapon invalidates the previous magazine model. This avoids a source-prop disappearance hiding a still-stocked belt magazine without treating a missing offer as available ammunition.

## Pouch posture

The input provider computes the pouch model's placement from the same body-relative target used by interaction. It stores that placement relative to the hips for the magazine and the sternum for chest items. The render consumer applies the current solved bone and the interpolated character root. It does not interpolate the already-observed local posture again. An unavailable bone retains the existing character-local interpolation fallback.

Native weapon docking retains its existing placement rules. Configured interaction landmarks remain a distinct policy from following displayed native grips. The pouch correction does not redefine inventory socket offsets.

## Runtime checks

The implementation is not game- or headset-accepted. The regression tests cover relative insertion state, retained-round transfer rejection after escape, model-source continuity policy, and current-bone pouch transport. Central formatting, compilation, and test execution remain separate verification steps.

- Repeat a tactical reload and a dry reload with each gun hand. The belt model must remain on its selected side through free-hand occupancy changes. Deplete reserve stock and disable pouch models separately; their visibility and interaction policies must remain distinct.
- Set a nonzero magazine model offset, then compare the displayed source, grab center, and belt-push center while standing, bending, and turning. Restore the setting after the check.
- Capture a magazine, move sideways out of the mouth, push deeper beside the gun, and release. No seat or ammunition transfer may occur. Repeat with a fast axial push and with the gun moved onto a stationary round.
- Check retained rounds, reserve ammunition, chamber readiness, and action completion after extraction and reinsertion. Repeat a weapon switch or feature disable while a pouch round is held, and inspect the inserted-part visibility when the weapon returns.
- Repeat posture checks on a translating and turning standing platform and in a seated moving vehicle. Root sampling and hand late-update transport are shared with the moving-platform correction and require integrated verification.
