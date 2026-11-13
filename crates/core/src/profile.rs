//! What carries over between runs: a record of them and the heat unlocked.
//!
//! Winning a run at the highest unlocked heat unlocks the next one. Each point of heat raises
//! every enemy's level.

use crate::content::Content;
use crate::run::{Phase, Run};
use serde::{Deserialize, Serialize};

pub const PROFILE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub version: u32,
    pub runs: u32,
    pub wins: u32,
    /// The furthest act reached, counted from 1.
    pub best_act: u32,
    /// The highest heat a new run may be started with.
    pub heat_unlocked: u32,
    /// The heat new runs start with.
    pub heat: u32,
    pub fights_won: u32,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            version: PROFILE_VERSION,
            runs: 0,
            wins: 0,
            best_act: 0,
            heat_unlocked: 0,
            heat: 0,
            fights_won: 0,
        }
    }
}

impl Profile {
    pub fn from_json(content: &Content, text: &str) -> Result<Profile, String> {
        let mut profile: Profile =
            serde_json::from_str(text).map_err(|e| format!("the profile cannot be read: {e}"))?;
        if profile.version != PROFILE_VERSION {
            return Err(format!(
                "the profile is version {}, this build reads version {PROFILE_VERSION}",
                profile.version
            ));
        }
        profile.heat_unlocked = profile.heat_unlocked.min(content.rules.max_heat);
        profile.heat = profile.heat.min(profile.heat_unlocked);
        Ok(profile)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a profile always serializes")
    }

    /// Records a finished run. Returns true when it unlocked a new heat.
    pub fn record(&mut self, content: &Content, run: &Run) -> bool {
        if !run.is_over() {
            return false;
        }
        self.runs = self.runs.saturating_add(1);
        self.best_act = self.best_act.max(run.act as u32 + 1);
        self.fights_won = self.fights_won.saturating_add(run.stats.fights_won);
        if run.phase != Phase::Victory {
            return false;
        }
        self.wins = self.wins.saturating_add(1);
        if run.heat >= self.heat_unlocked && self.heat_unlocked < content.rules.max_heat {
            self.heat_unlocked += 1;
            return true;
        }
        false
    }

    /// Moves the heat for new runs up or down within what is unlocked.
    pub fn cycle_heat(&mut self) {
        self.heat = if self.heat >= self.heat_unlocked {
            0
        } else {
            self.heat + 1
        };
    }
}
