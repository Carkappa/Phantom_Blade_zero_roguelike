//! What the player sees: each phase of a run as a title, some lines and numbered options.
//!
//! The director shows a `View` both in its own window and on the in-game panel. Option `i` of a
//! view is `Input::Choose(i)`.

use crate::content::{Blessing, Content};
use crate::map::NodeKind;
use crate::run::{FightStage, Phase, Run};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub title: String,
    /// Act, floor, gold and the like.
    pub status: String,
    pub body: Vec<String>,
    pub options: Vec<String>,
    /// The act map for a fixed-width display; empty when the map is not the subject.
    pub map_art: Vec<String>,
}

fn blessing_line(b: &Blessing, held: u32) -> String {
    let stack = if held > 0 {
        format!(" (you hold {held})")
    } else {
        String::new()
    };
    format!("{} [{}] - {}{}", b.name, b.rarity.label(), b.desc, stack)
}

fn offer(run: &Run, content: &Content, ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|id| match content.blessing(id) {
            Some(b) => blessing_line(b, run.stacks_of(id)),
            None => id.clone(),
        })
        .collect()
}

fn held_summary(run: &Run, content: &Content) -> Option<String> {
    if run.blessings.is_empty() {
        return None;
    }
    let names: Vec<String> = run
        .blessings
        .iter()
        .map(|o| {
            let name = content
                .blessing(&o.id)
                .map_or(o.id.as_str(), |b| b.name.as_str());
            if o.stacks > 1 {
                format!("{name} x{}", o.stacks)
            } else {
                name.to_string()
            }
        })
        .collect();
    Some(format!("Blessings: {}", names.join(", ")))
}

fn stats_lines(run: &Run) -> Vec<String> {
    let s = &run.stats;
    vec![
        format!(
            "Fights won: {} (elites {}, lords {})",
            s.fights_won, s.elites_won, s.lords_won
        ),
        format!("Deaths: {}   Gold earned: {}", s.deaths, s.gold_earned),
        format!("Seed: {}   Heat: {}", run.seed, run.heat),
    ]
}

pub fn status_line(run: &Run, content: &Content) -> String {
    let act_name = content.acts.get(run.act).map_or("?", |a| a.name.as_str());
    format!(
        "Act {}/{}: {} | Floor {}/{} | Gold {} | Forgiven deaths {} | Heat {}",
        run.act + 1,
        content.acts.len(),
        act_name,
        run.floor_number(),
        run.map.floors + 1,
        run.gold,
        run.revives,
        run.heat
    )
}

