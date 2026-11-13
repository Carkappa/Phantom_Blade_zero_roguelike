//! The director against a simulated game.

use pbz_rogue_core::content::Content;
use pbz_rogue_core::embedded;
use pbz_rogue_core::run::{FightStage, Phase, Run};
use pbz_rogue_director::director::{Director, Store, UserKey};
use pbz_rogue_director::protocol::{parse_state, Command, UnitStatus};
use pbz_rogue_director::sim::SimGame;
use pbz_rogue_director::transport::Transport;
use std::io;
use std::path::PathBuf;

fn content() -> Content {
    embedded::content().unwrap()
}

struct Rig {
    d: Director<SimGame>,
    now: u64,
}

impl Rig {
    fn new() -> Rig {
        Rig::with(SimGame::new(), Store::in_memory())
    }

    fn with(sim: SimGame, store: Store) -> Rig {
        let mut d = Director::new(content(), sim, store, 0);
        d.fixed_seed = Some(1234);
        Rig { d, now: 0 }
    }

    fn sim(&mut self) -> &mut SimGame {
        self.d.transport_mut()
    }

    /// The text on the in-game panel.
    fn shown(&self) -> String {
        self.d.transport().screen.1.clone()
    }

    fn tick(&mut self) {
        self.now += 100;
        self.d.tick(self.now);
    }

    /// Ticks until every command is answered and the panel shows the current screen.
    fn settle(&mut self) {
        for _ in 0..200 {
            self.tick();
            if self.d.pending_commands() == 0 && self.shown() == self.d.panel_text() {
                return;
            }
        }
        panic!(
            "the director did not settle; pending {}",
            self.d.pending_commands()
        );
    }

    fn phase(&self) -> Phase {
        self.d.run().expect("a run is in progress").phase.clone()
    }

    /// Presses a number key in the game and lets the director act on it.
    fn press(&mut self, digit: u8) {
        self.sim().press(&digit.to_string());
        self.settle();
    }

    /// From the title to the first fight, waiting to be begun.
    fn reach_first_fight(&mut self) {
        self.settle();
        self.press(1); // begin a run
        let skip = self.d.run().unwrap().option_count(self.d.content()) as u8;
        self.press(skip); // take no boon
        self.press(1); // the first node is always a fight
        assert!(matches!(
            self.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ));
    }

    fn spawn_lines(&mut self) -> Vec<String> {
        self.sim()
            .received
            .iter()
            .filter(|l| l.contains("\tspawn\t"))
            .cloned()
            .collect()
    }
}

/// The key for a sensible choice on the current screen.
fn bot_choice(run: &Run, content: &Content) -> u8 {
    let count = run.option_count(content) as u8;
    match &run.phase {
        // The last option of a merchant is "Leave"; the last of every shipped event is free.
        Phase::Shop { .. } | Phase::Event { result: None, .. } => count,
        _ => 1,
    }
}

#[test]
fn it_greets_the_bridge_and_shows_the_title() {
    let mut rig = Rig::new();
    assert!(!rig.d.connected());
    rig.settle();
    assert!(rig.d.connected() && rig.d.game_ready());
    let received = rig.sim().received.clone();
    assert!(
        received[0].ends_with("\thello\t0.1.0"),
        "first command: {}",
        received[0]
    );
    assert!(
        received.iter().any(|l| l.ends_with("\tclear")),
        "leftover enemies are cleared at the start of a session"
    );
    let (visible, text) = rig.sim().screen.clone();
    assert!(visible);
    assert!(text.starts_with("PBZ Roguelike\n"));
    assert!(text.contains(" 1. Begin a new run (heat 0)"));
    assert!(
        !text.contains(" 2. "),
        "heat cannot be changed before it is unlocked"
    );
    assert_eq!(text, rig.d.panel_text());
    assert!(rig.d.console_text().contains("Game: connected."));
}

