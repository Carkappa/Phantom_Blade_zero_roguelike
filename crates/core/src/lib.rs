//! The run logic of the Phantom Blade Zero roguelike mod.
//!
//! Nothing here knows about the game process. The director crate turns what this crate decides into
//! commands for the in-game bridge, so all of it can be tested without the game.

pub mod content;
pub mod map;
pub mod profile;
pub mod rng;
pub mod run;
pub mod view;

/// The content shipped with the mod, compiled in so the director works with no files beside it.
pub mod embedded {
    pub const ROSTER: &str = include_str!("../../../data/roster.json");
    pub const ACTS: &str = include_str!("../../../data/acts.json");
    pub const BLESSINGS: &str = include_str!("../../../data/blessings.json");
    pub const EVENTS: &str = include_str!("../../../data/events.json");

    pub fn content() -> Result<crate::content::Content, String> {
        crate::content::Content::from_json(ROSTER, ACTS, BLESSINGS, EVENTS)
    }
}
