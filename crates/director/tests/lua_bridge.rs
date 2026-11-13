//! End to end: this director against the real bridge scripts (mod/PBZRoguelike/Scripts), run by a
//! Lua interpreter over a mock of the engine (tests/lua/mock_ue.lua), talking through real files.
//!
//! This proves the two sides agree on the protocol and that the bridge's own logic holds. The mock
//! engine is this project's reading of the game, so it cannot prove the game itself behaves so.
//!
//! The tests need a Lua interpreter: PBZR_LUA, or lua5.4 / lua54 / lua / luajit on the PATH. Without
//! one they are skipped with a notice, unless PBZR_REQUIRE_LUA is set (CI sets it).

use pbz_rogue_core::content::{Content, Pool};
use pbz_rogue_core::embedded;
use pbz_rogue_core::run::{FightStage, Phase, Run};
use pbz_rogue_director::director::{Director, Store, UserKey};
use pbz_rogue_director::transport::FileTransport;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn lua() -> Option<String> {
    let mut candidates: Vec<String> = std::env::var("PBZR_LUA").into_iter().collect();
    candidates.extend(["lua5.4", "lua54", "lua", "luajit"].map(String::from));
    let found = candidates.into_iter().find(|name| {
        Command::new(name)
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("PBZR_REQUIRE_LUA").is_none(),
            "PBZR_REQUIRE_LUA is set but no Lua interpreter was found"
        );
        eprintln!(
            "SKIPPED: no Lua interpreter found (set PBZR_LUA); the bridge scripts were not run"
        );
    }
    found
}

struct LuaGame {
    child: Child,
    dir: PathBuf,
    control_seq: u32,
}

impl LuaGame {
    fn start(lua: &str, name: &str, content: &Content) -> LuaGame {
        let dir = std::env::temp_dir().join(format!("pbzr-e2e-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Scripts")).unwrap();
        fs::create_dir_all(dir.join("Runtime")).unwrap();
        for entry in fs::read_dir(root().join("mod/PBZRoguelike/Scripts")).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, dir.join("Scripts").join(path.file_name().unwrap())).unwrap();
        }
        let units: Vec<&str> = content
            .units
            .iter()
            .filter(|u| u.pool != Pool::Excluded)
            .map(|u| u.id.as_str())
            .collect();
        fs::write(dir.join("units.txt"), units.join("\n")).unwrap();
        let child = Command::new(lua)
            .arg(root().join("tests/lua/mock_game.lua"))
            .arg(root())
            .arg(&dir)
            .stdout(fs::File::create(dir.join("stdout.txt")).unwrap())
            .stderr(fs::File::create(dir.join("stderr.txt")).unwrap())
            .spawn()
            .expect("the Lua interpreter starts");
        LuaGame {
            child,
            dir,
            control_seq: 0,
        }
    }

    fn runtime(&self) -> PathBuf {
        self.dir.join("Runtime")
    }

    fn control(&mut self, word: &str) {
        self.control_seq += 1;
        let scratch = self.runtime().join("control.tmp");
        fs::write(&scratch, format!("{} {word}\n", self.control_seq)).unwrap();
        fs::rename(scratch, self.runtime().join("control.txt")).unwrap();
    }

    /// Fails with the mock game's own output if it died or logged a failure.
    fn check(&mut self) {
        let stderr = fs::read_to_string(self.dir.join("stderr.txt")).unwrap_or_default();
        if let Some(status) = self.child.try_wait().unwrap() {
            panic!("the mock game exited ({status}):\n{stderr}");
        }
        let log = fs::read_to_string(self.runtime().join("bridge.log")).unwrap_or_default();
        assert!(
            !log.contains("step failed"),
            "the bridge logged a failure:\n{log}"
        );
        assert!(
            !log.contains("failed:"),
            "a command failed in the bridge:\n{log}"
        );
    }
}

impl Drop for LuaGame {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.dir);
        } else {
            eprintln!("work folder kept for inspection: {}", self.dir.display());
        }
    }
}

