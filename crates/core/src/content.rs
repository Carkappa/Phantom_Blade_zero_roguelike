//! What a run is made of: enemies, acts, blessings, events and the numbers that tie them together.
//!
//! All of it is data (`data/*.json`). `Content::from_json` refuses data that could break a run, so a
//! hand-edited file fails loudly at start-up and not in the middle of a fight.

use crate::map::MapParams;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pool {
    Mob,
    Elite,
    Lord,
    /// Never summoned. `Unit::note` says why.
    Excluded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    /// The game's blueprint name, e.g. `BP_Monster_MoTian`. Sent to the bridge as is.
    pub id: String,
    /// The codex name.
    pub name: String,
    /// A short Latin-script label made from the id.
    #[serde(default)]
    pub label: String,
    pub chapter: u32,
    pub boss: bool,
    pub pool: Pool,
    /// True when another mod recorded real fights with this unit summoned.
    #[serde(default)]
    pub verified: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl Unit {
    pub fn display(&self) -> String {
        if self.label.is_empty() || self.label == self.name {
            self.name.clone()
        } else {
            format!("{} - {}", self.label, self.name)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActDef {
    pub name: String,
    /// Game chapters whose enemies appear in this act.
    pub chapters: Vec<u32>,
    pub floors: usize,
    /// Enemy level on the bottom row and on the top row.
    pub level_start: u32,
    pub level_end: u32,
    pub elite_level_bonus: u32,
    pub lord_level_bonus: u32,
    pub pack_min: u32,
    pub pack_max: u32,
    /// Out of a hundred plain fights, how many are a duel with a boss instead of a pack.
    pub duel_chance: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rarity {
    Common,
    Rare,
    Legendary,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Rarity::Common => "Common",
            Rarity::Rare => "Rare",
            Rarity::Legendary => "Legendary",
        }
    }
}

/// What a blessing does. Percentages are whole numbers; each stack of a blessing adds its effects again.
///
/// Every effect is something the mod can do without writing anything lasting into the player's
/// character: it changes what is summoned, heals, or changes the run's own bookkeeping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// Heal this share of maximum health after each fight won.
    HealAfterFight {
        pct: u32,
    },
    /// Heal this share of maximum health when a fight begins.
    HealOnFightStart {
        pct: u32,
    },
    /// Shrines heal this much more.
    RestHeal {
        pct: u32,
    },
    /// Enemies start with this much less health (zero or below).
    EnemyHealth {
        pct: i32,
    },
    EliteHealth {
        pct: i32,
    },
    LordHealth {
        pct: i32,
    },
    /// Enemy level change. Below zero is easier.
    EnemyLevel {
        delta: i32,
    },
    GoldGain {
        pct: i32,
    },
    ShopDiscount {
        pct: u32,
    },
    /// More blessings to choose from after a fight.
    ExtraChoice {
        n: u32,
    },
    /// Rare blessings turn up more often.
    RareChance {
        pct: u32,
    },
    /// Packs are larger or smaller.
    PackSize {
        delta: i32,
    },
    /// On pickup: a death is forgiven this many times.
    Revive {
        charges: u32,
    },
    /// On pickup.
    GoldNow {
        amount: u32,
    },
    /// On pickup.
    HealNow {
        pct: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blessing {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub rarity: Rarity,
    #[serde(default = "one")]
    pub max_stacks: u32,
    pub effects: Vec<Effect>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    /// Gain gold, or lose it when below zero (never below nothing).
    Gold {
        amount: i32,
    },
    Heal {
        pct: u32,
    },
    /// Lose this share of maximum health. Never kills.
    Hurt {
        pct: u32,
    },
    /// A random blessing of this rarity.
    Blessing {
        rarity: Rarity,
    },
    Revive {
        charges: u32,
    },
    /// A fight follows.
    Ambush,
    /// `pct` times out of a hundred `win` happens, otherwise `lose`.
    Gamble {
        pct: u32,
        win: Vec<Outcome>,
        win_text: String,
        lose: Vec<Outcome>,
        lose_text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventOption {
    pub label: String,
    #[serde(default)]
    pub cost_gold: u32,
    /// Shown once the option is taken (a gamble shows its own text instead).
    pub result: String,
    pub outcomes: Vec<Outcome>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventDef {
    pub id: String,
    pub title: String,
    pub text: String,
    pub options: Vec<EventOption>,
}

/// The numbers of a run. Every field has a default, so `rules` may list only what it changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    pub boon_choices: u32,
    pub reward_choices: u32,
    pub gold_fight: [u32; 2],
    pub gold_elite: [u32; 2],
    pub gold_lord: [u32; 2],
    pub gold_treasure: [u32; 2],
    pub price_common: u32,
    pub price_rare: u32,
    pub price_legendary: u32,
    pub shop_stock: u32,
    pub shop_heal_price: u32,
    pub shop_heal_pct: u32,
    pub rest_heal_pct: u32,
    pub rest_gold: u32,
    pub act_clear_heal_pct: u32,
    pub weight_common: u32,
    pub weight_rare: u32,
    pub weight_legendary: u32,
    /// Enemy levels added per point of heat.
    pub heat_level_step: u32,
    pub max_heat: u32,
    pub max_level: u32,
    /// Blessings never bring an enemy's starting health below this.
    pub min_enemy_health_pct: u32,
    /// How many times a fight whose enemies cannot be summoned is replaced before it is skipped.
    pub max_rerolls: u32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            boon_choices: 3,
            reward_choices: 3,
            gold_fight: [12, 20],
            gold_elite: [28, 38],
            gold_lord: [60, 80],
            gold_treasure: [20, 30],
            price_common: 45,
            price_rare: 80,
            price_legendary: 140,
            shop_stock: 4,
            shop_heal_price: 35,
            shop_heal_pct: 30,
            rest_heal_pct: 35,
            rest_gold: 25,
            act_clear_heal_pct: 50,
            weight_common: 70,
            weight_rare: 25,
            weight_legendary: 5,
            heat_level_step: 2,
            max_heat: 10,
            max_level: 60,
            min_enemy_health_pct: 50,
            max_rerolls: 3,
        }
    }
}

/// The sum of every owned blessing's lasting effects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub heal_after_fight: u32,
    pub heal_on_fight_start: u32,
    pub rest_heal: u32,
    pub enemy_health: i32,
    pub elite_health: i32,
    pub lord_health: i32,
    pub enemy_level: i32,
    pub gold_gain: i32,
    pub shop_discount: u32,
    pub extra_choice: u32,
    pub rare_chance: u32,
    pub pack_size: i32,
}

impl Mods {
    pub fn add(&mut self, effect: &Effect, stacks: u32) {
        let s = stacks;
        let si = stacks as i32;
        match effect {
            Effect::HealAfterFight { pct } => self.heal_after_fight += pct * s,
            Effect::HealOnFightStart { pct } => self.heal_on_fight_start += pct * s,
            Effect::RestHeal { pct } => self.rest_heal += pct * s,
            Effect::EnemyHealth { pct } => self.enemy_health += pct * si,
            Effect::EliteHealth { pct } => self.elite_health += pct * si,
            Effect::LordHealth { pct } => self.lord_health += pct * si,
            Effect::EnemyLevel { delta } => self.enemy_level += delta * si,
            Effect::GoldGain { pct } => self.gold_gain += pct * si,
            Effect::ShopDiscount { pct } => self.shop_discount += pct * s,
            Effect::ExtraChoice { n } => self.extra_choice += n * s,
            Effect::RareChance { pct } => self.rare_chance += pct * s,
            Effect::PackSize { delta } => self.pack_size += delta * si,
            Effect::Revive { .. } | Effect::GoldNow { .. } | Effect::HealNow { .. } => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Content {
    pub units: Vec<Unit>,
    pub acts: Vec<ActDef>,
    pub blessings: Vec<Blessing>,
    pub events: Vec<EventDef>,
    pub rules: Rules,
}

#[derive(Deserialize)]
struct RosterFile {
    units: Vec<Unit>,
}

#[derive(Deserialize)]
struct ActsFile {
    #[serde(default)]
    rules: Rules,
    acts: Vec<ActDef>,
}

#[derive(Deserialize)]
struct BlessingsFile {
    blessings: Vec<Blessing>,
}

#[derive(Deserialize)]
struct EventsFile {
    events: Vec<EventDef>,
}

fn is_ident(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 96
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn range_ok(pair: [u32; 2]) -> bool {
    pair[0] <= pair[1] && pair[1] <= 100_000
}

impl Content {
    pub fn from_json(
        roster: &str,
        acts: &str,
        blessings: &str,
        events: &str,
    ) -> Result<Content, String> {
        let roster: RosterFile =
            serde_json::from_str(roster).map_err(|e| format!("roster.json: {e}"))?;
        let acts: ActsFile = serde_json::from_str(acts).map_err(|e| format!("acts.json: {e}"))?;
        let blessings: BlessingsFile =
            serde_json::from_str(blessings).map_err(|e| format!("blessings.json: {e}"))?;
        let events: EventsFile =
            serde_json::from_str(events).map_err(|e| format!("events.json: {e}"))?;
        let content = Content {
            units: roster.units,
            acts: acts.acts,
            blessings: blessings.blessings,
            events: events.events,
            rules: acts.rules,
        };
        content.validate()?;
        Ok(content)
    }

    pub fn unit(&self, id: &str) -> Option<&Unit> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn blessing(&self, id: &str) -> Option<&Blessing> {
        self.blessings.iter().find(|b| b.id == id)
    }

    pub fn event(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }

    /// The units of one pool that belong to an act's chapters.
    pub fn act_units(&self, act: usize, pool: Pool) -> Vec<&Unit> {
        let Some(def) = self.acts.get(act) else {
            return Vec::new();
        };
        self.units
            .iter()
            .filter(|u| u.pool == pool && def.chapters.contains(&u.chapter))
            .collect()
    }

    pub fn price(&self, rarity: Rarity) -> u32 {
        match rarity {
            Rarity::Common => self.rules.price_common,
            Rarity::Rare => self.rules.price_rare,
            Rarity::Legendary => self.rules.price_legendary,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let r = &self.rules;
        if r.boon_choices == 0
            || r.boon_choices > 5
            || r.reward_choices == 0
            || r.reward_choices > 5
        {
            return Err("rules: boon_choices and reward_choices must be 1 to 5".into());
        }
        if !(range_ok(r.gold_fight)
            && range_ok(r.gold_elite)
            && range_ok(r.gold_lord)
            && range_ok(r.gold_treasure))
        {
            return Err("rules: a gold range must be [low, high] with low <= high".into());
        }
        if r.shop_stock == 0 || r.shop_stock > 6 {
            return Err("rules: shop_stock must be 1 to 6".into());
        }
        for (name, pct) in [
            ("shop_heal_pct", r.shop_heal_pct),
            ("rest_heal_pct", r.rest_heal_pct),
            ("act_clear_heal_pct", r.act_clear_heal_pct),
        ] {
            if pct > 100 {
                return Err(format!("rules: {name} must be 0 to 100"));
            }
        }
        if r.weight_common == 0 || r.weight_common + r.weight_rare + r.weight_legendary > 10_000 {
            return Err(
                "rules: rarity weights must be sensible and weight_common above zero".into(),
            );
        }
        if r.max_level == 0 || r.max_level > 200 {
            return Err("rules: max_level must be 1 to 200".into());
        }
        if r.max_heat > 50 || r.heat_level_step > 20 {
            return Err("rules: max_heat or heat_level_step is too large".into());
        }
        if !(10..=100).contains(&r.min_enemy_health_pct) {
            return Err("rules: min_enemy_health_pct must be 10 to 100".into());
        }
        if r.max_rerolls > 10 {
            return Err("rules: max_rerolls must be 0 to 10".into());
        }
        if r.price_common > 100_000
            || r.price_rare > 100_000
            || r.price_legendary > 100_000
            || r.shop_heal_price > 100_000
            || r.rest_gold > 100_000
        {
            return Err("rules: a price is too large".into());
        }

        let mut ids = HashSet::new();
        for unit in &self.units {
            if !is_ident(&unit.id) {
                return Err(format!(
                    "roster: unit id {:?} must be letters, digits and underscores",
                    unit.id
                ));
            }
            if !ids.insert(unit.id.as_str()) {
                return Err(format!("roster: unit {} is listed twice", unit.id));
            }
            if unit.name.trim().is_empty() {
                return Err(format!("roster: unit {} has no name", unit.id));
            }
            if unit.pool == Pool::Excluded && unit.note.trim().is_empty() {
                return Err(format!(
                    "roster: excluded unit {} needs a note saying why",
                    unit.id
                ));
            }
            if unit.pool == Pool::Lord && !unit.boss {
                return Err(format!("roster: lord {} is not a boss", unit.id));
            }
        }

        if self.acts.is_empty() || self.acts.len() > 9 {
            return Err("acts: a run needs 1 to 9 acts".into());
        }
        for (i, act) in self.acts.iter().enumerate() {
            let at = format!("acts: act {} ({})", i + 1, act.name);
            if act.name.trim().is_empty() || act.chapters.is_empty() {
                return Err(format!("{at} needs a name and chapters"));
            }
            MapParams::standard(act.floors)
                .check()
                .map_err(|e| format!("{at}: {e}"))?;
            if act.level_start == 0
                || act.level_start > act.level_end
                || act.level_end > r.max_level
            {
                return Err(format!(
                    "{at}: levels must satisfy 1 <= level_start <= level_end <= max_level"
                ));
            }
            if act.elite_level_bonus > 30 || act.lord_level_bonus > 30 {
                return Err(format!("{at}: a level bonus above 30 is a mistake"));
            }
            if act.pack_min == 0 || act.pack_min > act.pack_max || act.pack_max > 6 {
                return Err(format!(
                    "{at}: packs must satisfy 1 <= pack_min <= pack_max <= 6"
                ));
            }
            if act.duel_chance > 100 {
                return Err(format!("{at}: duel_chance must be 0 to 100"));
            }
            if self.act_units(i, Pool::Mob).is_empty() {
                return Err(format!("{at} has no ordinary enemies"));
            }
            if self.act_units(i, Pool::Elite).len() < 3 {
                return Err(format!("{at} needs at least three elites"));
            }
            if self.act_units(i, Pool::Lord).is_empty() {
                return Err(format!("{at} has no lord"));
            }
        }

        let mut ids = HashSet::new();
        for b in &self.blessings {
            if !is_ident(&b.id) {
                return Err(format!(
                    "blessings: id {:?} must be letters, digits and underscores",
                    b.id
                ));
            }
            if !ids.insert(b.id.as_str()) {
                return Err(format!("blessings: {} is listed twice", b.id));
            }
            if b.name.trim().is_empty() || b.desc.trim().is_empty() {
                return Err(format!(
                    "blessings: {} needs a name and a description",
                    b.id
                ));
            }
            if b.max_stacks == 0 || b.max_stacks > 9 {
                return Err(format!("blessings: {} max_stacks must be 1 to 9", b.id));
            }
            if b.effects.is_empty() {
                return Err(format!("blessings: {} does nothing", b.id));
            }
            for effect in &b.effects {
                check_effect(effect).map_err(|e| format!("blessings: {}: {e}", b.id))?;
            }
        }
        for rarity in [Rarity::Common, Rarity::Rare, Rarity::Legendary] {
            if !self.blessings.iter().any(|b| b.rarity == rarity) {
                return Err(format!(
                    "blessings: there is no {} blessing",
                    rarity.label()
                ));
            }
        }
        if self.blessings.len() < 8 {
            return Err("blessings: a run needs at least eight blessings".into());
        }

        let mut ids = HashSet::new();
        for e in &self.events {
            if !is_ident(&e.id) {
                return Err(format!(
                    "events: id {:?} must be letters, digits and underscores",
                    e.id
                ));
            }
            if !ids.insert(e.id.as_str()) {
                return Err(format!("events: {} is listed twice", e.id));
            }
            if e.title.trim().is_empty() || e.text.trim().is_empty() {
                return Err(format!("events: {} needs a title and text", e.id));
            }
            if e.options.is_empty() || e.options.len() > 4 {
                return Err(format!("events: {} needs 1 to 4 options", e.id));
            }
            // A player with no gold must always have a way out.
            if e.options.iter().all(|o| o.cost_gold > 0) {
                return Err(format!("events: {} has no free option", e.id));
            }
            for option in &e.options {
                // A gamble brings its own text; anything else needs a result line.
                let gambles = option
                    .outcomes
                    .iter()
                    .any(|o| matches!(o, Outcome::Gamble { .. }));
                if option.label.trim().is_empty() || (option.result.trim().is_empty() && !gambles) {
                    return Err(format!(
                        "events: {} has an option without label or result",
                        e.id
                    ));
                }
                for outcome in &option.outcomes {
                    check_outcome(outcome, true)
                        .map_err(|err| format!("events: {}: {err}", e.id))?;
                }
            }
        }
        if self.events.len() < 3 {
            return Err("events: a run needs at least three events".into());
        }
        Ok(())
    }
}

fn check_effect(effect: &Effect) -> Result<(), String> {
    let ok = match effect {
        Effect::HealAfterFight { pct }
        | Effect::HealOnFightStart { pct }
        | Effect::RestHeal { pct }
        | Effect::HealNow { pct } => (1..=100).contains(pct),
        Effect::EnemyHealth { pct } | Effect::EliteHealth { pct } | Effect::LordHealth { pct } => {
            // Enemies cannot start above full health, so these only go down.
            (-50..=-1).contains(pct)
        }
        Effect::EnemyLevel { delta } => (-10..=10).contains(delta) && *delta != 0,
        Effect::GoldGain { pct } => (-90..=300).contains(pct) && *pct != 0,
        Effect::ShopDiscount { pct } => (1..=90).contains(pct),
        Effect::ExtraChoice { n } => (1..=2).contains(n),
        Effect::RareChance { pct } => (1..=100).contains(pct),
        Effect::PackSize { delta } => (-3..=3).contains(delta) && *delta != 0,
        Effect::Revive { charges } => (1..=3).contains(charges),
        Effect::GoldNow { amount } => (1..=1000).contains(amount),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("effect out of range: {effect:?}"))
    }
}

fn check_outcome(outcome: &Outcome, top_level: bool) -> Result<(), String> {
    let ok = match outcome {
        Outcome::Gold { amount } => (-1000..=1000).contains(amount) && *amount != 0,
        Outcome::Heal { pct } => (1..=100).contains(pct),
        Outcome::Hurt { pct } => (1..=60).contains(pct),
        Outcome::Blessing { .. } => true,
        Outcome::Revive { charges } => (1..=3).contains(charges),
        Outcome::Ambush => true,
        Outcome::Gamble {
            pct,
            win,
            win_text,
            lose,
            lose_text,
        } => {
            if !top_level {
                return Err("a gamble inside a gamble is not allowed".into());
            }
            if win_text.trim().is_empty() || lose_text.trim().is_empty() {
                return Err("a gamble needs win_text and lose_text".into());
            }
            for inner in win.iter().chain(lose.iter()) {
                check_outcome(inner, false)?;
            }
            (1..=99).contains(pct)
        }
    };
    if ok {
        Ok(())
    } else {
        Err(format!("outcome out of range: {outcome:?}"))
    }
}