#[test]
fn a_whole_run_is_played_with_in_game_keys() {
    let mut rig = Rig::new();
    rig.settle();
    rig.press(1);
    let mut steps = 0;
    while !rig.d.run().unwrap().is_over() {
        steps += 1;
        assert!(steps < 4000, "the run does not end");
        if let Phase::Fight {
            stage: FightStage::Active,
            encounter,
            ..
        } = rig.phase()
        {
            assert_eq!(
                rig.sim().living(),
                encounter.spawns.len(),
                "every enemy of the encounter is in the world"
            );
            rig.sim().win();
            rig.settle();
            assert!(
                !matches!(rig.phase(), Phase::Fight { .. }),
                "a won fight is over"
            );
            assert_eq!(
                rig.sim().units.len(),
                0,
                "the arena is cleared after a fight"
            );
            continue;
        }
        let key = bot_choice(rig.d.run().unwrap(), rig.d.content());
        let before = rig.phase();
        rig.press(key);
        if rig.d.run().unwrap().is_over() {
            break;
        }
        assert_ne!(rig.phase(), before, "key {key} did nothing in {before:?}");
    }
    assert_eq!(rig.phase(), Phase::Victory);
    assert_eq!(rig.d.run().unwrap().stats.lords_won, 3);
    assert_eq!(
        (
            rig.d.profile().runs,
            rig.d.profile().wins,
            rig.d.profile().heat_unlocked
        ),
        (1, 1, 1)
    );
    let text = rig.shown();
    assert!(text.starts_with("Victory\n"), "{text}");
    assert!(text.contains("Heat 1 is unlocked."));
    assert!(text.contains(" 1. Begin a new run") && text.contains(" 2. Back to the title"));

    // Back to the title: heat can now be raised, and a new run starts with it.
    rig.press(2);
    assert!(rig.d.run().is_none());
    assert!(rig.shown().contains(" 2. Change heat"));
    rig.press(2);
    assert_eq!(rig.d.profile().heat, 1);
    rig.press(1);
    assert_eq!(rig.d.run().unwrap().heat, 1);
}

#[test]
fn a_fight_summons_in_order_and_ends_on_what_the_game_reports() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    let Phase::Fight { encounter, .. } = rig.phase() else {
        unreachable!()
    };
    assert!(
        rig.spawn_lines().is_empty(),
        "nothing is summoned before the player begins"
    );

    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    let spawns = rig.spawn_lines();
    assert_eq!(spawns.len(), encounter.spawns.len());
    let count = spawns.len();
    for (slot, (line, spec)) in spawns.iter().zip(&encounter.spawns).enumerate() {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f[4], spec.unit_id);
        assert_eq!(f[5], spec.level.to_string());
        assert_eq!(f[6], spec.health_pct.to_string());
        assert_eq!(
            (f[7], f[8]),
            (slot.to_string().as_str(), count.to_string().as_str())
        );
    }
    // The arena is cleared before anything is summoned.
    let received = rig.sim().received.clone();
    let first_spawn = received
        .iter()
        .position(|l| l.contains("\tspawn\t"))
        .unwrap();
    assert!(
        received[first_spawn - 1].ends_with("\tclear")
            || received[first_spawn - 2].ends_with("\tclear")
    );
    assert!(rig.shown().contains("Defeat them all."));

    // Nothing happens while they live.
    for _ in 0..30 {
        rig.tick();
    }
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));

    // A pack is not beaten until its last member falls.
    if count > 1 {
        rig.sim().units[0].hp = 0.0;
        rig.sim().units[0].dead = true;
        for _ in 0..10 {
            rig.tick();
        }
        assert!(matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Active,
                ..
            }
        ));
    }
    rig.sim().win();
    rig.settle();
    let Phase::Reward { gold, .. } = rig.phase() else {
        panic!("a won fight gives a reward")
    };
    assert_eq!(rig.d.run().unwrap().gold, gold);
    assert_eq!(rig.sim().units.len(), 0);
    assert!(rig.shown().starts_with("Spoils\n"));
}

