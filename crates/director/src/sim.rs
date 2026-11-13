//! A pretend game for trying the mod without Phantom Blade Zero, and for tests.
//!
//! `SimGame` answers the director the way the in-game bridge does: it runs each command id once,
//! reports the player, the summoned enemies and key presses, and frames its state the same way. It
//! is this crate's own reading of the bridge; `tests/lua_bridge.rs` runs the real bridge scripts.

use crate::protocol::{escape, unescape, PROTO};
use crate::transport::Transport;
use std::collections::HashSet;
use std::io;

#[derive(Clone, Debug)]
pub struct SimUnit {
    pub handle: String,
    pub unit_id: String,
    pub level: u32,
    pub hp: f64,
    pub max_hp: f64,
    pub dead: bool,
    pub gone: bool,
}

pub struct SimGame {
    inbox: String,
    last_id: Option<String>,
    ack: Option<(String, bool, String)>,
    tick: u64,
    sessions: u32,
    session: String,
    keys: Vec<(u64, String)>,
    key_seq: u64,
    probes: Vec<(String, String, String)>,

    /// The game is running and its bridge is writing state.
    pub running: bool,
    /// A map is loaded.
    pub in_map: bool,
    pub player_hp: f64,
    pub player_max: f64,
    pub units: Vec<SimUnit>,
    /// The panel as last set: shown, and its text.
    pub screen: (bool, String),
    /// Every command line received, for tests.
    pub received: Vec<String>,
    /// Units the game cannot summon.
    pub fail_units: HashSet<String>,
    /// Each living enemy loses this share of its health per state read. 0 leaves fights to `win`.
    pub damage_per_step: f64,
    /// The panel cannot be made.
    pub panel_broken: bool,
}

impl Default for SimGame {
    fn default() -> Self {
        SimGame::new()
    }
}

impl SimGame {
    pub fn new() -> SimGame {
        SimGame {
            inbox: String::new(),
            last_id: None,
            ack: None,
            tick: 0,
            sessions: 1,
            session: "sim-1".to_string(),
            keys: Vec::new(),
            key_seq: 0,
            probes: Vec::new(),
            running: true,
            in_map: true,
            player_hp: 1000.0,
            player_max: 1000.0,
            units: Vec::new(),
            screen: (false, String::new()),
            received: Vec::new(),
            fail_units: HashSet::new(),
            damage_per_step: 0.0,
            panel_broken: false,
        }
    }

    /// A key press in the game.
    pub fn press(&mut self, name: &str) {
        self.key_seq += 1;
        self.keys.push((self.key_seq, name.to_string()));
        if self.keys.len() > 16 {
            self.keys.remove(0);
        }
    }

    /// The game restarted or its mods were reloaded: a new bridge, and every summoned enemy is gone.
    pub fn restart(&mut self) {
        self.sessions += 1;
        self.session = format!("sim-{}", self.sessions);
        self.units.clear();
        self.keys.clear();
        self.key_seq = 0;
        self.ack = None;
        self.tick = 0;
        self.screen = (false, String::new());
        self.player_hp = self.player_max;
        // A real bridge remembers the last id it answered through its state file.
    }

    /// Kills every enemy in the world.
    pub fn win(&mut self) {
        for unit in self.units.iter_mut().filter(|u| !u.gone) {
            unit.hp = 0.0;
            unit.dead = true;
        }
    }

    pub fn kill_player(&mut self) {
        self.player_hp = 0.0;
    }

    pub fn revive_player(&mut self) {
        self.player_hp = self.player_max;
    }

    /// Every enemy disappears without dying, as on a map change.
    pub fn vanish_all(&mut self) {
        for unit in self.units.iter_mut().filter(|u| !u.dead) {
            unit.gone = true;
        }
    }

    pub fn living(&self) -> usize {
        self.units.iter().filter(|u| !u.dead && !u.gone).count()
    }

    fn ready(&self) -> (bool, &'static str) {
        if !self.in_map {
            (false, "no map is loaded")
        } else if self.player_hp <= 0.0 {
            (false, "the player is down")
        } else {
            (true, "ready")
        }
    }

