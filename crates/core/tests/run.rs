//! Rules of a run, checked against the content the mod ships with.

use pbz_rogue_core::content::{Content, Pool, Rarity};
use pbz_rogue_core::map::NodeKind;
use pbz_rogue_core::profile::Profile;
use pbz_rogue_core::rng::Rng;
use pbz_rogue_core::run::{
    Encounter, FightStage, GameOp, Input, Phase, Run, RunError, ShopItem, SpawnSpec,
};
use pbz_rogue_core::view::view;
use pbz_rogue_core::{embedded, embedded::content};

fn c() -> Content {
    content().expect("the shipped content is valid")
}

/// Checks everything that must hold whatever the player does.
fn check_invariants(content: &Content, run: &Run) {
    let v = view(run, content);
    assert_eq!(
        v.options.len(),
        run.option_count(content),
        "view and option_count disagree in {:?}",
        run.phase
    );
    assert!(!v.title.is_empty());
    assert!(run.act < content.acts.len());
    if let Phase::Fight {
        encounter, attempt, ..
    } = &run.phase
    {
        assert!(*attempt >= 1);
        assert!(!encounter.spawns.is_empty() && encounter.spawns.len() <= 6);
        let chapters = &content.acts[run.act].chapters;
        for spawn in &encounter.spawns {
            let unit = content
                .unit(&spawn.unit_id)
                .expect("a summoned unit is in the roster");
            assert_ne!(
                unit.pool,
                Pool::Excluded,
                "{} is excluded and must never be summoned",
                unit.id
            );
            assert!(
                chapters.contains(&unit.chapter),
                "{} is not of this act",
                unit.id
            );
            assert!((1..=content.rules.max_level).contains(&spawn.level));
            assert!((content.rules.min_enemy_health_pct..=100).contains(&spawn.health_pct));
            assert!(!run.banned_units.contains(&spawn.unit_id));
        }
        if encounter.kind == NodeKind::Lord {
            assert_eq!(encounter.spawns.len(), 1);
        }
    }
    for owned in &run.blessings {
        let b = content.blessing(&owned.id).expect("a held blessing exists");
        assert!(owned.stacks >= 1 && owned.stacks <= b.max_stacks);
    }
    // A save taken at any moment loads back to the same run.
    let loaded = Run::from_json(content, &run.to_json()).expect("a save loads");
    assert_eq!(&loaded, run);
}

/// Plays a run with random choices. `win_pct` of fights are won.
fn play(content: &Content, seed: u64, heat: u32, win_pct: u32) -> Run {
    let mut run = Run::new(content, seed, heat).expect("a run starts");
    let mut bot = Rng::stream(seed, "bot", 0);
    let mut steps = 0;
    while !run.is_over() {
        steps += 1;
        assert!(steps < 3000, "seed {seed}: the run does not end");
        check_invariants(content, &run);
        if matches!(
            run.phase,
            Phase::Fight {
                stage: FightStage::Active,
                ..
            }
        ) {
            let input = if bot.chance(win_pct) {
                Input::FightWon
            } else {
                Input::FightLost
            };
            let ops = run
                .input(content, input)
                .expect("a fight result is accepted");
            assert_eq!(
                ops.first(),
                Some(&GameOp::ClearUnits),
                "the arena is cleared after a fight"
            );
            continue;
        }
        let count = run.option_count(content);
        assert!(
            count > 0,
            "seed {seed}: nothing to choose in {:?}",
            run.phase
        );
        let first = bot.below(count as u32) as usize;
        let mut moved = false;
        // Try the random pick, then every other option: one of them must be possible.
        for offset in 0..count {
            let i = (first + offset) % count;
            let before = run.clone();
            match run.input(content, Input::Choose(i)) {
                Ok(ops) => {
                    for op in &ops {
                        match op {
                            GameOp::Heal { pct } | GameOp::Hurt { pct } => {
                                assert!((1..=100).contains(pct))
                            }
                            GameOp::Spawn(spec) => assert!(content.unit(&spec.unit_id).is_some()),
                            GameOp::ClearUnits => {}
                        }
                    }
                    moved = true;
                    break;
                }
                Err(RunError::CannotAfford) | Err(RunError::BadChoice) => {
                    assert_eq!(run, before, "a refused input must change nothing");
                }
                Err(other) => panic!(
                    "seed {seed}: unexpected error {other:?} in {:?}",
                    before.phase
                ),
            }
        }
        assert!(
            moved,
            "seed {seed}: no option could be taken in {:?}",
            run.phase
        );
    }
    check_invariants(content, &run);
    run
}

