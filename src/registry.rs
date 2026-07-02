//! The generic content-registry container: definitions keyed by a stable id, inserted
//! as a Bevy [`Resource`] at app build so `Startup` spawners can read it.
//!
//! Each content domain that has its own systems (and so its own stat shape) gets one
//! *typed alias* of this — `ModuleRegistry` (`ModuleKind` → `ModuleDef`) and
//! `TurretRegistry` (`TurretKind` → `TurretDef`) — plus a `Default` impl that builds it
//! from that domain's authored `*_defs()` table. The **definition vs instance** split
//! applies to every alias: the def is the shared static data, the instance is the live
//! entity. Consumers only go through [`Registry::get`], so a registry can later be
//! populated from asset files without touching them.
//!
//! This is *not* the place for every future item type: trade goods / loot / crafting
//! materials should share one item registry as data rows. New `Registry` aliases are
//! only for new simulation domains (shields, mining lasers, ...) whose stats feed
//! their own systems.

use std::collections::HashMap;
use std::hash::Hash;

use bevy::prelude::*;

/// A content registry: every `Def` of one domain, keyed by its stable `Id`.
#[derive(Resource)]
pub struct Registry<Id, Def>
where
    Id: Send + Sync + 'static,
    Def: Send + Sync + 'static,
{
    defs: HashMap<Id, Def>,
}

impl<Id: Copy + Eq + Hash + Send + Sync + 'static, Def: Send + Sync + 'static> Registry<Id, Def> {
    /// Build a registry from `(id, def)` entries (a domain's authored defs table).
    pub fn new(entries: impl IntoIterator<Item = (Id, Def)>) -> Self {
        Self {
            defs: entries.into_iter().collect(),
        }
    }

    /// The definition for `id`. Every id has one (panics otherwise, which would be a
    /// registry-construction bug — the defs table missing a variant).
    pub fn get(&self, id: Id) -> &Def {
        self.defs.get(&id).unwrap_or_else(|| {
            panic!(
                "no definition registered in {}",
                std::any::type_name::<Self>()
            )
        })
    }
}
