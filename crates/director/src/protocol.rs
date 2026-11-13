//! The wire format between the director and the in-game bridge.
//!
//! Two small text files in the mod's `Runtime` folder:
//!
//! * `inbox.tsv`, written by the director: one command on one line,
//!   `PBZR1 <id> <action> <args...>`, tab-separated, ending in a newline. An empty file means
//!   "nothing to do".
//! * `state.tsv`, written by the bridge several times a second: one record per line, between a
//!   `begin <tick>` line and an `end <tick> <line count>` line. A file read while it was being
//!   written fails that check and is ignored until the next read.
//!
//! The bridge runs each command id once and reports the last one it ran in an `ack` record. The Lua
//! side of this format is `mod/PBZRoguelike/Scripts/proto.lua`; `tests/lua` checks both against the
//! same lines.

pub const PROTO: &str = "PBZR1";
/// The bridge refuses longer command lines.
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;

/// Makes text safe for one tab-separated field.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // An unknown escape keeps its character; a trailing backslash is dropped.
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn is_token(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// First command of a session: tells the bridge who is driving.
    Hello { version: String },
    /// The panel text. Hidden panels keep their text.
    Screen { visible: bool, text: String },
    /// Summon one enemy. `slot` of `count` spreads a group out in front of the player.
    Spawn {
        handle: String,
        unit_id: String,
        level: u32,
        health_pct: u32,
        slot: u32,
        count: u32,
    },
    /// Remove every enemy the mod summoned.
    Clear,
    /// Heal the player by a share of maximum health.
    Heal { pct: u32 },
    /// Take a share of maximum health from the player, never the last point.
    Hurt { pct: u32 },
    /// Check the engine calls the bridge relies on and report each one.
    Probe,
}

impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Command::Hello { .. } => "hello",
            Command::Screen { .. } => "screen",
            Command::Spawn { .. } => "spawn",
            Command::Clear => "clear",
            Command::Heal { .. } => "heal",
            Command::Hurt { .. } => "hurt",
            Command::Probe => "probe",
        }
    }

    /// Commands that act on the game world and need a loaded map with a living player.
    pub fn needs_world(&self) -> bool {
        !matches!(self, Command::Hello { .. })
    }

    /// The inbox line for this command, newline included.
    pub fn encode(&self, id: &str) -> Result<String, String> {
        if !is_token(id, 64) {
            return Err(format!("bad command id {id:?}"));
        }
        let mut fields: Vec<String> =
            vec![PROTO.to_string(), id.to_string(), self.action().to_string()];
        match self {
            Command::Hello { version } => fields.push(escape(version)),
            Command::Screen { visible, text } => {
                fields.push(if *visible { "1" } else { "0" }.to_string());
                fields.push(escape(text));
            }
            Command::Spawn {
                handle,
                unit_id,
                level,
                health_pct,
                slot,
                count,
            } => {
                if !is_token(handle, 32) {
                    return Err(format!("bad handle {handle:?}"));
                }
                if !is_token(unit_id, 96) || unit_id.contains('-') {
                    return Err(format!("bad unit id {unit_id:?}"));
                }
                if !(1..=200).contains(level) || !(1..=100).contains(health_pct) {
                    return Err(format!(
                        "spawn out of range: level {level}, health {health_pct}%"
                    ));
                }
                if *count == 0 || *count > 16 || slot >= count {
                    return Err(format!("bad slot {slot} of {count}"));
                }
                fields.push(handle.clone());
                fields.push(unit_id.clone());
                fields.push(level.to_string());
                fields.push(health_pct.to_string());
                fields.push(slot.to_string());
                fields.push(count.to_string());
            }
            Command::Clear | Command::Probe => {}
            Command::Heal { pct } | Command::Hurt { pct } => {
                if !(1..=100).contains(pct) {
                    return Err(format!("{} out of range: {pct}%", self.action()));
                }
                fields.push(pct.to_string());
            }
        }
        let mut line = fields.join("\t");
        line.push('\n');
        if line.len() > MAX_COMMAND_BYTES {
            return Err(format!(
                "command is {} bytes, the limit is {MAX_COMMAND_BYTES}",
                line.len()
            ));
        }
        Ok(line)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub hp: f64,
    pub max_hp: f64,
    pub alive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ack {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitStatus {
    Alive,
    Dead,
    /// The actor no longer exists.
    Gone,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitState {
    pub handle: String,
    pub status: UnitStatus,
    pub hp: f64,
    pub max_hp: f64,
    pub unit_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub seq: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeResult {
    pub name: String,
    /// `ok`, `fail` or `skip`.
    pub status: String,
    pub detail: String,
}

/// One complete `state.tsv`.
#[derive(Clone, Debug, PartialEq)]
pub struct GameState {
    pub tick: u64,
    pub bridge_version: String,
    /// Changes whenever the bridge starts anew (game restart or mod reload).
    pub session: String,
    /// A map is loaded and the player is alive and controllable.
    pub ready: bool,
    pub reason: String,
    pub player: Option<PlayerState>,
    pub ack: Option<Ack>,
    pub units: Vec<UnitState>,
    pub keys: Vec<KeyEvent>,
    pub probes: Vec<ProbeResult>,
}

fn number(text: &str) -> Option<f64> {
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

/// Parses `state.tsv`. Errors mean "not a complete state"; the caller reads again later.
pub fn parse_state(text: &str) -> Result<GameState, String> {
    if !text.ends_with('\n') {
        return Err("state has no final newline".into());
    }
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if lines.len() < 2 {
        return Err("state is too short".into());
    }
    let first: Vec<&str> = lines[0].split('\t').collect();
    let last: Vec<&str> = lines[lines.len() - 1].split('\t').collect();
    if first.len() != 2 || first[0] != "begin" {
        return Err("state does not start with begin".into());
    }
    if last.len() != 3 || last[0] != "end" {
        return Err("state does not finish with end".into());
    }
    let tick: u64 = first[1].parse().map_err(|_| "bad tick".to_string())?;
    if last[1] != first[1] {
        return Err("begin and end ticks differ".into());
    }
    let declared: usize = last[2].parse().map_err(|_| "bad line count".to_string())?;
    if declared != lines.len() {
        return Err(format!(
            "state declares {declared} lines and has {}",
            lines.len()
        ));
    }

    let mut state = GameState {
        tick,
        bridge_version: String::new(),
        session: String::new(),
        ready: false,
        reason: String::new(),
        player: None,
        ack: None,
        units: Vec::new(),
        keys: Vec::new(),
        probes: Vec::new(),
    };
    let mut proto_seen = false;
    for line in &lines[1..lines.len() - 1] {
        let f: Vec<&str> = line.split('\t').collect();
        match (f[0], f.len()) {
            ("proto", 3) => {
                if f[1] != "1" {
                    return Err(format!(
                        "the bridge speaks protocol {}, this director speaks 1",
                        f[1]
                    ));
                }
                proto_seen = true;
                state.bridge_version = unescape(f[2]);
            }
            ("session", 2) => state.session = f[1].to_string(),
            ("ready", 3) => {
                state.ready = f[1] == "1";
                state.reason = unescape(f[2]);
            }
            ("player", 4) => {
                let (Some(hp), Some(max_hp)) = (number(f[1]), number(f[2])) else {
                    return Err("bad player record".into());
                };
                state.player = Some(PlayerState {
                    hp,
                    max_hp,
                    alive: f[3] == "1",
                });
            }
            ("ack", 4) => {
                state.ack = Some(Ack {
                    id: f[1].to_string(),
                    ok: f[2] == "ok",
                    message: unescape(f[3]),
                });
            }
            ("unit", 6) => {
                let status = match f[2] {
                    "alive" => UnitStatus::Alive,
                    "dead" => UnitStatus::Dead,
                    "gone" => UnitStatus::Gone,
                    other => return Err(format!("unknown unit status {other:?}")),
                };
                let (Some(hp), Some(max_hp)) = (number(f[3]), number(f[4])) else {
                    return Err("bad unit record".into());
                };
                state.units.push(UnitState {
                    handle: f[1].to_string(),
                    status,
                    hp,
                    max_hp,
                    unit_id: f[5].to_string(),
                });
            }
            ("key", 3) => {
                let seq: u64 = f[1].parse().map_err(|_| "bad key sequence".to_string())?;
                state.keys.push(KeyEvent {
                    seq,
                    name: f[2].to_string(),
                });
            }
            ("probe", 4) => state.probes.push(ProbeResult {
                name: f[1].to_string(),
                status: f[2].to_string(),
                detail: unescape(f[3]),
            }),
            // Records this director does not know are skipped, so a newer bridge still works.
            _ => {}
        }
    }
    if !proto_seen {
        return Err("state has no proto record".into());
    }
    if state.session.is_empty() {
        return Err("state has no session".into());
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_round_trips_awkward_text() {
        for text in [
            "",
            "plain",
            "tab\there",
            "two\nlines\r\n",
            "back\\slash and \\n literal",
            "中文 名字 — dash",
            "\\",
            "\\\\t",
        ] {
            let escaped = escape(text);
            assert!(!escaped.contains('\t') && !escaped.contains('\n') && !escaped.contains('\r'));
            assert_eq!(unescape(&escaped), text, "round trip of {text:?}");
        }
    }

    #[test]
    fn commands_encode_to_one_line() {
        let line = Command::Spawn {
            handle: "u7".into(),
            unit_id: "BP_Monster_MoTian".into(),
            level: 12,
            health_pct: 90,
            slot: 0,
            count: 1,
        }
        .encode("d1-5")
        .unwrap();
        assert_eq!(
            line,
            "PBZR1\td1-5\tspawn\tu7\tBP_Monster_MoTian\t12\t90\t0\t1\n"
        );
        assert_eq!(Command::Clear.encode("a").unwrap(), "PBZR1\ta\tclear\n");
        assert_eq!(
            Command::Heal { pct: 35 }.encode("a").unwrap(),
            "PBZR1\ta\theal\t35\n"
        );
        assert_eq!(
            Command::Hurt { pct: 12 }.encode("a").unwrap(),
            "PBZR1\ta\thurt\t12\n"
        );
        assert_eq!(Command::Probe.encode("a").unwrap(), "PBZR1\ta\tprobe\n");
        let screen = Command::Screen {
            visible: true,
            text: "Title\n 1. A\tB".into(),
        }
        .encode("s1")
        .unwrap();
        assert_eq!(screen, "PBZR1\ts1\tscreen\t1\tTitle\\n 1. A\\tB\n");
        assert_eq!(screen.matches('\n').count(), 1);
    }

    #[test]
    fn bad_commands_are_refused() {
        assert!(Command::Clear.encode("has space").is_err());
        assert!(Command::Clear.encode("").is_err());
        assert!(Command::Heal { pct: 0 }.encode("a").is_err());
        assert!(Command::Hurt { pct: 101 }.encode("a").is_err());
        let spawn = |unit_id: &str, level, health_pct, slot, count| Command::Spawn {
            handle: "u1".into(),
            unit_id: unit_id.into(),
            level,
            health_pct,
            slot,
            count,
        };
        assert!(spawn("BP_Monster_X", 5, 100, 0, 1).encode("a").is_ok());
        assert!(spawn("../etc", 5, 100, 0, 1).encode("a").is_err());
        assert!(spawn("BP Monster", 5, 100, 0, 1).encode("a").is_err());
        assert!(spawn("BP_Monster_X", 0, 100, 0, 1).encode("a").is_err());
        assert!(spawn("BP_Monster_X", 5, 0, 0, 1).encode("a").is_err());
        assert!(spawn("BP_Monster_X", 5, 100, 1, 1).encode("a").is_err());
        assert!(spawn("BP_Monster_X", 5, 100, 0, 0).encode("a").is_err());
        let huge = Command::Screen {
            visible: true,
            text: "x".repeat(MAX_COMMAND_BYTES),
        };
        assert!(huge.encode("a").is_err());
    }

    const GOOD: &str = "begin\t41\nproto\t1\t0.1.0\nsession\ts-9\nready\t1\tok\nplayer\t812.5\t1000.0\t1\n\
ack\td1-5\tok\tspawned\nunit\tu7\talive\t5000.0\t9000.0\tBP_Monster_MoTian\nunit\tu8\tdead\t0.0\t800.0\tBP_Monster_Dog\n\
key\t3\t1\nkey\t4\ttoggle\nprobe\tworld\tok\tfound\nend\t41\t12\n";

    #[test]
    fn a_complete_state_parses() {
        let s = parse_state(GOOD).unwrap();
        assert_eq!(s.tick, 41);
        assert_eq!(s.bridge_version, "0.1.0");
        assert_eq!(s.session, "s-9");
        assert!(s.ready);
        assert_eq!(
            s.player,
            Some(PlayerState {
                hp: 812.5,
                max_hp: 1000.0,
                alive: true
            })
        );
        assert_eq!(
            s.ack,
            Some(Ack {
                id: "d1-5".into(),
                ok: true,
                message: "spawned".into()
            })
        );
        assert_eq!(s.units.len(), 2);
        assert_eq!(s.units[0].status, UnitStatus::Alive);
        assert_eq!(s.units[1].status, UnitStatus::Dead);
        assert_eq!(
            s.keys,
            vec![
                KeyEvent {
                    seq: 3,
                    name: "1".into()
                },
                KeyEvent {
                    seq: 4,
                    name: "toggle".into()
                }
            ]
        );
        assert_eq!(s.probes.len(), 1);
    }

    #[test]
    fn a_state_cut_anywhere_is_refused() {
        // Whatever prefix a reader catches while the bridge is writing, it must not parse.
        for cut in 0..GOOD.len() {
            assert!(
                parse_state(&GOOD[..cut]).is_err(),
                "a state cut at byte {cut} parsed"
            );
        }
        assert!(parse_state(GOOD).is_ok());
    }

    #[test]
    fn a_state_with_mismatched_frame_is_refused() {
        assert!(parse_state(&GOOD.replace("end\t41\t12", "end\t40\t12")).is_err());
        assert!(parse_state(&GOOD.replace("end\t41\t12", "end\t41\t13")).is_err());
        assert!(parse_state(&GOOD.replace("proto\t1\t", "proto\t2\t")).is_err());
        assert!(parse_state(
            &GOOD
                .replace("session\ts-9\n", "")
                .replace("end\t41\t12", "end\t41\t11")
        )
        .is_err());
        assert!(parse_state(&GOOD.replace("812.5", "nan")).is_err());
        assert!(parse_state(&GOOD.replace("\talive\t", "\tasleep\t")).is_err());
    }

    #[test]
    fn unknown_records_and_windows_line_ends_are_tolerated() {
        let newer = GOOD
            .replace("ready\t1\tok\n", "ready\t1\tok\nweather\train\n")
            .replace("end\t41\t12", "end\t41\t13");
        assert!(parse_state(&newer).is_ok());
        let crlf = GOOD.replace('\n', "\r\n");
        let s = parse_state(&crlf).unwrap();
        assert_eq!(s.session, "s-9");
        assert_eq!(s.units[1].unit_id, "BP_Monster_Dog");
    }
}