#[test]
fn slow_fights_are_won_when_health_runs_out() {
    let mut rig = Rig::new();
    rig.sim().damage_per_step = 0.2;
    rig.reach_first_fight();
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    // Nobody calls the fight: it ends when the game reports the enemies at zero health.
    for _ in 0..40 {
        if !matches!(rig.phase(), Phase::Fight { .. }) {
            break;
        }
        rig.tick();
    }
    rig.settle();
    assert!(
        matches!(rig.phase(), Phase::Reward { .. }),
        "the fight did not end by itself: {:?}",
        rig.phase()
    );
    assert_eq!(rig.d.run().unwrap().stats.fights_won, 1);
}

#[test]
fn death_without_a_forgiven_death_ends_the_run() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    rig.press(1);
    rig.sim().kill_player();
    rig.settle();
    assert_eq!(rig.phase(), Phase::Defeat);
    assert_eq!(
        rig.sim().units.len(),
        0,
        "enemies are removed though the player is down"
    );
    assert!(
        rig.shown().starts_with("You Have Fallen\n"),
        "the panel is updated though the player is down"
    );
    assert_eq!((rig.d.profile().runs, rig.d.profile().wins), (1, 0));
    // The dead player's game keeps reporting death; it must not be counted twice.
    for _ in 0..20 {
        rig.tick();
    }
    assert_eq!(rig.d.profile().runs, 1);
    assert_eq!(rig.d.run().unwrap().stats.deaths, 1);
}

#[test]
fn a_forgiven_death_waits_for_the_player_to_stand_again() {
    let mut rig = Rig::new();
    rig.settle();
    rig.press(1);
    // Take the boon only if it is the effigy; otherwise grant a forgiven death through a merchant-free path.
    let skip = rig.d.run().unwrap().option_count(rig.d.content()) as u8;
    rig.press(skip);
    rig.press(1);
    rig.press(1);
    let first: Vec<String> = rig.spawn_lines();

    // Give the run one forgiven death by saving, editing and reloading it, as a blessing would.
    let dir = temp_dir("revive");
    let mut json: serde_json::Value =
        serde_json::from_str(&rig.d.run().unwrap().to_json()).unwrap();
    json["run"]["revives"] = serde_json::json!(1);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("run.json"), json.to_string()).unwrap();
    let mut rig = Rig::with(SimGame::new(), Store::at(dir.clone()));
    rig.settle();
    assert!(
        matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ),
        "a loaded fight waits to be begun"
    );
    assert!(rig.shown().contains("interrupted"));
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));

    rig.sim().kill_player();
    rig.settle();
    let Phase::Fight { stage, attempt, .. } = rig.phase() else {
        panic!("the fight is to be fought again")
    };
    assert_eq!((stage, attempt), (FightStage::Ready, 2));
    assert_eq!(rig.d.run().unwrap().revives, 0);
    assert_eq!(rig.sim().units.len(), 0);

    // Still down: beginning is refused, and nothing is summoned.
    let before = rig.spawn_lines().len();
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Ready,
            ..
        }
    ));
    assert!(
        rig.shown().contains("Not yet: the player is down."),
        "{}",
        rig.shown()
    );
    assert_eq!(rig.spawn_lines().len(), before);

    rig.sim().revive_player();
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    let again = rig.spawn_lines();
    assert!(again.len() > before);
    // New enemies get new handles, never those of the first attempt.
    let handle = |l: &String| l.split('\t').nth(3).unwrap().to_string();
    let old: Vec<String> = again[..before].iter().map(handle).collect();
    for line in &again[before..] {
        assert!(!old.contains(&handle(line)));
    }
    assert!(!first.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn enemies_the_game_cannot_summon_are_replaced() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    let Phase::Fight { encounter, .. } = rig.phase() else {
        unreachable!()
    };
    let doomed: Vec<String> = encounter.spawns.iter().map(|s| s.unit_id.clone()).collect();
    for id in &doomed {
        rig.sim().fail_units.insert(id.clone());
    }
    rig.press(1);
    let Phase::Fight {
        encounter: replacement,
        stage,
        rerolls,
        ..
    } = rig.phase()
    else {
        panic!("the fight is replaced, not lost: {:?}", rig.phase())
    };
    assert_eq!((stage, rerolls), (FightStage::Ready, 1));
    for spawn in &replacement.spawns {
        assert!(!doomed.contains(&spawn.unit_id));
    }
    for id in &doomed {
        assert!(rig.d.run().unwrap().banned_units.contains(id));
    }
    assert_eq!(
        rig.sim().units.len(),
        0,
        "enemies summoned beside a failed one are removed"
    );
    assert!(rig.shown().contains("could not be summoned"));
    // The replacement can be fought.
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    rig.sim().win();
    rig.settle();
    assert!(matches!(rig.phase(), Phase::Reward { .. }));
}