#[test]
fn shipped_content_is_valid_and_sized() {
    let content = c();
    assert_eq!(content.acts.len(), 3);
    assert!(content.blessings.len() >= 20);
    assert!(content.events.len() >= 8);
    for act in 0..content.acts.len() {
        assert!(content.act_units(act, Pool::Mob).len() >= 10);
        assert!(content.act_units(act, Pool::Elite).len() >= 10);
        assert!(content.act_units(act, Pool::Lord).len() >= 2);
    }
}

#[test]
fn units_that_write_story_progress_are_excluded() {
    // Summoning these while their story mark is unset would write it into the save
    // (PhantomGladiator, story_writes.lua). They must stay out of every pool.
    let content = c();
    for id in [
        "BP_Monster_ZuoShang",
        "BP_Monster_LeiDang",
        "BP_Monster_LeiDang_P2",
        "BP_Monster_LeiDang_NPC",
        "BP_Monster_LengTu",
        "BP_Monster_MuXiaoKui_P2",
        "BP_Monster_MoTian_LongZhu_01",
        "BP_Monster_HookMan_HorseCartCombat",
        "BP_Monster_GuiChai_Black_Gun_Rider_HorseCartCombat_L",
        "BP_Monster_GuiChai_Black_TwinBlade_Rider_HorseCartCombat_L",
    ] {
        let unit = content
            .unit(id)
            .unwrap_or_else(|| panic!("{id} is missing from the roster"));
        assert_eq!(unit.pool, Pool::Excluded, "{id} must be excluded");
    }
    for unit in &content.units {
        let risky = unit.id.contains("HorseCartCombat")
            || unit.id.contains("Rider")
            || unit.id.starts_with("BP_Monster_LeiDang")
            || unit.id.starts_with("BP_Monster_LengTu")
            || unit.id.ends_with("_NPC");
        if risky {
            assert_eq!(unit.pool, Pool::Excluded, "{} must be excluded", unit.id);
        }
    }
}

#[test]
fn hundreds_of_random_runs_keep_every_rule() {
    let content = c();
    let (mut wins, mut losses) = (0, 0);
    for seed in 0..250u64 {
        let run = play(&content, seed, (seed % 4) as u32, 93);
        match run.phase {
            Phase::Victory => wins += 1,
            Phase::Defeat => losses += 1,
            ref other => panic!("a finished run is in {other:?}"),
        }
    }
    assert!(
        wins > 0 && losses > 0,
        "both endings should occur ({wins} wins, {losses} losses)"
    );
}

#[test]
fn winning_every_fight_wins_the_run() {
    let content = c();
    for seed in [1u64, 2, 3, 99, 12345] {
        let run = play(&content, seed, 0, 100);
        assert_eq!(run.phase, Phase::Victory);
        assert_eq!(run.act, content.acts.len() - 1);
        assert_eq!(run.stats.lords_won as usize, content.acts.len());
        assert_eq!(run.stats.deaths, 0);
    }
}

