//! When the controller input needs a game restart to match the settings, and what the player is
//! told about it.
//!
//! The OpenXR instance and session outlive an inject cycle, a session accepts action sets once, and
//! the suggested bindings are fixed when the sets are attached (see the module docs of
//! [`crate::vr::input`]). So two things can only take effect after the game restarts:
//!
//! - a payload whose action layout, suggested bindings, or dominant hand differ from those of the
//!   payload that created the persisted action sets gets no controller input at all
//!   ([`RestartReason::LayoutChanged`], [`RestartReason::StashedForOtherHand`],
//!   [`RestartReason::SessionAttached`]);
//! - a change of `controllers.dominant_hand` while the input is attached keeps the bindings that
//!   were suggested for the other hand ([`RestartReason::HandChanged`]).
//!
//! [`reason`] decides from the published [`InputStatus`] and the configured hand. It is pure, and
//! its inputs are leaf slots, so any thread may ask; the notice panel, the debug UI, and the pause
//! menu all show it.

use std::fmt;

use crate::{config::Config, controllers::frame::Hand, vr::input::InputStatus};

/// Why the game must restart for the controller input to match the settings.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RestartReason {
    /// The persisted OpenXR instance holds action sets that a payload with another action layout
    /// or other suggested bindings created, so this payload has no controller input.
    LayoutChanged,
    /// The same, where the persisted sets were suggested for the other dominant hand.
    StashedForOtherHand,
    /// The session already has action sets attached, so this payload's could not be.
    SessionAttached,
    /// The dominant hand changed since the bindings were suggested: the controllers work, with the
    /// other hand's button layout.
    HandChanged {
        /// The hand the bindings were suggested for.
        suggested: Hand,
    },
}

impl RestartReason {
    /// Whether the controller input is unavailable until the restart, rather than laid out for the
    /// other hand.
    pub const fn blocks_input(self) -> bool {
        !matches!(self, RestartReason::HandChanged { .. })
    }

    /// The one-line notice.
    pub const fn headline(self) -> &'static str {
        match self {
            RestartReason::LayoutChanged
            | RestartReason::StashedForOtherHand
            | RestartReason::SessionAttached => "Restart the game to use the motion controllers",
            RestartReason::HandChanged { .. } => "Restart the game to apply the dominant hand",
        }
    }

    /// What happened, and what holds until the restart.
    pub fn detail(self) -> String {
        match self {
            RestartReason::LayoutChanged => "The controller bindings changed since the game \
                                             started, and OpenXR keeps the first bindings until \
                                             the game restarts. The controllers do nothing until \
                                             then."
                .to_owned(),
            RestartReason::StashedForOtherHand => "The dominant hand changed since the game \
                                                   started, and OpenXR keeps the first bindings \
                                                   until the game restarts. The controllers do \
                                                   nothing until then."
                .to_owned(),
            RestartReason::SessionAttached => "The VR session already holds another set of \
                                               controller bindings, and accepts no other until \
                                               the game restarts. The controllers do nothing \
                                               until then."
                .to_owned(),
            RestartReason::HandChanged { suggested } => format!(
                "The buttons and the hands' roles stay laid out for a {}-handed player until the \
                 game restarts. Change the dominant hand back to keep that layout.",
                hand_word(suggested)
            ),
        }
    }
}

impl fmt::Display for RestartReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.headline(), self.detail())
    }
}

/// Why the game must restart for the controller input to match a configured dominant hand of
/// `configured`, or `None` when it already does.
pub fn reason(status: &InputStatus, configured: Hand) -> Option<RestartReason> {
    if let Some(failure) = status.restart {
        return Some(failure);
    }
    match status.suggested_for {
        Some(suggested) if status.attached && suggested != configured => {
            Some(RestartReason::HandChanged { suggested })
        }
        _ => None,
    }
}

/// [`reason`] for the published input status and the configured dominant hand, or `None` while
/// the controllers are disabled, when nothing reads them. Takes the input status slot and then the
/// config lock, one after the other, never nested.
pub fn current() -> Option<RestartReason> {
    let status = crate::vr::input::status();
    let (enabled, configured) =
        Config::lock_query(|c| (c.controllers.enabled, c.controllers.dominant_hand));
    reason(&status, configured).filter(|_| enabled)
}