pub fn view(run: &Run, content: &Content) -> View {
    let mut v = View {
        status: status_line(run, content),
        ..View::default()
    };
    if !run.notice.is_empty() {
        v.body.push(run.notice.clone());
    }
    match &run.phase {
        Phase::Boon { choices } => {
            v.title = "A Parting Gift".into();
            v.body
                .push("Choose one blessing to carry into the run.".into());
            v.options = offer(run, content, choices);
            v.options.push("Take nothing".into());
        }
        Phase::Map => {
            v.title = "The Road".into();
            v.body.push("Choose where to go.".into());
            v.body.extend(held_summary(run, content));
            let here = run.at.and_then(|id| run.map.node(id)).map(|n| n.col);
            for id in run.map_options() {
                let Some(node) = run.map.node(id) else {
                    continue;
                };
                let side = match here {
                    _ if node.kind == NodeKind::Lord => String::new(),
                    Some(col) if node.col < col => " (left)".to_string(),
                    Some(col) if node.col > col => " (right)".to_string(),
                    Some(_) => " (ahead)".to_string(),
                    None => format!(" (path {})", node.col + 1),
                };
                let mut beyond: Vec<&str> = node
                    .next
                    .iter()
                    .filter_map(|n| run.map.node(*n))
                    .map(|n| n.kind.label())
                    .collect();
                beyond.dedup();
                let tail = if beyond.is_empty() {
                    String::new()
                } else {
                    format!("  ->  then {}", beyond.join(" / "))
                };
                v.options
                    .push(format!("{}{}{}", node.kind.label(), side, tail));
            }
            v.map_art = run.map.render(run.at, &run.visited);
            v.map_art.push(String::new());
            v.map_art.push(
                "F fight  E elite  R shrine  $ merchant  ? unknown  T treasure  L lord".into(),
            );
        }
        Phase::Fight {
            encounter,
            stage,
            attempt,
            ..
        } => {
            let act_name = content
                .acts
                .get(run.act)
                .map_or("this land", |a| a.name.as_str());
            v.title = match (encounter.kind, stage) {
                (NodeKind::Lord, _) => format!("The Lord of {act_name}"),
                (NodeKind::Elite, _) => "An Elite Foe".into(),
                (_, _) => "A Fight".into(),
            };
            if *attempt > 1 {
                v.body.push(format!("Attempt {attempt}."));
            }
            for spawn in &encounter.spawns {
                let name = content
                    .unit(&spawn.unit_id)
                    .map_or(spawn.unit_id.clone(), |u| u.display());
                let health = if spawn.health_pct < 100 {
                    format!(", {}% health", spawn.health_pct)
                } else {
                    String::new()
                };
                v.body
                    .push(format!("  {name}  (level {}{health})", spawn.level));
            }
            match stage {
                FightStage::Ready => {
                    v.body
                        .push("Stand on open ground. They appear in front of you.".into());
                    v.options.push("Begin the fight".into());
                }
                FightStage::Active => v.body.push("Defeat them all.".into()),
            }
        }
        Phase::Reward {
            gold,
            choices,
            source,
        } => {
            v.title = if *source == NodeKind::Treasure {
                "Treasure"
            } else {
                "Spoils"
            }
            .into();
            v.body.push(format!("You gain {gold} gold."));
            if choices.is_empty() {
                v.body.push("You already hold every blessing.".into());
            } else {
                v.body.push("Choose a blessing.".into());
            }
            v.options = offer(run, content, choices);
            v.options.push("Take nothing".into());
        }
        Phase::Rest => {
            v.title = "A Shrine".into();
            let pct = (content.rules.rest_heal_pct + run.mods(content).rest_heal).min(100);
            v.body.push("A quiet place off the road.".into());
            v.options.push(format!("Rest (heal {pct}% of your health)"));
            v.options.push("Search the grounds (find some gold)".into());
        }
        Phase::Shop {
            stock,
            heal_price,
            heal_sold,
        } => {
            v.title = "A Merchant".into();
            v.body.push(format!("You have {} gold.", run.gold));
            for item in stock {
                let line = match content.blessing(&item.blessing) {
                    Some(b) => blessing_line(b, run.stacks_of(&item.blessing)),
                    None => item.blessing.clone(),
                };
                v.options.push(if item.sold {
                    format!("(sold) {line}")
                } else {
                    format!("{line} - {} gold", item.price)
                });
            }
            let pct = content.rules.shop_heal_pct;
            v.options.push(if *heal_sold {
                "(sold) Remedy".to_string()
            } else {
                format!("Remedy: heal {pct}% of your health - {heal_price} gold")
            });
            v.options.push("Leave".into());
        }
        Phase::Event { id, result, ambush } => match (content.event(id), result) {
            (Some(event), None) => {
                v.title = event.title.clone();
                v.body.push(event.text.clone());
                for option in &event.options {
                    v.options.push(if option.cost_gold > 0 {
                        format!("{} (costs {} gold)", option.label, option.cost_gold)
                    } else {
                        option.label.clone()
                    });
                }
            }
            (event, Some(text)) => {
                v.title = event.map_or_else(|| "An Encounter".to_string(), |e| e.title.clone());
                v.body.push(text.clone());
                v.options
                    .push(if *ambush { "Draw your blade" } else { "Go on" }.into());
            }
            (None, None) => {
                v.title = "An Encounter".into();
            }
        },
        Phase::ActClear => {
            let cleared = content
                .acts
                .get(run.act)
                .map_or("The act", |a| a.name.as_str());
            v.title = format!("{cleared} is cleared");
            v.body.extend(stats_lines(run));
            let next = content
                .acts
                .get(run.act + 1)
                .map_or("the next act", |a| a.name.as_str());
            v.options.push(format!("Go on to {next}"));
        }
        Phase::Victory => {
            v.title = "Victory".into();
            v.body.push("Every lord has fallen. The run is won.".into());
            v.body.extend(stats_lines(run));
            v.body.extend(held_summary(run, content));
        }
        Phase::Defeat => {
            v.title = "You Have Fallen".into();
            v.body.push("The run ends here.".into());
            v.body.extend(stats_lines(run));
            v.body.extend(held_summary(run, content));
        }
    }
    v
}
