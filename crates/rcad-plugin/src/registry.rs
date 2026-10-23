//! Process-wide plugin registry. Documents name plugins by id; regeneration
//! looks them up here. A feature whose plugin is not registered fails with a
//! clear error and keeps its data (it regenerates again once the plugin is
//! installed).
use crate::{Plugin, PluginError};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

type Map = BTreeMap<String, Arc<dyn Plugin>>;

fn map() -> &'static RwLock<Map> {
    static M: OnceLock<RwLock<Map>> = OnceLock::new();
    M.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Register `p` (its manifest is validated first). Registering an id again
/// replaces the earlier plugin (so a re-load picks up a new version).
pub fn register(p: Arc<dyn Plugin>) -> Result<(), PluginError> {
    p.manifest().validate()?;
    let id = p.manifest().id.clone();
    map()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, p);
    Ok(())
}

/// Remove plugin `id`; true if it was registered.
pub fn unregister(id: &str) -> bool {
    map()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(id)
        .is_some()
}

/// The plugin registered as `id`.
pub fn find(id: &str) -> Option<Arc<dyn Plugin>> {
    map()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(id)
        .cloned()
}

/// Every registered plugin, by id.
pub fn plugins() -> Vec<Arc<dyn Plugin>> {
    map()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}