#[test]
fn losing_without_a_forgiven_death_ends_the_run() {
    let content = c();
    let mut run = Run::new(&content, 7, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap(); // take no boon
    assert_eq!(run.revives, 0);
    run.input(&content, Input::Choose(0)).unwrap(); // first node: always a fight
    assert!(matches!(
        run.phase,
        Phase::Fight {
            stage: FightStage::Ready,
            ..
        }
    ));
    let ops = run.input(&content, Input::Choose(0)).unwrap();
    assert_eq!(ops[0], GameOp::ClearUnits);
    assert!(ops.iter().any(|op| matches!(op, GameOp::Spawn(_))));
    let ops = run.input(&content, Input::FightLost).unwrap();
    assert_eq!(ops, vec![GameOp::ClearUnits]);
    assert_eq!(run.phase, Phase::Defeat);
    assert_eq!(run.stats.deaths, 1);
    assert_eq!(run.input(&content, Input::Choose(0)), Err(RunError::NotNow));
}

#[test]
fn a_forgiven_death_replays_the_same_fight() {
    let content = c();
    let mut run = Run::new(&content, 8, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.revives = 1;
    run.input(&content, Input::Choose(0)).unwrap();
    let Phase::Fight {
        encounter: first, ..
    } = run.phase.clone()
    else {
        panic!()
    };
    run.input(&content, Input::Choose(0)).unwrap();
    run.input(&content, Input::FightLost).unwrap();
    match &run.phase {
        Phase::Fight {
            encounter,
            stage,
            attempt,
            ..
        } => {
            assert_eq!(encounter, &first);
            assert_eq!(*stage, FightStage::Ready);
            assert_eq!(*attempt, 2);
        }
        other => panic!("expected the fight again, got {other:?}"),
    }
    assert_eq!(run.revives, 0);
    assert!(!run.notice.is_empty());
    run.input(&content, Input::Choose(0)).unwrap();
    run.input(&content, Input::FightLost).unwrap();
    assert_eq!(run.phase, Phase::Defeat);
}

#[test]
fn same_seed_and_choices_give_the_same_run() {
    let content = c();
    let a = play(&content, 4242, 1, 90);
    let b = play(&content, 4242, 1, 90);
    assert_eq!(a.to_json(), b.to_json());
    let other = play(&content, 4243, 1, 90);
    assert_ne!(a.to_json(), other.to_json());
}

#[test]
fn fight_results_are_refused_outside_an_active_fight() {
    let content = c();
    let mut run = Run::new(&content, 5, 0).unwrap();
    assert_eq!(run.input(&content, Input::FightWon), Err(RunError::NotNow));
    assert_eq!(run.input(&content, Input::FightLost), Err(RunError::NotNow));
    assert_eq!(
        run.input(&content, Input::Choose(99)),
        Err(RunError::BadChoice)
    );
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.input(&content, Input::Choose(0)).unwrap();
    // Ready: the fight has not begun, so it cannot be won yet.
    assert_eq!(run.input(&content, Input::FightWon), Err(RunError::NotNow));
    assert_eq!(
        run.input(&content, Input::Choose(1)),
        Err(RunError::BadChoice)
    );
    run.input(&content, Input::Choose(0)).unwrap();
    // Active: there is nothing to choose.
    assert_eq!(run.input(&content, Input::Choose(0)), Err(RunError::NotNow));
}

fn shop_run(content: &Content, gold: u32) -> Run {
    let mut run = Run::new(content, 21, 0).unwrap();
    let skip = run.option_count(content) - 1;
    run.input(content, Input::Choose(skip)).unwrap();
    run.input(content, Input::Choose(0)).unwrap();
    run.gold = gold;
    run.phase = Phase::Shop {
        stock: vec![
            ShopItem {
                blessing: "steady_breath".into(),
                price: 45,
                sold: false,
            },
            ShopItem {
                blessing: "paper_effigy".into(),
                price: 80,
                sold: false,
            },
        ],
        heal_price: 35,
        heal_sold: false,
    };
    run
}

#[test]
fn a_merchant_takes_gold_and_refuses_what_cannot_be_paid() {
    let content = c();
    let mut run = shop_run(&content, 100);
    let before = run.clone();
    // Out of range.
    assert_eq!(
        run.input(&content, Input::Choose(9)),
        Err(RunError::BadChoice)
    );
    assert_eq!(run, before);

    assert_eq!(run.input(&content, Input::Choose(0)).unwrap(), vec![]);
    assert_eq!(run.gold, 55);
    assert_eq!(run.stacks_of("steady_breath"), 1);
    // Sold items cannot be bought twice.
    assert_eq!(
        run.input(&content, Input::Choose(0)),
        Err(RunError::BadChoice)
    );
    // 80 gold for the effigy, 55 in the purse.
    let before = run.clone();
    assert_eq!(
        run.input(&content, Input::Choose(1)),
        Err(RunError::CannotAfford)
    );
    assert_eq!(run, before);
    // The remedy heals and is sold once.
    let ops = run.input(&content, Input::Choose(2)).unwrap();
    assert_eq!(
        ops,
        vec![GameOp::Heal {
            pct: content.rules.shop_heal_pct
        }]
    );
    assert_eq!(run.gold, 20);
    assert_eq!(
        run.input(&content, Input::Choose(2)),
        Err(RunError::BadChoice)
    );
    // Leaving returns to the road.
    run.input(&content, Input::Choose(3)).unwrap();
    assert_eq!(run.phase, Phase::Map);
}

#[test]
fn buying_an_effigy_forgives_a_death() {
    let content = c();
    let mut run = shop_run(&content, 200);
    run.input(&content, Input::Choose(1)).unwrap();
    assert_eq!(run.revives, 1);
    assert_eq!(run.gold, 120);
}

#[test]
fn blessings_change_levels_health_and_gold() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.input(&content, Input::Choose(0)).unwrap();
    let base_level = run.enemy_level(&content, NodeKind::Fight);
    assert_eq!(base_level, content.acts[0].level_start);
    assert_eq!(
        run.enemy_level(&content, NodeKind::Elite),
        base_level + content.acts[0].elite_level_bonus
    );
    assert_eq!(
        run.enemy_level(&content, NodeKind::Lord),
        content.acts[0].level_end + content.acts[0].lord_level_bonus
    );
    assert_eq!(run.enemy_health_pct(&content, NodeKind::Fight), 100);

    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "quiet_world".into(),
        stacks: 1,
    });
    assert_eq!(
        run.enemy_level(&content, NodeKind::Fight),
        1.max(base_level as i64 - 3) as u32
    );

    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "dulled_edges".into(),
        stacks: 3,
    });
    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "marked_prey".into(),
        stacks: 2,
    });
    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "hunters_feast".into(),
        stacks: 1,
    });
    assert_eq!(run.enemy_health_pct(&content, NodeKind::Fight), 78);
    assert_eq!(run.enemy_health_pct(&content, NodeKind::Elite), 54);
    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "kingslayers_oath".into(),
        stacks: 1,
    });
    assert_eq!(run.enemy_health_pct(&content, NodeKind::Lord), 58);

    let mods = run.mods(&content);
    assert_eq!(mods.enemy_level, -3);
    assert_eq!(mods.enemy_health, -22);
}

