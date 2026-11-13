//! One run, from the first blessing to victory or death.
//!
//! `Run` is a state machine. The director feeds it what the player chose and what happened in the
//! game (`Input`) and gets back what the game must do next (`GameOp`). The run never talks to the
//! game itself, so every rule can be tested here.

use crate::content::{Content, Effect, Mods, Outcome, Pool, Rarity};
use crate::map::{ActMap, MapParams, NodeKind};
use crate::rng::Rng;
use serde::{Deserialize, Serialize};

pub const SAVE_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owned {
    pub id: String,
    pub stacks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnSpec {
    pub unit_id: String,
    pub level: u32,
    /// Starting health as a share of the unit's maximum, 1 to 100.
    pub health_pct: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encounter {
    pub kind: NodeKind,
    pub spawns: Vec<SpawnSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FightStage {
    /// The player has seen who waits and has not begun.
    Ready,
    /// The enemies are in the world.
    Active,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopItem {
    pub blessing: String,
    pub price: u32,
    pub sold: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// The first blessing of the run.
    Boon {
        choices: Vec<String>,
    },
    /// Choosing the next node.
    Map,
    Fight {
        encounter: Encounter,
        stage: FightStage,
        attempt: u32,
        /// How many times this fight's enemies were replaced because they could not be summoned.
        rerolls: u32,
    },
    Reward {
        gold: u32,
        choices: Vec<String>,
        source: NodeKind,
    },
    Rest,
    Shop {
        stock: Vec<ShopItem>,
        heal_price: u32,
        heal_sold: bool,
    },
    Event {
        id: String,
        /// Set once an option is taken.
        result: Option<String>,
        /// A fight follows the result.
        ambush: bool,
    },
    ActClear,
    Victory,
    Defeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// The player picked option `0..n` of the current screen.
    Choose(usize),
    /// Every enemy of the active fight is dead.
    FightWon,
    /// The player died in the active fight.
    FightLost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameOp {
    /// Remove every enemy the mod summoned.
    ClearUnits,
    Spawn(SpawnSpec),
    /// Heal the player by this share of maximum health.
    Heal {
        pct: u32,
    },
    /// Take this share of maximum health from the player, never the last point.
    Hurt {
        pct: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// That input means nothing in the current phase.
    NotNow,
    /// There is no such option.
    BadChoice,
    CannotAfford,
    /// The data cannot provide what the run needs. Not expected with validated content.
    Content(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::NotNow => write!(f, "that cannot be done now"),
            RunError::BadChoice => write!(f, "there is no such option"),
            RunError::CannotAfford => write!(f, "not enough gold"),
            RunError::Content(why) => write!(f, "content problem: {why}"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStats {
    pub fights_won: u32,
    pub elites_won: u32,
    pub lords_won: u32,
    pub deaths: u32,
    pub gold_earned: u32,
    pub nodes_visited: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub version: u32,
    pub seed: u64,
    pub heat: u32,
    /// Index into `Content::acts`.
    pub act: usize,
    pub map: ActMap,
    /// The node the player is on; `None` before the first step of an act.
    pub at: Option<usize>,
    pub visited: Vec<usize>,
    pub phase: Phase,
    pub gold: u32,
    pub blessings: Vec<Owned>,
    pub revives: u32,
    pub stats: RunStats,
    /// Bosses already fought this run, so they do not come twice.
    pub used_units: Vec<String>,
    /// Units the game could not summon. Never picked again this run.
    pub banned_units: Vec<String>,
    pub seen_events: Vec<String>,
    /// One line about what just happened, for the next screen.
    pub notice: String,
    rng_encounter: Rng,
    rng_reward: Rng,
    rng_event: Rng,
    rng_shop: Rng,
}

#[derive(Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    run: Run,
}

fn map_params(content: &Content, act: usize) -> Result<MapParams, RunError> {
    content
        .acts
        .get(act)
        .map(|a| MapParams::standard(a.floors))
        .ok_or_else(|| RunError::Content(format!("there is no act {}", act + 1)))
}

fn make_map(content: &Content, seed: u64, act: usize) -> Result<ActMap, RunError> {
    let params = map_params(content, act)?;
    let mut rng = Rng::stream(seed, "map", act as u64);
    ActMap::generate(&mut rng, &params).map_err(RunError::Content)
}

impl Run {
    pub fn new(content: &Content, seed: u64, heat: u32) -> Result<Run, RunError> {
        if heat > content.rules.max_heat {
            return Err(RunError::Content(format!(
                "heat {heat} is above the maximum of {}",
                content.rules.max_heat
            )));
        }
        let map = make_map(content, seed, 0)?;
        let mut run = Run {
            version: SAVE_VERSION,
            seed,
            heat,
            act: 0,
            map,
            at: None,
            visited: Vec::new(),
            phase: Phase::Map,
            gold: 0,
            blessings: Vec::new(),
            revives: 0,
            stats: RunStats::default(),
            used_units: Vec::new(),
            banned_units: Vec::new(),
            seen_events: Vec::new(),
            notice: String::new(),
            rng_encounter: Rng::stream(seed, "encounter", 0),
            rng_reward: Rng::stream(seed, "reward", 0),
            rng_event: Rng::stream(seed, "event", 0),
            rng_shop: Rng::stream(seed, "shop", 0),
        };
        let choices = run.roll_offer(content, content.rules.boon_choices, None);
        run.phase = Phase::Boon { choices };
        Ok(run)
    }

    pub fn to_json(&self) -> String {
        let file = SaveFile {
            version: SAVE_VERSION,
            run: self.clone(),
        };
        serde_json::to_string_pretty(&file).expect("a run always serializes")
    }

    /// Loads a saved run and checks it against the content it is to be played with.
    pub fn from_json(content: &Content, text: &str) -> Result<Run, String> {
        let file: SaveFile =
            serde_json::from_str(text).map_err(|e| format!("the save cannot be read: {e}"))?;
        if file.version != SAVE_VERSION || file.run.version != SAVE_VERSION {
            return Err(format!(
                "the save is version {}, this build reads version {SAVE_VERSION}",
                file.version
            ));
        }
        let run = file.run;
        if run.act >= content.acts.len() {
            return Err("the save is in an act this content does not have".into());
        }
        if run.heat > content.rules.max_heat {
            return Err("the save has more heat than this content allows".into());
        }
        let params = map_params(content, run.act).map_err(|e| e.to_string())?;
        run.map
            .validate(&params)
            .map_err(|e| format!("the saved map is broken: {e}"))?;
        if let Some(at) = run.at {
            if run.map.node(at).is_none() {
                return Err("the save stands on a node that does not exist".into());
            }
        }
        for owned in &run.blessings {
            let Some(blessing) = content.blessing(&owned.id) else {
                return Err(format!("the save holds an unknown blessing: {}", owned.id));
            };
            if owned.stacks == 0 || owned.stacks > blessing.max_stacks {
                return Err(format!(
                    "the save holds an impossible stack of {}",
                    owned.id
                ));
            }
        }
        let known_blessings = |ids: &[String]| ids.iter().all(|id| content.blessing(id).is_some());
        let known_spawns = |enc: &Encounter| {
            !enc.spawns.is_empty()
                && enc.kind.is_fight()
                && enc.spawns.iter().all(|s| {
                    content
                        .unit(&s.unit_id)
                        .is_some_and(|u| u.pool != Pool::Excluded)
                        && (1..=content.rules.max_level).contains(&s.level)
                        && (1..=100).contains(&s.health_pct)
                })
        };
        let phase_ok = match &run.phase {
            Phase::Boon { choices } => known_blessings(choices),
            Phase::Reward { choices, .. } => known_blessings(choices) && run.at.is_some(),
            Phase::Shop { stock, .. } => {
                run.at.is_some()
                    && stock
                        .iter()
                        .all(|i| content.blessing(&i.blessing).is_some())
            }
            Phase::Fight {
                encounter, attempt, ..
            } => run.at.is_some() && *attempt >= 1 && known_spawns(encounter),
            Phase::Event { id, .. } => run.at.is_some() && content.event(id).is_some(),
            Phase::Rest | Phase::ActClear => run.at.is_some(),
            Phase::Map | Phase::Victory | Phase::Defeat => true,
        };
        if !phase_ok {
            return Err("the save is in a state this content cannot continue".into());
        }
        Ok(run)
    }

    pub fn is_over(&self) -> bool {
        matches!(self.phase, Phase::Victory | Phase::Defeat)
    }

    pub fn mods(&self, content: &Content) -> Mods {
        let mut mods = Mods::default();
        for owned in &self.blessings {
            if let Some(blessing) = content.blessing(&owned.id) {
                for effect in &blessing.effects {
                    mods.add(effect, owned.stacks);
                }
            }
        }
        mods
    }

    pub fn stacks_of(&self, id: &str) -> u32 {
        self.blessings
            .iter()
            .find(|o| o.id == id)
            .map_or(0, |o| o.stacks)
    }

    /// The nodes the player may step to now.
    pub fn map_options(&self) -> Vec<usize> {
        self.map.options_from(self.at)
    }

    /// The floor the player stands on, counted from 1; 0 before the first step.
    pub fn floor_number(&self) -> usize {
        self.at
            .and_then(|id| self.map.node(id))
            .map_or(0, |n| n.floor + 1)
    }

    /// How many options the current screen offers.
    pub fn option_count(&self, content: &Content) -> usize {
        match &self.phase {
            Phase::Boon { choices } => choices.len() + 1,
            Phase::Map => self.map_options().len(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            } => 1,
            Phase::Fight {
                stage: FightStage::Active,
                ..
            } => 0,
            Phase::Reward { choices, .. } => choices.len() + 1,
            Phase::Rest => 2,
            Phase::Shop { stock, .. } => stock.len() + 2,
            Phase::Event {
                id, result: None, ..
            } => content.event(id).map_or(0, |e| e.options.len()),
            Phase::Event {
                result: Some(_), ..
            } => 1,
            Phase::ActClear => 1,
            Phase::Victory | Phase::Defeat => 0,
        }
    }

    /// Applies one input. On an error nothing has changed.
    pub fn input(&mut self, content: &Content, input: Input) -> Result<Vec<GameOp>, RunError> {
        let mut next = self.clone();
        let ops = next.step(content, input)?;
        *self = next;
        Ok(ops)
    }

    fn step(&mut self, content: &Content, input: Input) -> Result<Vec<GameOp>, RunError> {
        let phase = self.phase.clone();
        match (phase, input) {
            (Phase::Boon { choices }, Input::Choose(i)) => {
                self.notice.clear();
                let mut ops = Vec::new();
                if i < choices.len() {
                    ops = self.gain_blessing(content, &choices[i])?;
                } else if i != choices.len() {
                    return Err(RunError::BadChoice);
                }
                self.phase = Phase::Map;
                Ok(ops)
            }

            (Phase::Map, Input::Choose(i)) => {
                let options = self.map_options();
                let id = *options.get(i).ok_or(RunError::BadChoice)?;
                let kind = self.map.node(id).ok_or(RunError::BadChoice)?.kind;
                self.notice.clear();
                self.at = Some(id);
                self.visited.push(id);
                self.stats.nodes_visited += 1;
                self.enter(content, kind)
            }

            (
                Phase::Fight {
                    encounter,
                    stage: FightStage::Ready,
                    attempt,
                    rerolls,
                },
                Input::Choose(0),
            ) => {
                self.notice.clear();
                let mods = self.mods(content);
                let mut ops = vec![GameOp::ClearUnits];
                if mods.heal_on_fight_start > 0 {
                    ops.push(GameOp::Heal {
                        pct: mods.heal_on_fight_start.min(100),
                    });
                }
                ops.extend(encounter.spawns.iter().cloned().map(GameOp::Spawn));
                self.phase = Phase::Fight {
                    encounter,
                    stage: FightStage::Active,
                    attempt,
                    rerolls,
                };
                Ok(ops)
            }

            (
                Phase::Fight {
                    encounter,
                    stage: FightStage::Active,
                    ..
                },
                Input::FightWon,
            ) => {
                let mods = self.mods(content);
                let mut ops = vec![GameOp::ClearUnits];
                if mods.heal_after_fight > 0 {
                    ops.push(GameOp::Heal {
                        pct: mods.heal_after_fight.min(100),
                    });
                }
                self.stats.fights_won += 1;
                let rules = &content.rules;
                let (range, guarantee) = match encounter.kind {
                    NodeKind::Lord => {
                        self.stats.lords_won += 1;
                        (rules.gold_lord, Some(Rarity::Legendary))
                    }
                    NodeKind::Elite => {
                        self.stats.elites_won += 1;
                        (rules.gold_elite, Some(Rarity::Rare))
                    }
                    _ => (rules.gold_fight, None),
                };
                let base = self.rng_reward.range(range[0], range[1]);
                let gold = self.earn(content, base);
                self.notice.clear();
                // The lord of the last act ends the run; there is nothing left to spend a reward on.
                let standing_on_lord = self.at == Some(self.map.lord);
                if encounter.kind == NodeKind::Lord
                    && standing_on_lord
                    && self.act + 1 == content.acts.len()
                {
                    self.phase = Phase::Victory;
                    return Ok(ops);
                }
                let count = rules.reward_choices + mods.extra_choice;
                let choices = self.roll_offer(content, count, guarantee);
                self.phase = Phase::Reward {
                    gold,
                    choices,
                    source: encounter.kind,
                };
                Ok(ops)
            }

            (
                Phase::Fight {
                    encounter,
                    stage: FightStage::Active,
                    attempt,
                    rerolls,
                },
                Input::FightLost,
            ) => {
                self.stats.deaths += 1;
                if self.revives > 0 {
                    self.revives -= 1;
                    self.notice = format!(
                        "A death is forgiven. {} left.",
                        match self.revives {
                            0 => "None".to_string(),
                            n => n.to_string(),
                        }
                    );
                    self.phase = Phase::Fight {
                        encounter,
                        stage: FightStage::Ready,
                        attempt: attempt + 1,
                        rerolls,
                    };
                } else {
                    self.notice.clear();
                    self.phase = Phase::Defeat;
                }
                Ok(vec![GameOp::ClearUnits])
            }

            (Phase::Reward { choices, .. }, Input::Choose(i)) => {
                self.notice.clear();
                let mut ops = Vec::new();
                if i < choices.len() {
                    ops = self.gain_blessing(content, &choices[i])?;
                } else if i != choices.len() {
                    return Err(RunError::BadChoice);
                }
                self.leave_node(content);
                Ok(ops)
            }

            (Phase::Rest, Input::Choose(i)) => {
                let mods = self.mods(content);
                let ops = match i {
                    0 => {
                        let pct = (content.rules.rest_heal_pct + mods.rest_heal).min(100);
                        self.notice = format!("You rest. {pct}% of your health returns.");
                        if pct > 0 {
                            vec![GameOp::Heal { pct }]
                        } else {
                            Vec::new()
                        }
                    }
                    1 => {
                        let gold = self.earn(content, content.rules.rest_gold);
                        self.notice =
                            format!("You search the shrine grounds and find {gold} gold.");
                        Vec::new()
                    }
                    _ => return Err(RunError::BadChoice),
                };
                self.leave_node(content);
                Ok(ops)
            }

            (
                Phase::Shop {
                    mut stock,
                    heal_price,
                    heal_sold,
                },
                Input::Choose(i),
            ) => {
                if i < stock.len() {
                    if stock[i].sold {
                        return Err(RunError::BadChoice);
                    }
                    if self.gold < stock[i].price {
                        return Err(RunError::CannotAfford);
                    }
                    let id = stock[i].blessing.clone();
                    // Refuse before taking the gold if the blessing cannot be held.
                    let blessing = content
                        .blessing(&id)
                        .ok_or_else(|| RunError::Content(format!("unknown blessing {id}")))?;
                    if self.stacks_of(&id) >= blessing.max_stacks {
                        return Err(RunError::BadChoice);
                    }
                    self.gold -= stock[i].price;
                    stock[i].sold = true;
                    let ops = self.gain_blessing(content, &id)?;
                    self.notice = format!("Bought {}.", blessing.name);
                    self.phase = Phase::Shop {
                        stock,
                        heal_price,
                        heal_sold,
                    };
                    Ok(ops)
                } else if i == stock.len() {
                    if heal_sold {
                        return Err(RunError::BadChoice);
                    }
                    if self.gold < heal_price {
                        return Err(RunError::CannotAfford);
                    }
                    self.gold -= heal_price;
                    let pct = content.rules.shop_heal_pct;
                    self.notice = format!("The remedy returns {pct}% of your health.");
                    self.phase = Phase::Shop {
                        stock,
                        heal_price,
                        heal_sold: true,
                    };
                    Ok(if pct > 0 {
                        vec![GameOp::Heal { pct }]
                    } else {
                        Vec::new()
                    })
                } else if i == stock.len() + 1 {
                    self.notice.clear();
                    self.leave_node(content);
                    Ok(Vec::new())
                } else {
                    Err(RunError::BadChoice)
                }
            }

            (
                Phase::Event {
                    id, result: None, ..
                },
                Input::Choose(i),
            ) => {
                let event = content
                    .event(&id)
                    .ok_or_else(|| RunError::Content(format!("unknown event {id}")))?;
                let option = event.options.get(i).ok_or(RunError::BadChoice)?;
                if self.gold < option.cost_gold {
                    return Err(RunError::CannotAfford);
                }
                self.gold -= option.cost_gold;
                self.notice.clear();
                let mut ops = Vec::new();
                let mut ambush = false;
                let mut lines: Vec<String> = Vec::new();
                if !option.result.trim().is_empty() {
                    lines.push(option.result.clone());
                }
                self.apply_outcomes(content, &option.outcomes, &mut ops, &mut ambush, &mut lines)?;
                self.phase = Phase::Event {
                    id,
                    result: Some(lines.join(" ")),
                    ambush,
                };
                Ok(ops)
            }

            (
                Phase::Event {
                    result: Some(_),
                    ambush,
                    ..
                },
                Input::Choose(0),
            ) => {
                self.notice.clear();
                if ambush {
                    let encounter = self.build_encounter(content, NodeKind::Elite)?;
                    self.phase = Phase::Fight {
                        encounter,
                        stage: FightStage::Ready,
                        attempt: 1,
                        rerolls: 0,
                    };
                } else {
                    self.leave_node(content);
                }
                Ok(Vec::new())
            }

            (Phase::ActClear, Input::Choose(0)) => {
                let next_act = self.act + 1;
                if next_act >= content.acts.len() {
                    return Err(RunError::Content("there is no next act".into()));
                }
                self.map = make_map(content, self.seed, next_act)?;
                self.act = next_act;
                self.at = None;
                self.visited.clear();
                self.phase = Phase::Map;
                let pct = content.rules.act_clear_heal_pct;
                self.notice = format!("{} lies ahead.", content.acts[next_act].name);
                Ok(if pct > 0 {
                    vec![GameOp::Heal { pct }]
                } else {
                    Vec::new()
                })
            }

            (
                Phase::Fight {
                    stage: FightStage::Ready,
                    ..
                },
                Input::Choose(_),
            )
            | (
                Phase::Event {
                    result: Some(_), ..
                },
                Input::Choose(_),
            )
            | (Phase::ActClear, Input::Choose(_)) => Err(RunError::BadChoice),

            _ => Err(RunError::NotNow),
        }
    }

    /// The game could not summon these units. The fight gets other enemies; after
    /// `Rules::max_rerolls` replacements it is skipped without a reward.
    pub fn spawn_failed(
        &mut self,
        content: &Content,
        failed: &[String],
    ) -> Result<Vec<GameOp>, RunError> {
        let mut next = self.clone();
        let Phase::Fight {
            encounter,
            attempt,
            rerolls,
            ..
        } = next.phase.clone()
        else {
            return Err(RunError::NotNow);
        };
        for id in failed {
            if !next.banned_units.contains(id) {
                next.banned_units.push(id.clone());
            }
        }
        let replacement = if rerolls >= content.rules.max_rerolls {
            None
        } else {
            // A fight that cannot be rebuilt (every unit banned) is skipped as well.
            next.build_encounter(content, encounter.kind).ok()
        };
        match replacement {
            Some(encounter) => {
                next.notice =
                    "That foe could not be summoned. Another takes its place.".to_string();
                next.phase = Phase::Fight {
                    encounter,
                    stage: FightStage::Ready,
                    attempt,
                    rerolls: rerolls + 1,
                };
            }
            None => {
                next.notice = "No foe answers the summons. The way is clear.".to_string();
                let last_lord = encounter.kind == NodeKind::Lord
                    && next.at == Some(next.map.lord)
                    && next.act + 1 == content.acts.len();
                if last_lord {
                    next.phase = Phase::Victory;
                } else {
                    next.leave_node(content);
                }
            }
        }
        *self = next;
        Ok(vec![GameOp::ClearUnits])
    }

    /// The enemies of the active fight are gone without a result (the game restarted, the map
    /// changed, they were left behind). The fight waits to be begun again; nothing is lost or won.
    pub fn fight_interrupted(&mut self) -> Result<Vec<GameOp>, RunError> {
        let Phase::Fight {
            encounter,
            stage: FightStage::Active,
            attempt,
            rerolls,
        } = self.phase.clone()
        else {
            return Err(RunError::NotNow);
        };
        self.notice = "The fight was interrupted. Begin it again when you are ready.".to_string();
        self.phase = Phase::Fight {
            encounter,
            stage: FightStage::Ready,
            attempt,
            rerolls,
        };
        Ok(vec![GameOp::ClearUnits])
    }

    fn enter(&mut self, content: &Content, kind: NodeKind) -> Result<Vec<GameOp>, RunError> {
        match kind {
            NodeKind::Fight | NodeKind::Elite | NodeKind::Lord => {
                let encounter = self.build_encounter(content, kind)?;
                self.phase = Phase::Fight {
                    encounter,
                    stage: FightStage::Ready,
                    attempt: 1,
                    rerolls: 0,
                };
            }
            NodeKind::Rest => self.phase = Phase::Rest,
            NodeKind::Shop => {
                let mods = self.mods(content);
                let discount = mods.shop_discount.min(90);
                let ids = self.roll_offer_with(content, content.rules.shop_stock, None, true);
                let stock = ids
                    .into_iter()
                    .map(|id| {
                        let rarity = content.blessing(&id).map_or(Rarity::Common, |b| b.rarity);
                        ShopItem {
                            price: discounted(content.price(rarity), discount),
                            blessing: id,
                            sold: false,
                        }
                    })
                    .collect();
                self.phase = Phase::Shop {
                    stock,
                    heal_price: discounted(content.rules.shop_heal_price, discount),
                    heal_sold: false,
                };
            }
            NodeKind::Event => {
                let mut fresh: Vec<&str> = content
                    .events
                    .iter()
                    .map(|e| e.id.as_str())
                    .filter(|id| !self.seen_events.iter().any(|s| s == id))
                    .collect();
                if fresh.is_empty() {
                    self.seen_events.clear();
                    fresh = content.events.iter().map(|e| e.id.as_str()).collect();
                }
                let id = self
                    .rng_event
                    .pick(&fresh)
                    .ok_or_else(|| RunError::Content("there are no events".into()))?
                    .to_string();
                self.seen_events.push(id.clone());
                self.phase = Phase::Event {
                    id,
                    result: None,
                    ambush: false,
                };
            }
            NodeKind::Treasure => {
                let range = content.rules.gold_treasure;
                let base = self.rng_reward.range(range[0], range[1]);
                let gold = self.earn(content, base);
                let mods = self.mods(content);
                let count = content.rules.reward_choices + mods.extra_choice;
                let choices = self.roll_offer(content, count, Some(Rarity::Rare));
                self.phase = Phase::Reward {
                    gold,
                    choices,
                    source: NodeKind::Treasure,
                };
            }
        }
        Ok(Vec::new())
    }

    /// Called when the node the player stands on is finished.
    fn leave_node(&mut self, content: &Content) {
        let on_lord = self.at == Some(self.map.lord);
        self.phase = if !on_lord {
            Phase::Map
        } else if self.act + 1 == content.acts.len() {
            Phase::Victory
        } else {
            Phase::ActClear
        };
    }

    /// Adds gold after the gold-gain blessings and returns what was added.
    fn earn(&mut self, content: &Content, base: u32) -> u32 {
        let pct = (100 + i64::from(self.mods(content).gold_gain)).clamp(10, 1000);
        let gold = ((i64::from(base) * pct + 50) / 100).clamp(0, 1_000_000) as u32;
        self.gold = self.gold.saturating_add(gold).min(9_999_999);
        self.stats.gold_earned = self.stats.gold_earned.saturating_add(gold);
        gold
    }

    fn gain_blessing(&mut self, content: &Content, id: &str) -> Result<Vec<GameOp>, RunError> {
        let blessing = content
            .blessing(id)
            .ok_or_else(|| RunError::Content(format!("unknown blessing {id}")))?;
        match self.blessings.iter_mut().find(|o| o.id == id) {
            Some(owned) if owned.stacks >= blessing.max_stacks => return Err(RunError::BadChoice),
            Some(owned) => owned.stacks += 1,
            None => self.blessings.push(Owned {
                id: id.to_string(),
                stacks: 1,
            }),
        }
        let mut ops = Vec::new();
        for effect in &blessing.effects {
            match effect {
                Effect::Revive { charges } => self.revives = (self.revives + charges).min(99),
                Effect::GoldNow { amount } => {
                    self.gold = self.gold.saturating_add(*amount).min(9_999_999);
                    self.stats.gold_earned = self.stats.gold_earned.saturating_add(*amount);
                }
                Effect::HealNow { pct } => ops.push(GameOp::Heal {
                    pct: (*pct).min(100),
                }),
                _ => {}
            }
        }
        Ok(ops)
    }

    fn apply_outcomes(
        &mut self,
        content: &Content,
        outcomes: &[Outcome],
        ops: &mut Vec<GameOp>,
        ambush: &mut bool,
        lines: &mut Vec<String>,
    ) -> Result<(), RunError> {
        for outcome in outcomes {
            match outcome {
                Outcome::Gold { amount } => {
                    if *amount >= 0 {
                        let gold = self.earn(content, *amount as u32);
                        lines.push(format!("(+{gold} gold)"));
                    } else {
                        let loss = amount.unsigned_abs().min(self.gold);
                        self.gold -= loss;
                        lines.push(format!("(-{loss} gold)"));
                    }
                }
                Outcome::Heal { pct } => {
                    ops.push(GameOp::Heal {
                        pct: (*pct).min(100),
                    });
                    lines.push(format!("(+{pct}% health)"));
                }
                Outcome::Hurt { pct } => {
                    ops.push(GameOp::Hurt {
                        pct: (*pct).min(100),
                    });
                    lines.push(format!("(-{pct}% health)"));
                }
                Outcome::Blessing { rarity } => {
                    let mut pool: Vec<&str> = content
                        .blessings
                        .iter()
                        .filter(|b| b.rarity == *rarity && self.stacks_of(&b.id) < b.max_stacks)
                        .map(|b| b.id.as_str())
                        .collect();
                    if pool.is_empty() {
                        // Every blessing of that rarity is held in full: any other will do.
                        pool = content
                            .blessings
                            .iter()
                            .filter(|b| self.stacks_of(&b.id) < b.max_stacks)
                            .map(|b| b.id.as_str())
                            .collect();
                    }
                    match self.rng_event.pick(&pool).map(|id| id.to_string()) {
                        Some(id) => {
                            ops.extend(self.gain_blessing(content, &id)?);
                            let name = content
                                .blessing(&id)
                                .map_or(id.as_str(), |b| b.name.as_str());
                            lines.push(format!("(Blessing: {name})"));
                        }
                        None => lines.push("(You already hold every blessing.)".to_string()),
                    }
                }
                Outcome::Revive { charges } => {
                    self.revives = (self.revives + charges).min(99);
                    lines.push(format!("(+{charges} forgiven death)"));
                }
                Outcome::Ambush => *ambush = true,
                Outcome::Gamble {
                    pct,
                    win,
                    win_text,
                    lose,
                    lose_text,
                } => {
                    if self.rng_event.chance(*pct) {
                        lines.push(win_text.clone());
                        self.apply_outcomes(content, win, ops, ambush, lines)?;
                    } else {
                        lines.push(lose_text.clone());
                        self.apply_outcomes(content, lose, ops, ambush, lines)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Blessings to choose from: no repeats, nothing already held in full.
    fn roll_offer(
        &mut self,
        content: &Content,
        count: u32,
        guarantee: Option<Rarity>,
    ) -> Vec<String> {
        self.roll_offer_with(content, count, guarantee, false)
    }

    fn roll_offer_with(
        &mut self,
        content: &Content,
        count: u32,
        guarantee: Option<Rarity>,
        shop: bool,
    ) -> Vec<String> {
        let mods = self.mods(content);
        let rules = &content.rules;
        let mut pool: Vec<&crate::content::Blessing> = content
            .blessings
            .iter()
            .filter(|b| self.stacks_of(&b.id) < b.max_stacks)
            .collect();
        let rng = if shop {
            &mut self.rng_shop
        } else {
            &mut self.rng_reward
        };
        let mut picks: Vec<String> = Vec::new();

        if let Some(floor) = guarantee {
            let good: Vec<usize> = pool
                .iter()
                .enumerate()
                .filter(|(_, b)| b.rarity >= floor)
                .map(|(i, _)| i)
                .collect();
            if let Some(i) = rng.pick(&good).copied() {
                picks.push(pool.remove(i).id.clone());
            }
        }
        while (picks.len() as u32) < count && !pool.is_empty() {
            let weights: Vec<u32> = pool
                .iter()
                .map(|b| match b.rarity {
                    Rarity::Common => rules.weight_common,
                    Rarity::Rare => rules.weight_rare + mods.rare_chance.min(500),
                    Rarity::Legendary => rules.weight_legendary,
                })
                .collect();
            let i = match rng.weighted(&weights) {
                Some(i) => i,
                None => rng.below(pool.len() as u32) as usize,
            };
            picks.push(pool.remove(i).id.clone());
        }
        rng.shuffle(&mut picks);
        picks
    }

    /// The level of an enemy summoned on the current node.
    pub fn enemy_level(&self, content: &Content, kind: NodeKind) -> u32 {
        let Some(act) = content.acts.get(self.act) else {
            return 1;
        };
        let span = i64::from(act.level_end - act.level_start);
        let top = (self.map.floors.max(2) - 1) as i64;
        let floor = self
            .at
            .and_then(|id| self.map.node(id))
            .map_or(0, |n| n.floor) as i64;
        let base = if kind == NodeKind::Lord {
            i64::from(act.level_end)
        } else {
            i64::from(act.level_start) + (span * floor.min(top) + top / 2) / top
        };
        let bonus = match kind {
            NodeKind::Elite => act.elite_level_bonus,
            NodeKind::Lord => act.lord_level_bonus,
            _ => 0,
        };
        let level = base
            + i64::from(bonus)
            + i64::from(self.heat * content.rules.heat_level_step)
            + i64::from(self.mods(content).enemy_level);
        level.clamp(1, i64::from(content.rules.max_level)) as u32
    }

    /// The starting health of an enemy, as a share of its maximum.
    pub fn enemy_health_pct(&self, content: &Content, kind: NodeKind) -> u32 {
        let mods = self.mods(content);
        let extra = match kind {
            NodeKind::Elite => mods.elite_health,
            NodeKind::Lord => mods.lord_health,
            _ => 0,
        };
        let pct = 100 + i64::from(mods.enemy_health) + i64::from(extra);
        pct.clamp(i64::from(content.rules.min_enemy_health_pct), 100) as u32
    }

    fn build_encounter(
        &mut self,
        content: &Content,
        kind: NodeKind,
    ) -> Result<Encounter, RunError> {
        let act = content
            .acts
            .get(self.act)
            .ok_or_else(|| RunError::Content(format!("there is no act {}", self.act + 1)))?;
        let allowed = |run: &Run, pool: Pool, fresh_only: bool| -> Vec<String> {
            content
                .act_units(run.act, pool)
                .into_iter()
                .filter(|u| !run.banned_units.contains(&u.id))
                .filter(|u| !fresh_only || !run.used_units.contains(&u.id))
                .map(|u| u.id.clone())
                .collect()
        };
        // A boss not fought yet this run, if there is one.
        let pick_boss = |run: &mut Run, pool: Pool| -> Option<String> {
            let mut candidates = allowed(run, pool, true);
            if candidates.is_empty() {
                candidates = allowed(run, pool, false);
            }
            let id = run.rng_encounter.pick(&candidates)?.clone();
            if !run.used_units.contains(&id) {
                run.used_units.push(id.clone());
            }
            Some(id)
        };

        let mut units: Vec<String> = Vec::new();
        match kind {
            NodeKind::Lord => {
                // With every lord unavailable, an elite stands in.
                let id = pick_boss(self, Pool::Lord).or_else(|| pick_boss(self, Pool::Elite));
                units.extend(id);
            }
            NodeKind::Elite => units.extend(pick_boss(self, Pool::Elite)),
            _ => {
                if self.rng_encounter.chance(act.duel_chance) {
                    units.extend(pick_boss(self, Pool::Elite));
                }
            }
        }
        if units.is_empty() {
            let mobs = allowed(self, Pool::Mob, false);
            if !mobs.is_empty() {
                let size = i64::from(self.rng_encounter.range(act.pack_min, act.pack_max))
                    + i64::from(self.mods(content).pack_size);
                for _ in 0..size.clamp(1, 6) {
                    if let Some(id) = self.rng_encounter.pick(&mobs) {
                        units.push(id.clone());
                    }
                }
            }
        }
        if units.is_empty() {
            // No mobs left either: any boss that can still be summoned.
            units.extend(pick_boss(self, Pool::Elite).or_else(|| pick_boss(self, Pool::Lord)));
        }
        if units.is_empty() {
            return Err(RunError::Content(
                "no enemy of this act can be summoned".into(),
            ));
        }
        let level = self.enemy_level(content, kind);
        let health_pct = self.enemy_health_pct(content, kind);
        let spawns = units
            .into_iter()
            .map(|unit_id| SpawnSpec {
                unit_id,
                level,
                health_pct,
            })
            .collect();
        Ok(Encounter { kind, spawns })
    }
}

fn discounted(price: u32, discount_pct: u32) -> u32 {
    let keep = 100 - discount_pct.min(90);
    ((u64::from(price) * u64::from(keep) + 50) / 100).max(1) as u32
}
