//! The director of the Phantom Blade Zero roguelike mod.
//!
//! `pbz-rogue-core` decides what happens in a run; this crate makes it happen in the game, through
//! the bridge that runs inside it (`mod/PBZRoguelike`). See `protocol` for what passes between them.

pub mod director;
pub mod protocol;
pub mod sim;
pub mod transport;