/// The bring-up failure that a restart fixes, carried through the error chain so that
/// [`crate::vr::input::InputState::acquire`] can record it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct NeedsRestart(pub(super) RestartReason);

impl fmt::Display for NeedsRestart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.0 {
            RestartReason::LayoutChanged => {
                "a payload with a different action layout or suggested bindings created the \
                 action sets on the persisted OpenXR instance"
            }
            RestartReason::StashedForOtherHand => {
                "the action sets on the persisted OpenXR instance were suggested for the other \
                 dominant hand"
            }
            RestartReason::SessionAttached => {
                "the session already has action sets attached, and accepts them once"
            }
            RestartReason::HandChanged { .. } => {
                "the bindings were suggested for the other dominant hand"
            }
        };
        write!(
            f,
            "input: controller input is unavailable: {what}; restart the game"
        )
    }
}

impl std::error::Error for NeedsRestart {}

/// The restart reason an error chain carries, if any.
pub(super) fn of_error(error: &anyhow::Error) -> Option<RestartReason> {
    // anyhow's downcast looks through the context layers too.
    error.downcast_ref::<NeedsRestart>().map(|needs| needs.0)
}

const fn hand_word(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;

    use super::*;

    fn attached(suggested: Hand) -> InputStatus {
        InputStatus {
            attached: true,
            suggested_for: Some(suggested),
            ..InputStatus::default()
        }
    }

    #[test]
    fn a_matching_hand_needs_no_restart() {
        for hand in Hand::BOTH {
            assert_eq!(reason(&attached(hand), hand), None);
        }
        assert_eq!(reason(&InputStatus::default(), Hand::Left), None);
    }

    #[test]
    fn a_hand_changed_while_attached_needs_a_restart_until_it_is_changed_back() {
        let status = attached(Hand::Right);
        let changed = reason(&status, Hand::Left);
        assert_eq!(
            changed,
            Some(RestartReason::HandChanged {
                suggested: Hand::Right
            })
        );
        assert!(!changed.unwrap().blocks_input());
        assert_eq!(reason(&status, Hand::Right), None);
    }

    #[test]
    fn a_detached_input_is_not_reported_as_a_hand_change() {
        let status = InputStatus {
            attached: false,
            ..attached(Hand::Right)
        };
        assert_eq!(reason(&status, Hand::Left), None);
    }

    #[test]
    fn a_bring_up_failure_wins_whatever_the_hand() {
        for failure in [
            RestartReason::LayoutChanged,
            RestartReason::StashedForOtherHand,
            RestartReason::SessionAttached,
        ] {
            let status = InputStatus {
                restart: Some(failure),
                ..InputStatus::default()
            };
            for hand in Hand::BOTH {
                assert_eq!(reason(&status, hand), Some(failure));
            }
            assert!(failure.blocks_input());
        }
    }

    #[test]
    fn the_reason_survives_the_error_chain() {
        let error = anyhow::Error::new(NeedsRestart(RestartReason::LayoutChanged));
        assert_eq!(of_error(&error), Some(RestartReason::LayoutChanged));
        let wrapped = Err::<(), _>(anyhow::anyhow!("attaching the action sets"))
            .context(NeedsRestart(RestartReason::SessionAttached))
            .context("bring-up")
            .unwrap_err();
        assert_eq!(of_error(&wrapped), Some(RestartReason::SessionAttached));
        assert_eq!(of_error(&anyhow::anyhow!("unrelated")), None);
        assert!(error.to_string().ends_with("restart the game"));
    }

    #[test]
    fn every_notice_says_to_restart() {
        for reason in [
            RestartReason::LayoutChanged,
            RestartReason::StashedForOtherHand,
            RestartReason::SessionAttached,
            RestartReason::HandChanged {
                suggested: Hand::Left,
            },
        ] {
            assert!(reason.headline().starts_with("Restart the game"));
            assert!(!reason.detail().is_empty());
        }
    }
}
