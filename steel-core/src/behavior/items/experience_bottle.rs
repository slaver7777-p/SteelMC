//! Egg item behavior (`ExpBottleItem`).
//!
//! Throwing an egg spawns a [`ExpBottleItem`] from the player's eye, shot
//! along their look direction, and consumes one egg unless the player
//! has infinite materials. Mirrors vanilla `EggItem.use`.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};

use super::throw_projectile::{ThrowParams, throw_item_projectile};
use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ThrownExperienceBottleEntity;
use crate::entity::next_entity_id;

/// Sound, shoot power and spread used when throwing an egg.
const THROW: ThrowParams = ThrowParams {
    sound: &sound_events::ENTITY_EXPERIENCE_BOTTLE_THROW,
    sound_source: SoundSource::Players,
    sound_volume: 0.5,
    y_offset: -20.0,
    power: 0.7,
    uncertainty: 1.0,
};

/// Behavior for the egg item.
#[item_behavior(class = "ExperienceBottleItem")]
pub struct ExperienceBottleItem;

impl ItemBehavior for ExperienceBottleItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let world = context.world;
        throw_item_projectile(context, &THROW, |spawn_pos| {
            ThrownExperienceBottleEntity::new(
                &vanilla_entities::EXPERIENCE_BOTTLE,
                next_entity_id(),
                spawn_pos,
                Arc::downgrade(world),
            )
        });
        InteractionResult::Success
    }
}