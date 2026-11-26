//! The body lives in the gameplay crate and is re-exported here; its pause menu page ([`menu`])
//! belongs to the UI layer and stays beside it until that cut.

pub(crate) use jc3vr_gameplay::controllers::body::*;

pub(crate) mod menu;