struct Rig {
    game: LuaGame,
    d: Director<FileTransport>,
    started: Instant,
}

impl Rig {
    fn start(name: &str) -> Option<Rig> {
        let lua = lua()?;
        let content = embedded::content().unwrap();
        let game = LuaGame::start(&lua, name, &content);
        let transport = FileTransport::new(&game.runtime()).unwrap();
        let mut d = Director::new(content, transport, Store::in_memory(), 1);
        d.fixed_seed = Some(2026);
        Some(Rig {
            game,
            d,
            started: Instant::now(),
        })
    }

    fn tick(&mut self) {
        std::thread::sleep(Duration::from_millis(2));
        self.d.tick(1 + self.started.elapsed().as_millis() as u64);
    }

    /// Ticks until `done` holds, or fails after `seconds`.
    fn until(
        &mut self,
        what: &str,
        seconds: u64,
        mut done: impl FnMut(&Director<FileTransport>) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            self.tick();
            if done(&self.d) {
                return;
            }
            if Instant::now() > deadline {
                self.game.check();
                let state =
                    fs::read_to_string(self.game.runtime().join("state.tsv")).unwrap_or_default();
                panic!(
                    "timed out waiting for: {what}\nscreen:\n{}\nstate:\n{state}",
                    self.d.console_text()
                );
            }
        }
    }

    fn settle(&mut self) {
        self.until("the director to settle", 20, |d| {
            d.connected() && d.pending_commands() == 0
        });
    }

    fn phase(&self) -> Phase {
        self.d.run().expect("a run").phase.clone()
    }

    /// A number key pressed in the game, read by the bridge's own key code.
    fn press_in_game(&mut self, digit: u8) {
        let names = [
            "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
        ];
        let before = self.d.run().cloned();
        self.game
            .control(&format!("press {}", names[usize::from(digit) - 1]));
        self.until("the in-game key press to arrive", 20, |d| {
            d.run().cloned() != before
        });
        self.settle();
    }

    fn choose(&mut self, digit: u8) {
        self.d.key(UserKey::Digit(digit));
        self.settle();
    }

    fn reach_first_fight(&mut self) {
        self.settle();
        self.press_in_game(1);
        let skip = self.d.run().unwrap().option_count(self.d.content()) as u8;
        self.choose(skip);
        self.choose(1);
        assert!(matches!(
            self.phase(),
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ));
    }

    fn state(&self) -> String {
        fs::read_to_string(self.game.runtime().join("state.tsv")).unwrap_or_default()
    }
}

fn bot_choice(run: &Run, content: &Content) -> u8 {
    let count = run.option_count(content) as u8;
    match &run.phase {
        Phase::Shop { .. } | Phase::Event { result: None, .. } => count,
        _ => 1,
    }
}

#[test]
fn the_bridge_unit_tests_pass() {
    let Some(lua) = lua() else { return };
    let output = Command::new(&lua)
        .arg(root().join("tests/lua/run_tests.lua"))
        .arg(root())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains(" 0 failed"), "{text}");
}

#[test]
fn a_whole_run_through_the_real_bridge() {
    let Some(mut rig) = Rig::start("run") else {
        return;
    };
    rig.settle();
    assert!(rig.d.game_ready());
    rig.press_in_game(1); // begin a run with a key read by the bridge
    let mut fights = 0;
    let mut steps = 0;
    while !rig.d.run().unwrap().is_over() {
        steps += 1;
        assert!(steps < 3000, "the run does not end");
        match rig.phase() {
            Phase::Fight {
                stage: FightStage::Active,
                ..
            } => {
                rig.until("the fight to end", 30, |d| {
                    !matches!(
                        d.run().unwrap().phase,
                        Phase::Fight {
                            stage: FightStage::Active,
                            ..
                        }
                    )
                });
                rig.settle();
                assert!(
                    !matches!(rig.phase(), Phase::Fight { .. }),
                    "the fight should be won: {:?}",
                    rig.phase()
                );
            }
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            } => {
                fights += 1;
                // Every fifth fight is begun with a real key press; the rest through the window.
                if fights % 5 == 1 {
                    rig.press_in_game(1);
                } else {
                    rig.choose(1);
                }
            }
            _ => {
                let key = bot_choice(rig.d.run().unwrap(), rig.d.content());
                rig.choose(key);
            }
        }
    }
    assert_eq!(rig.phase(), Phase::Victory);
    let stats = rig.d.run().unwrap().stats.clone();
    assert_eq!((stats.lords_won, stats.deaths), (3, 0));
    assert!(stats.fights_won >= 20, "only {} fights", stats.fights_won);
    assert_eq!((rig.d.profile().runs, rig.d.profile().wins), (1, 1));
    rig.settle();
    assert!(
        !rig.state().contains("\nunit\t"),
        "enemies remain after the run:\n{}",
        rig.state()
    );
    rig.game.check();
}