#[test]
fn enemy_health_never_drops_below_the_floor() {
    let mut content = c();
    content.rules.min_enemy_health_pct = 70;
    let mut run = Run::new(&content, 3, 0).unwrap();
    for (id, stacks) in [
        ("dulled_edges", 3),
        ("marked_prey", 2),
        ("hunters_feast", 1),
    ] {
        run.blessings.push(pbz_rogue_core::run::Owned {
            id: id.into(),
            stacks,
        });
    }
    assert_eq!(run.enemy_health_pct(&content, NodeKind::Elite), 70);
}

#[test]
fn heat_raises_enemy_levels_and_is_capped() {
    let content = c();
    let cold = Run::new(&content, 3, 0).unwrap();
    let hot = Run::new(&content, 3, 4).unwrap();
    assert_eq!(
        hot.enemy_level(&content, NodeKind::Fight),
        cold.enemy_level(&content, NodeKind::Fight) + 4 * content.rules.heat_level_step
    );
    let hottest = Run::new(&content, 3, content.rules.max_heat).unwrap();
    for kind in [NodeKind::Fight, NodeKind::Elite, NodeKind::Lord] {
        assert!(hottest.enemy_level(&content, kind) <= content.rules.max_level);
    }
    assert!(Run::new(&content, 3, content.rules.max_heat + 1).is_err());
}