#[test]
fn enemies_that_vanish_win_nothing() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    rig.press(1);
    let gold = rig.d.run().unwrap().gold;
    rig.sim().vanish_all();
    rig.settle();
    assert!(
        matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ),
        "{:?}",
        rig.phase()
    );
    assert_eq!(rig.d.run().unwrap().gold, gold);
    assert_eq!(rig.d.run().unwrap().stats.fights_won, 0);
    assert!(rig.shown().contains("interrupted"));
}

#[test]
fn a_game_restart_in_a_fight_returns_to_before_it() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    rig.press(1);
    assert!(rig.sim().living() > 0);
    let hellos = |rig: &mut Rig| {
        rig.sim()
            .received
            .iter()
            .filter(|l| l.contains("\thello\t"))
            .count()
    };
    assert_eq!(hellos(&mut rig), 1);

    rig.sim().restart();
    rig.settle();
    assert_eq!(hellos(&mut rig), 2, "a new bridge session is greeted");
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Ready,
            ..
        }
    ));
    assert_eq!(
        rig.d.run().unwrap().stats.deaths,
        0,
        "a restart is not a death"
    );
    assert_eq!(
        rig.shown(),
        rig.d.panel_text(),
        "the new session gets the panel again"
    );
    rig.press(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    rig.sim().win();
    rig.settle();
    assert!(matches!(rig.phase(), Phase::Reward { .. }));
}

#[test]
fn keys_pressed_before_the_director_listened_are_not_choices() {
    let mut sim = SimGame::new();
    sim.press("1");
    sim.press("1");
    let mut rig = Rig::with(sim, Store::in_memory());
    rig.settle();
    assert!(rig.d.run().is_none(), "an old key press started a run");
    rig.press(1);
    assert!(rig.d.run().is_some());
    // The same press is not taken twice while it stays listed in the state.
    assert!(matches!(rig.phase(), Phase::Boon { .. }));
    for _ in 0..20 {
        rig.tick();
    }
    assert!(matches!(rig.phase(), Phase::Boon { .. }));
}

#[test]
fn without_the_game_the_window_still_works_and_fights_wait() {
    let mut sim = SimGame::new();
    sim.running = false;
    let mut rig = Rig::with(sim, Store::in_memory());
    for _ in 0..10 {
        rig.tick();
    }
    assert!(!rig.d.connected());
    assert!(rig.d.console_text().contains("Game: not answering"));
    assert!(rig.sim().received.is_empty());

    // The director's own window takes choices.
    rig.d.key(UserKey::Digit(1));
    let skip = rig.d.run().unwrap().option_count(rig.d.content()) as u8;
    rig.d.key(UserKey::Digit(skip));
    rig.d.key(UserKey::Digit(1));
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Ready,
            ..
        }
    ));
    rig.d.key(UserKey::Digit(1));
    assert!(
        matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ),
        "a fight cannot begin without the game"
    );
    assert!(rig
        .d
        .console_text()
        .contains("Not yet: the game is not running."));

    // The game arrives, but on a loading screen.
    rig.sim().running = true;
    rig.sim().in_map = false;
    rig.settle();
    assert!(rig.d.connected() && !rig.d.game_ready());
    assert!(rig.d.console_text().contains("waiting (no map is loaded)"));
    rig.d.key(UserKey::Digit(1));
    assert!(rig.d.console_text().contains("Not yet: no map is loaded."));
    assert!(rig.spawn_lines().is_empty());

    rig.sim().in_map = true;
    rig.settle();
    rig.d.key(UserKey::Digit(1));
    rig.settle();
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    assert!(rig.sim().living() > 0);
}

