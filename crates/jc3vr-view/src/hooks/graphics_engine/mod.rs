//! The registries and cells the render crate's graphics-engine hooks share with the layers above,
//! at the paths those hooks give them: the eye and preview captures, the post-draw's registrations,
//! the body's palette edit, and the local player's draw inputs. None of them names a detour.

// The eye and preview captures the post-draw and the post chain fill, which the VR blit, the
// mirror, the recorder, the screenshots, and the Previews tab read.
pub mod capture;
// The body's palette edit the character render blocks apply, and its provider's registration.
pub mod palette_edit;
// The local player's draw inputs, which the character hook publishes and the render blocks read.
pub mod player_draw;
// The registrations the post-draw, the UI redirect, and the flip run for the layers above.
pub mod post_draw;