#[test]
fn levels_climb_with_the_floors() {
    let content = c();
    let mut run = Run::new(&content, 77, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    let mut last = 0;
    let act = content.acts[0].clone();
    // Walk straight up the first path and read the level on every floor.
    let mut id = run.map.starts[0];
    loop {
        run.at = Some(id);
        if id == run.map.lord {
            break;
        }
        let level = run.enemy_level(&content, NodeKind::Fight);
        assert!(level >= last, "levels must not fall while climbing");
        assert!((act.level_start..=act.level_end).contains(&level));
        last = level;
        id = run.map.nodes[id].next[0];
    }
    assert_eq!(
        last, act.level_end,
        "the top row is at the act's highest level"
    );
}

#[test]
fn enemies_that_cannot_be_summoned_are_replaced_then_skipped() {
    let content = c();
    let mut run = Run::new(&content, 11, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.input(&content, Input::Choose(0)).unwrap();
    let mut banned = Vec::new();
    for round in 1..=content.rules.max_rerolls {
        let Phase::Fight { encounter, .. } = run.phase.clone() else {
            panic!("round {round}: not a fight")
        };
        let failed: Vec<String> = encounter.spawns.iter().map(|s| s.unit_id.clone()).collect();
        banned.extend(failed.clone());
        let ops = run.spawn_failed(&content, &failed).unwrap();
        assert_eq!(ops, vec![GameOp::ClearUnits]);
        let Phase::Fight {
            encounter,
            stage,
            rerolls,
            attempt,
        } = &run.phase
        else {
            panic!("round {round}: expected another fight")
        };
        assert_eq!((*stage, *rerolls, *attempt), (FightStage::Ready, round, 1));
        for spawn in &encounter.spawns {
            assert!(!banned.contains(&spawn.unit_id), "a banned unit came back");
        }
        assert!(!run.notice.is_empty());
        check_invariants(&content, &run);
    }
    // The allowance is spent: one more failure skips the fight, with no reward.
    let gold = run.gold;
    run.spawn_failed(&content, &[]).unwrap();
    assert_eq!(run.phase, Phase::Map);
    assert_eq!(run.gold, gold);
    assert!(!run.notice.is_empty());
    for id in &banned {
        assert!(run.banned_units.contains(id));
    }

    // Outside a fight there is nothing to replace.
    assert_eq!(run.spawn_failed(&content, &[]), Err(RunError::NotNow));
}

#[test]
fn the_last_lord_ends_the_run_and_earlier_lords_open_the_next_act() {
    let content = c();
    let mut run = Run::new(&content, 31, 0).unwrap();
    let lord_fight = |run: &mut Run| {
        run.at = Some(run.map.lord);
        run.phase = Phase::Fight {
            encounter: Encounter {
                kind: NodeKind::Lord,
                spawns: vec![SpawnSpec {
                    unit_id: "BP_Monster_MoTian".into(),
                    level: 12,
                    health_pct: 100,
                }],
            },
            stage: FightStage::Active,
            attempt: 1,
            rerolls: 0,
        };
    };
    lord_fight(&mut run);
    run.input(&content, Input::FightWon).unwrap();
    let Phase::Reward {
        choices,
        source,
        gold,
    } = run.phase.clone()
    else {
        panic!("a lord gives a reward")
    };
    assert_eq!(source, NodeKind::Lord);
    assert!(gold >= content.rules.gold_lord[0]);
    assert!(
        choices
            .iter()
            .any(|id| content.blessing(id).unwrap().rarity == Rarity::Legendary),
        "a lord's reward includes a legendary blessing"
    );
    run.input(&content, Input::Choose(choices.len())).unwrap();
    assert_eq!(run.phase, Phase::ActClear);
    let old_map = run.map.clone();
    let ops = run.input(&content, Input::Choose(0)).unwrap();
    assert_eq!(
        ops,
        vec![GameOp::Heal {
            pct: content.rules.act_clear_heal_pct
        }]
    );
    assert_eq!((run.act, run.at, run.phase.clone()), (1, None, Phase::Map));
    assert_ne!(run.map, old_map);
    assert!(run.visited.is_empty());

    run.act = content.acts.len() - 1;
    lord_fight(&mut run);
    run.input(&content, Input::FightWon).unwrap();
    assert_eq!(run.phase, Phase::Victory);
}

#[test]
fn elite_rewards_include_a_rare_blessing() {
    let content = c();
    for seed in 0..40u64 {
        let mut run = Run::new(&content, seed, 0).unwrap();
        let skip = run.option_count(&content) - 1;
        run.input(&content, Input::Choose(skip)).unwrap();
        run.input(&content, Input::Choose(0)).unwrap();
        run.phase = Phase::Fight {
            encounter: Encounter {
                kind: NodeKind::Elite,
                spawns: vec![SpawnSpec {
                    unit_id: "BP_Monster_AHua_GuiTouDao".into(),
                    level: 5,
                    health_pct: 100,
                }],
            },
            stage: FightStage::Active,
            attempt: 1,
            rerolls: 0,
        };
        run.input(&content, Input::FightWon).unwrap();
        let Phase::Reward { choices, .. } = &run.phase else {
            panic!()
        };
        assert_eq!(choices.len() as u32, content.rules.reward_choices);
        let mut unique = choices.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            choices.len(),
            "an offer never repeats a blessing"
        );
        assert!(choices
            .iter()
            .any(|id| content.blessing(id).unwrap().rarity >= Rarity::Rare));
        assert_eq!(run.stats.elites_won, 1);
    }
}

#[test]
fn a_blessing_held_in_full_is_never_offered() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    // Hold everything in full except two blessings.
    for b in &content.blessings {
        if b.id != "steady_breath" && b.id != "war_chest" {
            run.blessings.push(pbz_rogue_core::run::Owned {
                id: b.id.clone(),
                stacks: b.max_stacks,
            });
        }
    }
    run.at = Some(run.map.starts[0]);
    run.phase = Phase::Fight {
        encounter: Encounter {
            kind: NodeKind::Fight,
            spawns: vec![SpawnSpec {
                unit_id: "BP_Monster_MaleThug_Sword_Blue".into(),
                level: 3,
                health_pct: 100,
            }],
        },
        stage: FightStage::Active,
        attempt: 1,
        rerolls: 0,
    };
    run.input(&content, Input::FightWon).unwrap();
    let Phase::Reward { choices, .. } = run.phase.clone() else {
        panic!()
    };
    let mut sorted = choices.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["steady_breath".to_string(), "war_chest".to_string()]
    );
    assert_eq!(view(&run, &content).options.len(), 3);
}

