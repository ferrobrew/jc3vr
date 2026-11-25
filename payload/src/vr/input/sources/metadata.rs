//! Runtime-owned, bounded names for opaque source paths. Enumeration remains uncached.

use openxr as xr;

use crate::vr::input::sources::{BoundSource, QueryFailure, identify_sources};

const CAPACITY: usize = 256;

#[derive(Default)]
pub(super) struct MetadataCache {
    entries: Vec<(xr::Path, BoundSource)>,
}

impl MetadataCache {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(super) fn resolve(
        &mut self,
        paths: Vec<xr::Path>,
        mut identify: impl FnMut(xr::Path) -> Result<String, String>,
        mut localize: impl FnMut(xr::Path) -> Result<String, String>,
    ) -> Result<Vec<BoundSource>, QueryFailure> {
        let mut result = Vec::with_capacity(paths.len());
        for path in paths {
            if let Some((_, source)) = self.entries.iter().find(|(key, _)| *key == path) {
                result.push(source.clone());
                continue;
            }
            let mut sources = identify_sources(vec![path], &mut identify, &mut localize)?;
            let source = sources.pop().expect("sources: one identified path");
            if self.entries.len() == CAPACITY {
                self.entries.remove(0);
            }
            self.entries.push((path, source.clone()));
            result.push(source);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn steady_paths_do_not_localize_and_new_binding_only_names_new_path() {
        let mut cache = MetadataCache::default();
        let identified = Cell::new(0);
        let localized = Cell::new(0);
        for paths in [vec![1, 2], vec![1, 2], vec![2, 3], vec![2, 3]] {
            cache
                .resolve(
                    paths.into_iter().map(xr::Path::from_raw).collect(),
                    |path| {
                        identified.set(identified.get() + 1);
                        Ok(format!("/user/hand/left/input/{path:?}"))
                    },
                    |_| {
                        localized.set(localized.get() + 1);
                        Ok("Source".to_owned())
                    },
                )
                .unwrap();
        }
        assert_eq!((identified.get(), localized.get()), (3, 3));
    }

    #[test]
    fn failed_identification_is_not_cached_and_recovers_on_the_same_path() {
        let mut cache = MetadataCache::default();
        let path = xr::Path::from_raw(1);
        assert!(
            cache
                .resolve(
                    vec![path],
                    |_| Err("temporarily unavailable".to_owned()),
                    |_| unreachable!()
                )
                .is_err()
        );
        assert!(cache.entries.is_empty());
        let sources = cache
            .resolve(
                vec![path],
                |_| Ok("/user/hand/left/input/a/click".to_owned()),
                |_| Ok("A".to_owned()),
            )
            .unwrap();
        assert_eq!(sources[0].hand, Some(crate::controllers::frame::Hand::Left));
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn eviction_and_runtime_reset_recover_without_reusing_stale_identity() {
        let mut cache = MetadataCache::default();
        let calls = Cell::new(0);
        let resolve = |cache: &mut MetadataCache, key| {
            cache
                .resolve(
                    vec![xr::Path::from_raw(key)],
                    |_| Ok("/user/hand/right/input/a/click".to_owned()),
                    |_| {
                        calls.set(calls.get() + 1);
                        Ok(format!("Name {}", calls.get()))
                    },
                )
                .unwrap()
        };
        for key in 1..=CAPACITY as u64 + 1 {
            resolve(&mut cache, key);
        }
        assert_eq!(cache.entries.len(), CAPACITY);
        let source = resolve(&mut cache, 1);
        assert_eq!(calls.get(), CAPACITY + 2);
        cache.clear();
        let replacement = resolve(&mut cache, 1);
        assert_ne!(source, replacement);
        assert_eq!(cache.entries.len(), 1);
    }
}