#[test]
fn a_game_that_stops_answering_is_noticed_and_picked_up_again() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    rig.sim().running = false;
    for _ in 0..29 {
        rig.tick();
    }
    assert!(rig.d.connected(), "three seconds of silence are tolerated");
    for _ in 0..3 {
        rig.tick();
    }
    assert!(!rig.d.connected());
    // Alt-tabbed back: same session, nothing lost, the panel is sent again.
    rig.sim().screen = (false, String::new());
    rig.sim().running = true;
    rig.settle();
    assert!(rig.d.connected());
    assert_eq!(rig.shown(), rig.d.panel_text());
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Ready,
            ..
        }
    ));
    assert_eq!(
        rig.sim()
            .received
            .iter()
            .filter(|l| l.contains("\thello\t"))
            .count(),
        1,
        "the same session is not greeted twice"
    );
}

/// A transport that loses commands on the way, as when the bridge misses a write.
struct Lossy {
    sim: SimGame,
    lose: u32,
    lost: u32,
}

impl Transport for Lossy {
    fn send(&mut self, line: &str) -> io::Result<()> {
        if self.lose > 0 {
            self.lose -= 1;
            self.lost += 1;
            return Ok(());
        }
        self.sim.send(line)
    }
    fn clear(&mut self) -> io::Result<()> {
        self.sim.clear()
    }
    fn read_state(&mut self) -> Option<String> {
        self.sim.read_state()
    }
}