#[test]
fn events_charge_gold_and_report_what_happened() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.input(&content, Input::Choose(0)).unwrap();
    run.phase = Phase::Event {
        id: "healers_hut".into(),
        result: None,
        ambush: false,
    };
    run.gold = 10;
    let before = run.clone();
    assert_eq!(
        run.input(&content, Input::Choose(0)),
        Err(RunError::CannotAfford)
    );
    assert_eq!(run, before);
    run.gold = 50;
    let ops = run.input(&content, Input::Choose(0)).unwrap();
    assert_eq!(ops, vec![GameOp::Heal { pct: 50 }]);
    assert_eq!(run.gold, 10);
    let Phase::Event {
        result: Some(text),
        ambush,
        ..
    } = run.phase.clone()
    else {
        panic!()
    };
    assert!(text.contains("health") && !ambush);
    assert_eq!(
        run.input(&content, Input::Choose(1)),
        Err(RunError::BadChoice)
    );
    run.input(&content, Input::Choose(0)).unwrap();
    assert_eq!(run.phase, Phase::Map);

    // The narrow road: fighting through leads to an elite fight with its reward.
    run.phase = Phase::Event {
        id: "narrow_road".into(),
        result: None,
        ambush: false,
    };
    run.input(&content, Input::Choose(0)).unwrap();
    assert!(matches!(run.phase, Phase::Event { ambush: true, .. }));
    run.input(&content, Input::Choose(0)).unwrap();
    let Phase::Fight {
        encounter, stage, ..
    } = run.phase.clone()
    else {
        panic!("an ambush is a fight")
    };
    assert_eq!(
        (encounter.kind, stage),
        (NodeKind::Elite, FightStage::Ready)
    );
    run.input(&content, Input::Choose(0)).unwrap();
    run.input(&content, Input::FightWon).unwrap();
    assert!(matches!(
        run.phase,
        Phase::Reward {
            source: NodeKind::Elite,
            ..
        }
    ));

    // Climbing around hurts and never kills: the bridge keeps the last point of health.
    run.phase = Phase::Event {
        id: "narrow_road".into(),
        result: None,
        ambush: false,
    };
    assert_eq!(
        run.input(&content, Input::Choose(2)).unwrap(),
        vec![GameOp::Hurt { pct: 12 }]
    );
}

#[test]
fn every_event_option_can_be_taken_with_enough_gold() {
    let content = c();
    for event in &content.events {
        for (i, _) in event.options.iter().enumerate() {
            for seed in 0..6u64 {
                let mut run = Run::new(&content, seed, 0).unwrap();
                let skip = run.option_count(&content) - 1;
                run.input(&content, Input::Choose(skip)).unwrap();
                run.input(&content, Input::Choose(0)).unwrap();
                run.gold = 500;
                run.phase = Phase::Event {
                    id: event.id.clone(),
                    result: None,
                    ambush: false,
                };
                run.input(&content, Input::Choose(i))
                    .unwrap_or_else(|e| panic!("{} option {i}: {e}", event.id));
                let Phase::Event {
                    result: Some(text), ..
                } = &run.phase
                else {
                    panic!()
                };
                assert!(
                    !text.trim().is_empty(),
                    "{} option {i} says nothing",
                    event.id
                );
                check_invariants(&content, &run);
                run.input(&content, Input::Choose(0)).unwrap();
                check_invariants(&content, &run);
            }
        }
    }
}

#[test]
fn a_shrine_heals_or_pays() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    run.at = Some(run.map.starts[0]);
    run.phase = Phase::Rest;
    let mut resting = run.clone();
    assert_eq!(
        resting.input(&content, Input::Choose(0)).unwrap(),
        vec![GameOp::Heal {
            pct: content.rules.rest_heal_pct
        }]
    );
    assert_eq!(resting.phase, Phase::Map);
    let mut searching = run.clone();
    assert_eq!(searching.input(&content, Input::Choose(1)).unwrap(), vec![]);
    assert_eq!(searching.gold, content.rules.rest_gold);
    assert_eq!(
        run.input(&content, Input::Choose(2)),
        Err(RunError::BadChoice)
    );

    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "shrine_incense".into(),
        stacks: 2,
    });
    assert_eq!(
        run.input(&content, Input::Choose(0)).unwrap(),
        vec![GameOp::Heal {
            pct: content.rules.rest_heal_pct + 20
        }]
    );
}