    fn run(&mut self, fields: &[&str]) -> (bool, String) {
        let int = |i: usize| fields.get(i).and_then(|f| f.parse::<u32>().ok());
        match fields.get(2).copied() {
            Some("hello") => (true, "sim".to_string()),
            Some("screen") => {
                if self.panel_broken {
                    return (false, "the panel could not be made: no widget".to_string());
                }
                self.screen = (
                    fields.get(3) == Some(&"1"),
                    unescape(fields.get(4).copied().unwrap_or("")),
                );
                (true, "ok".to_string())
            }
            Some("spawn") => {
                let (Some(handle), Some(unit_id), Some(level), Some(pct)) =
                    (fields.get(3), fields.get(4), int(5), int(6))
                else {
                    return (false, "bad spawn".to_string());
                };
                if self.units.iter().any(|u| &u.handle == handle) {
                    return (true, "already summoned".to_string());
                }
                if !self.ready().0 {
                    return (false, "no map or no player character".to_string());
                }
                if self.fail_units.contains(*unit_id) {
                    return (false, format!("{unit_id} is not in the game codex"));
                }
                let max_hp = 1000.0 * f64::from(level + 1);
                self.units.push(SimUnit {
                    handle: handle.to_string(),
                    unit_id: unit_id.to_string(),
                    level,
                    hp: max_hp * f64::from(pct) / 100.0,
                    max_hp,
                    dead: false,
                    gone: false,
                });
                (true, "summoned".to_string())
            }
            Some("clear") => {
                let n = self.units.len();
                self.units.clear();
                (true, format!("{n} removed"))
            }
            Some("heal") => {
                if !self.ready().0 {
                    return (false, "the player is down".to_string());
                }
                let pct = f64::from(int(3).unwrap_or(0));
                self.player_hp =
                    (self.player_hp + self.player_max * pct / 100.0).min(self.player_max);
                (true, "ok".to_string())
            }
            Some("hurt") => {
                if !self.ready().0 {
                    return (false, "the player is down".to_string());
                }
                let pct = f64::from(int(3).unwrap_or(0));
                self.player_hp = (self.player_hp - self.player_max * pct / 100.0).max(1.0);
                (true, "ok".to_string())
            }
            Some("probe") => {
                self.probes = vec![
                    (
                        "world".to_string(),
                        "ok".to_string(),
                        "simulated".to_string(),
                    ),
                    (
                        "codex".to_string(),
                        "ok".to_string(),
                        "simulated".to_string(),
                    ),
                ];
                (true, "2 checks".to_string())
            }
            _ => (false, "unknown action".to_string()),
        }
    }

    fn step(&mut self) {
        self.tick += 1;
        if self.inbox.ends_with('\n') {
            let line = self.inbox.trim_end_matches(['\n', '\r']).to_string();
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() >= 3 && fields[0] == PROTO && self.last_id.as_deref() != Some(fields[1])
            {
                self.last_id = Some(fields[1].to_string());
                self.received.push(line.clone());
                let (ok, message) = self.run(&fields);
                self.ack = Some((fields[1].to_string(), ok, message));
            }
        }
        if self.damage_per_step > 0.0 && self.player_hp > 0.0 {
            for unit in self.units.iter_mut().filter(|u| !u.dead && !u.gone) {
                unit.hp = (unit.hp - unit.max_hp * self.damage_per_step).max(0.0);
                unit.dead = unit.hp <= 0.0;
            }
        }
    }

    fn state_text(&self) -> String {
        let (ready, reason) = self.ready();
        let mut lines = vec![
            format!("begin\t{}", self.tick),
            "proto\t1\tsim".to_string(),
            format!("session\t{}", self.session),
            format!("ready\t{}\t{}", u8::from(ready), escape(reason)),
        ];
        if self.in_map {
            lines.push(format!(
                "player\t{:.1}\t{:.1}\t{}",
                self.player_hp,
                self.player_max,
                u8::from(self.player_hp > 0.0)
            ));
        }
        if let Some((id, ok, message)) = &self.ack {
            lines.push(format!(
                "ack\t{id}\t{}\t{}",
                if *ok { "ok" } else { "error" },
                escape(message)
            ));
        }
        for unit in &self.units {
            let status = if unit.dead {
                "dead"
            } else if unit.gone {
                "gone"
            } else {
                "alive"
            };
            lines.push(format!(
                "unit\t{}\t{status}\t{:.1}\t{:.1}\t{}",
                unit.handle, unit.hp, unit.max_hp, unit.unit_id
            ));
        }
        for (seq, name) in &self.keys {
            lines.push(format!("key\t{seq}\t{name}"));
        }
        for (name, status, detail) in &self.probes {
            lines.push(format!("probe\t{name}\t{status}\t{}", escape(detail)));
        }
        lines.push(format!("end\t{}\t{}", self.tick, lines.len() + 1));
        lines.join("\n") + "\n"
    }
}

impl Transport for SimGame {
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.inbox = line.to_string();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inbox.clear();
        Ok(())
    }

    fn read_state(&mut self) -> Option<String> {
        if !self.running {
            return None;
        }
        self.step();
        Some(self.state_text())
    }
}
