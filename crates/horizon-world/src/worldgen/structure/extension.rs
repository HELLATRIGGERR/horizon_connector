//! Native custom structure handlers. Registrations use the ordinary structure
//! registry, placement step, references, seed stream and saved-start machinery.
use super::{
    legacy::{LegacyPiece, PieceRandom},
    start::StartContext,
};
use horizon_core::position::ChunkPos;
use serde_json::Value;
use std::{collections::BTreeMap, fmt::Debug, sync::Arc};

pub type Extensions = BTreeMap<String, Arc<dyn StructureExtension>>;
pub type GeneratedPieces = ([i32; 3], Vec<Box<dyn LegacyPiece>>);
pub trait StructureExtension: Debug + Send + Sync {
    /// Return the generation stub and its pieces. The host validates the
    /// stub's biome with the structure's active biome set, as for vanilla.
    fn generate(
        &self,
        context: &mut dyn StartContext,
        random: &mut dyn PieceRandom,
        chunk: ChunkPos,
    ) -> Option<GeneratedPieces>;
    /// Restore one saved piece of this structure, without regenerating settings.
    fn read_piece(&self, piece: &Value) -> Result<Box<dyn LegacyPiece>, String>;
}

/// World/dimension-scoped persisted structure starts, queried before generation.
/// None denotes no saved STRUCTURE_STARTS yet; Some(empty) is authoritative.
pub trait SavedStartSource: Debug + Send + Sync {
    fn read_starts(&self, chunk: ChunkPos) -> Result<Option<crate::nbt::Compound>, String>;
}
