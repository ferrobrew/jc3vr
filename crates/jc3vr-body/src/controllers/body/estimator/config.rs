//! The learned body estimator's settings, under `controllers.body.estimator`
//! (`docs/mod/body/body-estimator.md`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The estimator's switch and its weights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EstimatorConfig {
    /// Run the estimator under the controller scheme, and let the grounded rig take its estimate.
    pub enabled: bool,
    /// The checkpoint's safetensors file. Unset, it is [`DEFAULT_WEIGHTS_FILE`] next to the
    /// payload DLL.
    pub weights_path: Option<String>,
}

/// The weights' file name next to the payload DLL when no path is set.
pub const DEFAULT_WEIGHTS_FILE: &str = "egoposer.safetensors";

/// The script that converts the authors' checkpoint into the file the estimator loads.
pub(crate) const WEIGHTS_SCRIPT: &str = "scripts/egoposer_weights.sh";

impl EstimatorConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            weights_path: None,
        }
    }

    /// Where the weights are read from: the configured path, or [`DEFAULT_WEIGHTS_FILE`] beside
    /// `payload`, the payload DLL's path. `None` when neither names a file.
    pub fn weights(&self, payload: Option<PathBuf>) -> Option<PathBuf> {
        match self.weights_path.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
            _ => Some(payload?.parent()?.join(DEFAULT_WEIGHTS_FILE)),
        }
    }
}

impl Default for EstimatorConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weights_default_to_beside_the_payload() {
        let payload = Some(PathBuf::from("C:/jc3vr/jc3vr_payload.dll"));
        assert_eq!(
            EstimatorConfig::new().weights(payload.clone()),
            Some(PathBuf::from("C:/jc3vr").join(DEFAULT_WEIGHTS_FILE))
        );
        let set = EstimatorConfig {
            weights_path: Some("D:/models/egoposer.safetensors".into()),
            ..EstimatorConfig::new()
        };
        assert_eq!(
            set.weights(payload),
            Some(PathBuf::from("D:/models/egoposer.safetensors"))
        );
        assert_eq!(EstimatorConfig::new().weights(None), None);
    }
}
