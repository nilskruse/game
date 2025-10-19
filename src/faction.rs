use bevy::prelude::*;

#[derive(PartialEq, Clone, Debug)]
pub enum Faction {
    Player,
    Enemy,
}

#[derive(Component, Deref, DerefMut, PartialEq, Clone, Debug)]
pub struct InFaction(pub Faction);