#[test]
fn gold_gain_blessings_scale_what_is_earned() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    run.at = Some(run.map.starts[0]);
    run.blessings.push(pbz_rogue_core::run::Owned {
        id: "travelers_purse".into(),
        stacks: 2,
    });
    run.phase = Phase::Rest;
    run.input(&content, Input::Choose(1)).unwrap();
    assert_eq!(run.gold, content.rules.rest_gold * 140 / 100);
}

#[test]
fn saves_from_other_versions_or_with_bad_data_are_refused() {
    let content = c();
    let run = Run::new(&content, 3, 0).unwrap();
    let good = run.to_json();
    assert!(Run::from_json(&content, &good).is_ok());
    assert!(Run::from_json(&content, "not json").is_err());
    assert!(Run::from_json(
        &content,
        &good.replacen("\"version\": 1", "\"version\": 2", 1)
    )
    .is_err());

    let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
    value["run"]["blessings"] = serde_json::json!([{"id": "no_such_blessing", "stacks": 1}]);
    assert!(Run::from_json(&content, &value.to_string()).is_err());

    let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
    value["run"]["act"] = serde_json::json!(7);
    assert!(Run::from_json(&content, &value.to_string()).is_err());

    let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
    value["run"]["at"] = serde_json::json!(9999);
    assert!(Run::from_json(&content, &value.to_string()).is_err());

    // A fight naming an excluded unit must not load: it would be summoned.
    let mut value: serde_json::Value = serde_json::from_str(&good).unwrap();
    value["run"]["at"] = serde_json::json!(0);
    value["run"]["phase"] = serde_json::json!({
        "phase": "fight", "stage": "ready", "attempt": 1, "rerolls": 0,
        "encounter": {"kind": "elite", "spawns": [{"unit_id": "BP_Monster_LeiDang", "level": 5, "health_pct": 100}]}
    });
    assert!(Run::from_json(&content, &value.to_string()).is_err());
}

#[test]
fn bad_content_is_refused_with_a_reason() {
    let ok = |roster: &str, acts: &str, blessings: &str, events: &str| {
        Content::from_json(roster, acts, blessings, events)
    };
    let (r, a, b, e) = (
        embedded::ROSTER,
        embedded::ACTS,
        embedded::BLESSINGS,
        embedded::EVENTS,
    );
    assert!(ok(r, a, b, e).is_ok());

    // A duplicated unit.
    let mut roster: serde_json::Value = serde_json::from_str(r).unwrap();
    let first = roster["units"][5].clone();
    roster["units"].as_array_mut().unwrap().push(first);
    assert!(ok(&roster.to_string(), a, b, e)
        .unwrap_err()
        .contains("twice"));

    // A unit id that could not be sent over the bridge.
    let mut roster: serde_json::Value = serde_json::from_str(r).unwrap();
    roster["units"][5]["id"] = serde_json::json!("BP Monster\tX");
    assert!(ok(&roster.to_string(), a, b, e).is_err());

    // An act with no lord.
    let mut acts: serde_json::Value = serde_json::from_str(a).unwrap();
    acts["acts"][0]["chapters"] = serde_json::json!([7]);
    assert!(ok(r, &acts.to_string(), b, e).unwrap_err().contains("lord"));

    // Levels the wrong way round.
    let mut acts: serde_json::Value = serde_json::from_str(a).unwrap();
    acts["acts"][0]["level_start"] = serde_json::json!(40);
    assert!(ok(r, &acts.to_string(), b, e).is_err());

    // An unknown rule name is a typo, not a silent default.
    let mut acts: serde_json::Value = serde_json::from_str(a).unwrap();
    acts["rules"]["rest_heal_percent"] = serde_json::json!(50);
    assert!(ok(r, &acts.to_string(), b, e).is_err());

    // A blessing that would raise enemy health above full.
    let mut blessings: serde_json::Value = serde_json::from_str(b).unwrap();
    blessings["blessings"][3]["effects"][0]["pct"] = serde_json::json!(10);
    assert!(ok(r, a, &blessings.to_string(), e).is_err());

    // An event whose every option costs gold would trap a player with none.
    let mut events: serde_json::Value = serde_json::from_str(e).unwrap();
    for option in events["events"][0]["options"].as_array_mut().unwrap() {
        option["cost_gold"] = serde_json::json!(5);
    }
    assert!(ok(r, a, b, &events.to_string())
        .unwrap_err()
        .contains("free option"));

    // A hurt that could take most of a health bar.
    let mut events: serde_json::Value = serde_json::from_str(e).unwrap();
    events["events"][0]["options"][0]["outcomes"][0]["pct"] = serde_json::json!(95);
    assert!(ok(r, a, b, &events.to_string()).is_err());
}