#[test]
fn an_unanswered_command_is_sent_again_and_runs_once() {
    let mut d = Director::new(
        content(),
        Lossy {
            sim: SimGame::new(),
            lose: 2,
            lost: 0,
        },
        Store::in_memory(),
        0,
    );
    let mut now = 0;
    for _ in 0..100 {
        now += 100;
        d.tick(now);
    }
    assert_eq!(d.transport_mut().lost, 2);
    assert_eq!(
        d.pending_commands(),
        0,
        "everything was answered in the end"
    );
    let received = d.transport_mut().sim.received.clone();
    assert_eq!(
        received.iter().filter(|l| l.contains("\thello\t")).count(),
        1
    );
    assert_eq!(
        received.iter().filter(|l| l.ends_with("\tclear")).count(),
        1
    );
    let mut ids: Vec<&str> = received
        .iter()
        .map(|l| l.split('\t').nth(1).unwrap())
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "no command id was run twice");
    assert_eq!(d.transport().sim.screen.1, d.panel_text());
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pbzr-director-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn the_run_is_saved_and_resumed() {
    let dir = temp_dir("resume");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rig = Rig::with(SimGame::new(), Store::at(dir.clone()));
    rig.reach_first_fight();
    rig.press(1);
    rig.sim().win();
    rig.settle();
    rig.press(1); // take the first blessing
    let saved = rig.d.run().unwrap().clone();
    assert_eq!(saved.phase, Phase::Map);
    assert!(dir.join("run.json").exists());
    assert!(
        !dir.join("run.tmp").exists(),
        "no scratch file is left behind"
    );
    drop(rig);

    let mut rig = Rig::with(SimGame::new(), Store::at(dir.clone()));
    assert_eq!(rig.d.run(), Some(&saved));
    rig.settle();
    assert!(rig.shown().starts_with("The Road\n"));

    // A finished run leaves no save, and the profile is kept.
    rig.press(1);
    while matches!(rig.phase(), Phase::Fight { .. }) || !rig.d.run().unwrap().is_over() {
        match rig.phase() {
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            } => rig.press(1),
            Phase::Fight {
                stage: FightStage::Active,
                ..
            } => {
                rig.sim().kill_player();
                rig.settle();
            }
            _ => {
                let key = bot_choice(rig.d.run().unwrap(), rig.d.content());
                rig.press(key);
            }
        }
        rig.sim().revive_player();
    }
    assert_eq!(rig.phase(), Phase::Defeat);
    assert!(!dir.join("run.json").exists());
    drop(rig);
    let rig = Rig::with(SimGame::new(), Store::at(dir.clone()));
    assert!(rig.d.run().is_none());
    assert_eq!(rig.d.profile().runs, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_that_cannot_be_used_is_set_aside_not_deleted() {
    let dir = temp_dir("bad-save");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("run.json"), "{ not a save").unwrap();
    std::fs::write(dir.join("profile.json"), "also broken").unwrap();
    let mut rig = Rig::with(SimGame::new(), Store::at(dir.clone()));
    assert!(rig.d.run().is_none());
    assert_eq!(rig.d.profile().runs, 0);
    assert!(!dir.join("run.json").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("run.bad")).unwrap(),
        "{ not a save"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("profile.bad")).unwrap(),
        "also broken"
    );
    let events = rig.d.take_events().join("\n");
    assert!(
        events.contains("run.json was set aside") && events.contains("profile.json was set aside")
    );
    rig.settle();
    assert!(rig.shown().starts_with("PBZ Roguelike\n"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_panel_that_cannot_be_made_does_not_stop_the_run() {
    let mut sim = SimGame::new();
    sim.panel_broken = true;
    let mut d = Director::new(content(), sim, Store::in_memory(), 0);
    let mut now = 0;
    let mut go = |d: &mut Director<SimGame>| {
        for _ in 0..30 {
            now += 100;
            d.tick(now);
        }
    };
    go(&mut d);
    assert!(d.console_text().contains("The in-game panel is not shown"));
    d.key(UserKey::Digit(1));
    go(&mut d);
    let skip = d.run().unwrap().option_count(d.content()) as u8;
    d.key(UserKey::Digit(skip));
    d.key(UserKey::Digit(1));
    go(&mut d);
    d.key(UserKey::Digit(1));
    go(&mut d);
    assert!(matches!(
        d.run().unwrap().phase,
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    d.transport_mut().win();
    go(&mut d);
    assert!(matches!(d.run().unwrap().phase, Phase::Reward { .. }));
    let complaints = d
        .take_events()
        .iter()
        .filter(|e| e.contains("panel is not shown"))
        .count();
    assert_eq!(complaints, 1, "the panel problem is reported once");
}

#[test]
fn choices_that_cannot_be_made_say_why() {
    let mut rig = Rig::new();
    rig.settle();
    rig.press(7);
    assert!(rig.d.run().is_none());
    assert!(rig.shown().ends_with("There is no such option."));
    rig.press(1);
    rig.press(9);
    assert!(matches!(rig.phase(), Phase::Boon { .. }));
    assert!(rig.shown().ends_with("There is no such option."));
    // The message goes away with the next key.
    rig.press(1);
    assert!(!rig.shown().contains("There is no such option."));
}

#[test]
fn a_run_can_be_abandoned() {
    let mut rig = Rig::new();
    rig.reach_first_fight();
    rig.press(1);
    assert!(rig.sim().living() > 0);
    rig.d.key(UserKey::Abandon);
    rig.settle();
    assert!(rig.d.run().is_none());
    assert_eq!(rig.sim().units.len(), 0, "abandoning removes the enemies");
    assert!(rig.shown().starts_with("PBZ Roguelike\n"));
    assert_eq!(
        rig.d.profile().runs,
        0,
        "an abandoned run is not a recorded run"
    );
    // With no run there is nothing to abandon.
    rig.d.key(UserKey::Abandon);
    rig.settle();
    assert!(rig.d.run().is_none());
}

#[test]
fn the_probe_report_comes_back() {
    let mut rig = Rig::new();
    rig.settle();
    assert!(rig.d.probe_report().is_none());
    rig.d.key(UserKey::Probe);
    rig.settle();
    let report = rig.d.probe_report().expect("a report");
    assert_eq!(report.len(), 2);
    assert!(report.iter().all(|r| r.status == "ok"));
    let events = rig.d.take_events().join("\n");
    assert!(events.contains("probe world"));
}

#[test]
fn handles_are_never_reused_within_a_session() {
    let mut rig = Rig::new();
    rig.sim().damage_per_step = 1.0;
    rig.settle();
    rig.press(1);
    let mut fights = 0;
    let mut steps = 0;
    while fights < 6 && !rig.d.run().unwrap().is_over() {
        steps += 1;
        assert!(steps < 500);
        if matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ) {
            fights += 1;
        }
        let key = bot_choice(rig.d.run().unwrap(), rig.d.content());
        rig.press(key);
    }
    let mut handles: Vec<String> = rig
        .spawn_lines()
        .iter()
        .map(|l| l.split('\t').nth(3).unwrap().to_string())
        .collect();
    let total = handles.len();
    assert!(total >= 6);
    handles.sort();
    handles.dedup();
    assert_eq!(handles.len(), total);
}

/// The lines in tests/golden are shared with the bridge's own tests (tests/lua/test_proto.lua).
#[test]
fn commands_match_the_lines_the_bridge_is_tested_against() {
    let golden = include_str!("../../../tests/golden/commands.tsv");
    let commands = [
        (
            "d1-5",
            Command::Spawn {
                handle: "u7".into(),
                unit_id: "BP_Monster_MoTian".into(),
                level: 12,
                health_pct: 90,
                slot: 0,
                count: 1,
            },
        ),
        ("a", Command::Clear),
        ("a", Command::Heal { pct: 35 }),
        ("a", Command::Hurt { pct: 12 }),
        ("a", Command::Probe),
        (
            "s1",
            Command::Screen {
                visible: true,
                text: "Title\n 1. A\tB \\ end".into(),
            },
        ),
        (
            "h1",
            Command::Hello {
                version: "0.1.0".into(),
            },
        ),
        (
            "s2",
            Command::Screen {
                visible: false,
                text: String::new(),
            },
        ),
    ];
    let encoded: String = commands
        .iter()
        .map(|(id, c)| c.encode(id).unwrap())
        .collect();
    assert_eq!(encoded, golden);
}

#[test]
fn the_state_the_bridge_writes_parses() {
    let state = parse_state(include_str!("../../../tests/golden/state.tsv")).unwrap();
    assert_eq!(state.tick, 41);
    assert_eq!(
        (state.session.as_str(), state.ready, state.reason.as_str()),
        ("s-9", true, "ready")
    );
    assert_eq!(state.bridge_version, "0.1.0");
    let player = state.player.unwrap();
    assert_eq!(
        (player.hp, player.max_hp, player.alive),
        (812.5, 1000.0, true)
    );
    let ack = state.ack.unwrap();
    assert_eq!(
        (ack.id.as_str(), ack.ok, ack.message.as_str()),
        ("d1-5", true, "summoned")
    );
    let statuses: Vec<UnitStatus> = state.units.iter().map(|u| u.status).collect();
    assert_eq!(
        statuses,
        vec![UnitStatus::Alive, UnitStatus::Dead, UnitStatus::Gone]
    );
    assert_eq!(state.units[0].unit_id, "BP_Monster_MoTian");
    assert_eq!((state.units[2].hp, state.units[2].max_hp), (12.3, 800.0));
    assert_eq!(
        state
            .keys
            .iter()
            .map(|k| (k.seq, k.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(3, "1"), (4, "2")]
    );
    assert_eq!(state.probes[1].detail, "two\nlines");
}