#[test]
fn death_and_a_restart_through_the_real_bridge() {
    let Some(mut rig) = Rig::start("death") else {
        return;
    };
    rig.game.control("damage 0");
    rig.reach_first_fight();
    rig.choose(1);
    assert!(matches!(
        rig.phase(),
        Phase::Fight {
            stage: FightStage::Active,
            ..
        }
    ));
    rig.until("the enemies to be reported alive", 20, |_| true);
    assert!(rig.state().contains("\talive\t"), "{}", rig.state());

    // The game restarts in the middle of the fight: a new bridge session, the enemies gone.
    rig.game.control("restart");
    rig.until("the fight to return to before it began", 20, |d| {
        matches!(
            d.run().unwrap().phase,
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        )
    });
    rig.settle();
    assert_eq!(rig.d.run().unwrap().stats.deaths, 0);
    rig.game.control("damage 0");

    // Begun again, and this time the player falls: with no forgiven death the run is over.
    rig.choose(1);
    assert!(
        matches!(
            rig.phase(),
            Phase::Fight {
                stage: FightStage::Active,
                ..
            }
        ),
        "{:?}",
        rig.phase()
    );
    rig.game.control("kill");
    rig.until("the run to end in defeat", 20, |d| {
        d.run().unwrap().phase == Phase::Defeat
    });
    rig.settle();
    assert!(
        !rig.state().contains("\talive\t"),
        "enemies are removed after a defeat:\n{}",
        rig.state()
    );
    assert!(rig.state().contains("\nready\t0\tthe player is down\n"));
    assert_eq!(rig.d.profile().runs, 1);
    let stderr = fs::read_to_string(rig.game.dir.join("stderr.txt")).unwrap_or_default();
    assert!(stderr.is_empty(), "the mock game wrote errors:\n{stderr}");
}

#[test]
fn vanished_enemies_and_the_panel_through_the_real_bridge() {
    let Some(mut rig) = Rig::start("vanish") else {
        return;
    };
    rig.game.control("damage 0");
    rig.reach_first_fight();
    // The panel text reached the bridge unchanged, line breaks and all.
    let log = fs::read_to_string(rig.game.runtime().join("bridge.log")).unwrap_or_default();
    assert!(log.contains("bridge 0.1.0 started"), "{log}");
    rig.choose(1);
    rig.until("the enemies to be reported alive", 20, |_| true);
    let gold = rig.d.run().unwrap().gold;
    rig.game.control("vanish");
    rig.until("the fight to be interrupted", 20, |d| {
        matches!(
            d.run().unwrap().phase,
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        )
    });
    assert_eq!(
        rig.d.run().unwrap().gold,
        gold,
        "vanished enemies pay nothing"
    );
    assert_eq!(rig.d.run().unwrap().stats.fights_won, 0);
    assert!(rig.d.panel_text().contains("interrupted"));
    // Fought properly, it is won.
    rig.game.control("damage 0.5");
    rig.settle();
    rig.choose(1);
    rig.until("the fight to be won", 30, |d| {
        matches!(d.run().unwrap().phase, Phase::Reward { .. })
    });
    rig.game.check();
}