#[test]
fn the_profile_counts_runs_and_unlocks_heat_on_a_win() {
    let content = c();
    let mut profile = Profile::default();
    let lost = play(&content, 2, 0, 0);
    assert_eq!(lost.phase, Phase::Defeat);
    assert!(!profile.record(&content, &lost));
    assert_eq!(
        (profile.runs, profile.wins, profile.heat_unlocked),
        (1, 0, 0)
    );

    let won = play(&content, 2, 0, 100);
    assert!(profile.record(&content, &won));
    assert_eq!(
        (
            profile.runs,
            profile.wins,
            profile.heat_unlocked,
            profile.best_act
        ),
        (2, 1, 1, 3)
    );
    // Winning again at heat 0 does not unlock more.
    assert!(!profile.record(&content, &won));
    assert_eq!(profile.heat_unlocked, 1);

    // An unfinished run is not recorded.
    let fresh = Run::new(&content, 9, 0).unwrap();
    assert!(!profile.record(&content, &fresh));
    assert_eq!(profile.runs, 3);

    profile.cycle_heat();
    assert_eq!(profile.heat, 1);
    profile.cycle_heat();
    assert_eq!(profile.heat, 0);

    let back = Profile::from_json(&content, &profile.to_json()).unwrap();
    assert_eq!(back, profile);
    // A profile claiming more heat than exists is brought back in range.
    let wild =
        Profile::from_json(&content, r#"{"version":1,"heat_unlocked":99,"heat":99}"#).unwrap();
    assert_eq!(
        (wild.heat_unlocked, wild.heat),
        (content.rules.max_heat, content.rules.max_heat)
    );
    assert!(Profile::from_json(&content, r#"{"version":2}"#).is_err());
}

#[test]
fn the_map_screen_lists_each_way_on_and_draws_the_map() {
    let content = c();
    let mut run = Run::new(&content, 3, 0).unwrap();
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    let v = view(&run, &content);
    assert_eq!(v.options.len(), run.map.starts.len());
    assert!(v.options.iter().all(|o| o.starts_with("Fight")));
    assert!(!v.map_art.is_empty());
    assert!(v.status.contains("Act 1/3"));
    assert!(v.status.contains("Floor 0/11"));
}

#[test]
fn an_interrupted_fight_waits_to_be_begun_again() {
    let content = c();
    let mut run = Run::new(&content, 8, 0).unwrap();
    assert_eq!(run.fight_interrupted(), Err(RunError::NotNow));
    let skip = run.option_count(&content) - 1;
    run.input(&content, Input::Choose(skip)).unwrap();
    run.input(&content, Input::Choose(0)).unwrap();
    // Not begun yet: nothing to interrupt.
    assert_eq!(run.fight_interrupted(), Err(RunError::NotNow));
    let Phase::Fight {
        encounter: first, ..
    } = run.phase.clone()
    else {
        panic!()
    };
    run.input(&content, Input::Choose(0)).unwrap();
    let before = (run.gold, run.revives, run.stats.clone());
    assert_eq!(run.fight_interrupted().unwrap(), vec![GameOp::ClearUnits]);
    match &run.phase {
        Phase::Fight {
            encounter,
            stage,
            attempt,
            ..
        } => {
            assert_eq!(
                (encounter, *stage, *attempt),
                (&first, FightStage::Ready, 1)
            );
        }
        other => panic!("expected the same fight, got {other:?}"),
    }
    assert_eq!(
        (run.gold, run.revives, run.stats.clone()),
        before,
        "an interruption costs and gives nothing"
    );
    assert!(!run.notice.is_empty());
    check_invariants(&content, &run);
    // It can be begun and won as usual.
    assert!(run
        .input(&content, Input::Choose(0))
        .unwrap()
        .iter()
        .any(|op| matches!(op, GameOp::Spawn(_))));
    run.input(&content, Input::FightWon).unwrap();
    assert!(matches!(run.phase, Phase::Reward { .. }));
}
