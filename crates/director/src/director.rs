//! The director: owns the run and turns it into commands for the game.
//!
//! `tick` is called about ten times a second. It reads what the bridge sees, feeds key presses and
//! fight results to the run, and sends the game one command at a time, each acknowledged before the
//! next. The director never assumes the game did something: a fight starts when every summon is
//! acknowledged and ends on what the bridge reports.

use crate::protocol::{parse_state, Command, GameState, ProbeResult, UnitStatus};
use crate::transport::Transport;
use pbz_rogue_core::content::Content;
use pbz_rogue_core::profile::Profile;
use pbz_rogue_core::rng::Rng;
use pbz_rogue_core::run::{FightStage, GameOp, Input, Phase, Run};
use pbz_rogue_core::view::view;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Without a new state from the bridge for this long, the game counts as away.
const STALE_MS: u64 = 3000;
/// An unanswered command is written again after this long (the bridge runs each id once).
const RESEND_MS: u64 = 3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserKey {
    /// Option 1 to 9 of the current screen.
    Digit(u8),
    /// Give up the run in progress.
    Abandon,
    /// Ask the bridge to check the engine calls it relies on.
    Probe,
}

/// Where the run and the profile are kept. `None` keeps them in memory only.
pub struct Store {
    dir: Option<PathBuf>,
}

impl Store {
    pub fn in_memory() -> Store {
        Store { dir: None }
    }

    pub fn at(dir: PathBuf) -> Store {
        Store { dir: Some(dir) }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }

    fn write(&self, name: &str, text: &str) -> Result<(), String> {
        let Some(path) = self.path(name) else {
            return Ok(());
        };
        // Write beside the file and rename, so a crash never leaves half a save.
        let scratch = path.with_extension("tmp");
        fs::write(&scratch, text)
            .and_then(|()| fs::rename(&scratch, &path))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    fn read(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.path(name)?).ok()
    }

    /// Moves a file that cannot be used out of the way instead of deleting it.
    fn set_aside(&self, name: &str) {
        if let Some(path) = self.path(name) {
            let _ = fs::rename(&path, path.with_extension("bad"));
        }
    }

    fn remove(&self, name: &str) {
        if let Some(path) = self.path(name) {
            let _ = fs::remove_file(path);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Summon {
    /// The spawn command is queued or unanswered.
    Pending,
    Summoned,
    Failed,
}

#[derive(Clone, Debug)]
struct Watched {
    handle: String,
    unit_id: String,
    summon: Summon,
}

struct Flight {
    id: String,
    line: String,
    command: Command,
    sent_ms: u64,
}

pub struct Director<T: Transport> {
    content: Content,
    transport: T,
    store: Store,
    run: Option<Run>,
    profile: Profile,
    /// The finished run on screen unlocked a new heat.
    unlocked_heat: bool,

    state: Option<GameState>,
    session: Option<String>,
    last_tick: Option<u64>,
    last_change_ms: u64,
    connected: bool,
    last_key_seq: u64,

    queue: VecDeque<Command>,
    flight: Option<Flight>,
    next_id: u64,
    id_prefix: String,
    next_handle: u64,
    /// The enemies of the fight in progress.
    watch: Vec<Watched>,

    /// The panel text last handed to the queue.
    screen_sent: Option<String>,
    /// One line answering the last key press ("not enough gold").
    flash: Option<String>,
    panel_problem: Option<String>,
    probe: Option<Vec<ProbeResult>>,
    events: Vec<String>,
    entropy: u64,
    /// Tests fix the seed of the next run here.
    pub fixed_seed: Option<u64>,
}

impl<T: Transport> Director<T> {
    pub fn new(content: Content, transport: T, store: Store, start_ms: u64) -> Director<T> {
        let mut events = Vec::new();
        let profile = match store.read("profile.json") {
            None => Profile::default(),
            Some(text) => Profile::from_json(&content, &text).unwrap_or_else(|why| {
                events.push(format!("profile.json was set aside: {why}"));
                store.set_aside("profile.json");
                Profile::default()
            }),
        };
        let mut run = match store.read("run.json") {
            None => None,
            Some(text) => match Run::from_json(&content, &text) {
                Ok(run) => {
                    events.push("the run in progress was loaded".to_string());
                    Some(run)
                }
                Err(why) => {
                    events.push(format!("run.json was set aside: {why}"));
                    store.set_aside("run.json");
                    None
                }
            },
        };
        // Whatever was summoned when the director last stopped is not tracked any more.
        if let Some(run) = run.as_mut() {
            let _ = run.fight_interrupted();
        }
        Director {
            content,
            transport,
            store,
            run,
            profile,
            unlocked_heat: false,
            state: None,
            session: None,
            last_tick: None,
            last_change_ms: start_ms,
            connected: false,
            last_key_seq: 0,
            queue: VecDeque::new(),
            flight: None,
            next_id: 0,
            id_prefix: format!("d{start_ms:x}"),
            next_handle: 0,
            watch: Vec::new(),
            screen_sent: None,
            flash: None,
            panel_problem: None,
            probe: None,
            events,
            entropy: start_ms ^ 0x5DEE_CE66_D1CE_4E5B,
            fixed_seed: None,
        }
    }

    pub fn run(&self) -> Option<&Run> {
        self.run.as_ref()
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// The bridge is answering.
    pub fn connected(&self) -> bool {
        self.connected
    }

    /// A map is loaded and the player is alive.
    pub fn game_ready(&self) -> bool {
        self.connected && self.state.as_ref().is_some_and(|s| s.ready)
    }

    pub fn probe_report(&self) -> Option<&[ProbeResult]> {
        self.probe.as_deref()
    }

    /// Commands waiting or unanswered.
    pub fn pending_commands(&self) -> usize {
        self.queue.len() + usize::from(self.flight.is_some())
    }

    /// Log lines since the last call.
    pub fn take_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
    }

    pub fn tick(&mut self, now_ms: u64) {
        self.entropy = self.entropy.rotate_left(13) ^ now_ms.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.read_link(now_ms);
        self.take_answer(now_ms);
        self.take_keys();
        self.watch_fight();
        self.refresh_screen();
        self.pump(now_ms);
    }

    fn read_link(&mut self, now_ms: u64) {
        // A state caught half-written does not parse; the last good one stands until the next read.
        if let Some(state) = self
            .transport
            .read_state()
            .and_then(|text| parse_state(&text).ok())
        {
            if self.last_tick != Some(state.tick)
                || self.session.as_deref() != Some(state.session.as_str())
            {
                self.last_tick = Some(state.tick);
                self.last_change_ms = now_ms;
            }
            let new_session = self.session.as_deref() != Some(state.session.as_str());
            self.state = Some(state);
            if new_session {
                self.begin_session();
            }
        }
        let alive = self.state.is_some() && now_ms.saturating_sub(self.last_change_ms) <= STALE_MS;
        if alive != self.connected {
            self.connected = alive;
            self.events.push(
                if alive {
                    "the game is answering"
                } else {
                    "the game stopped answering"
                }
                .to_string(),
            );
            if alive {
                // The panel may have been lost while the game was away.
                self.screen_sent = None;
            }
        }
    }

    /// The bridge started anew (game restart or mod reload): nothing sent before counts any more.
    fn begin_session(&mut self) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        self.events.push(format!(
            "bridge session {} (bridge {})",
            state.session, state.bridge_version
        ));
        self.session = Some(state.session.clone());
        // Keys pressed before this director was listening are not choices.
        self.last_key_seq = state.keys.iter().map(|k| k.seq).max().unwrap_or(0);
        self.flight = None;
        self.queue.clear();
        self.watch.clear();
        self.screen_sent = None;
        let _ = self.transport.clear();
        self.queue.push_back(Command::Hello {
            version: VERSION.to_string(),
        });
        // Enemies of the old session are not this bridge's to track. Clear is harmless if none remain.
        self.queue.push_back(Command::Clear);
        if let Some(run) = self.run.as_mut() {
            if run.fight_interrupted().is_ok() {
                self.save_run();
            }
        }
    }

    fn take_answer(&mut self, now_ms: u64) {
        let Some(flight) = self.flight.as_ref() else {
            return;
        };
        let answer = self
            .state
            .as_ref()
            .and_then(|s| s.ack.as_ref())
            .filter(|ack| ack.id == flight.id)
            .cloned();
        let Some(ack) = answer else {
            if self.connected && now_ms.saturating_sub(flight.sent_ms) >= RESEND_MS {
                let line = flight.line.clone();
                if self.transport.send(&line).is_ok() {
                    if let Some(flight) = self.flight.as_mut() {
                        flight.sent_ms = now_ms;
                    }
                }
            }
            return;
        };
        let Some(flight) = self.flight.take() else {
            return;
        };
        let _ = self.transport.clear();
        match &flight.command {
            Command::Spawn {
                handle, unit_id, ..
            } => {
                if let Some(watched) = self.watch.iter_mut().find(|w| &w.handle == handle) {
                    watched.summon = if ack.ok {
                        Summon::Summoned
                    } else {
                        Summon::Failed
                    };
                }
                if !ack.ok {
                    self.events
                        .push(format!("{unit_id} could not be summoned: {}", ack.message));
                }
            }
            Command::Screen { .. } => {
                let problem = (!ack.ok).then(|| ack.message.clone());
                if problem != self.panel_problem {
                    if let Some(why) = &problem {
                        self.events
                            .push(format!("the in-game panel is not shown: {why}"));
                    }
                    self.panel_problem = problem;
                }
            }
            Command::Probe => {
                let results = self
                    .state
                    .as_ref()
                    .map(|s| s.probes.clone())
                    .unwrap_or_default();
                for r in &results {
                    self.events
                        .push(format!("probe {:<28} {:<5} {}", r.name, r.status, r.detail));
                }
                if !ack.ok {
                    self.events
                        .push(format!("the probe failed: {}", ack.message));
                }
                self.probe = Some(results);
            }
            Command::Hello { .. } => self
                .events
                .push(format!("the bridge answered: {}", ack.message)),
            other => {
                if !ack.ok {
                    self.events
                        .push(format!("{} failed: {}", other.action(), ack.message));
                }
            }
        }
    }

    fn take_keys(&mut self) {
        if !self.connected {
            return;
        }
        let mut fresh: Vec<(u64, String)> = self
            .state
            .as_ref()
            .map(|s| {
                s.keys
                    .iter()
                    .filter(|k| k.seq > self.last_key_seq)
                    .map(|k| (k.seq, k.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        fresh.sort();
        for (seq, name) in fresh {
            self.last_key_seq = seq;
            if let Ok(digit @ 1..=9) = name.parse::<u8>() {
                self.key(UserKey::Digit(digit));
            }
        }
    }

    /// A key press, from the game or from the director's own window.
    pub fn key(&mut self, key: UserKey) {
        self.flash = None;
        match key {
            UserKey::Probe => {
                self.queue.push_back(Command::Probe);
                self.flash = Some("Probe requested.".to_string());
            }
            UserKey::Abandon => {
                if self.run.as_ref().is_some_and(|r| !r.is_over()) {
                    self.run = None;
                    self.store.remove("run.json");
                    self.watch.clear();
                    self.queue
                        .retain(|c| matches!(c, Command::Hello { .. } | Command::Probe));
                    self.queue.push_back(Command::Clear);
                    self.flash = Some("The run was abandoned.".to_string());
                    self.events.push("run abandoned".to_string());
                }
            }
            UserKey::Digit(n) => {
                let choice = usize::from(n.saturating_sub(1));
                match self.run.as_ref().map(Run::is_over) {
                    None => self.title_choice(choice),
                    Some(true) => self.finished_choice(choice),
                    Some(false) => self.run_choice(choice),
                }
            }
        }
    }

    fn title_choice(&mut self, choice: usize) {
        match choice {
            0 => self.start_run(),
            1 if self.profile.heat_unlocked > 0 => {
                self.profile.cycle_heat();
                self.save_profile();
            }
            _ => self.flash = Some("There is no such option.".to_string()),
        }
    }

    fn finished_choice(&mut self, choice: usize) {
        match choice {
            0 => self.start_run(),
            1 => {
                self.run = None;
                self.unlocked_heat = false;
            }
            _ => self.flash = Some("There is no such option.".to_string()),
        }
    }

    fn start_run(&mut self) {
        let seed = self.fixed_seed.take().unwrap_or_else(|| {
            let mut rng = Rng::new(
                self.entropy ^ u64::from(self.profile.runs).wrapping_mul(0xA24B_AED4_963E_E407),
            );
            // Short enough to read out and type back in.
            rng.next_u64() % 1_000_000_000
        });
        match Run::new(&self.content, seed, self.profile.heat) {
            Ok(run) => {
                self.events
                    .push(format!("new run: seed {seed}, heat {}", run.heat));
                self.run = Some(run);
                self.unlocked_heat = false;
                self.watch.clear();
                self.save_run();
            }
            Err(why) => self.flash = Some(format!("A run cannot start: {why}")),
        }
    }

    fn run_choice(&mut self, choice: usize) {
        let Some(run) = self.run.as_ref() else { return };
        if matches!(
            run.phase,
            Phase::Fight {
                stage: FightStage::Ready,
                ..
            }
        ) && choice == 0
        {
            // Summoning needs a living player standing in a map.
            if !self.game_ready() {
                let why = match self.state.as_ref() {
                    Some(state) if self.connected => state.reason.clone(),
                    _ => "the game is not running".to_string(),
                };
                self.flash = Some(format!("Not yet: {why}."));
                return;
            }
            // Everything sent before must be done, so the arena is clear and health is settled.
            if self
                .queue
                .iter()
                .any(|c| !matches!(c, Command::Screen { .. }))
                || self.flight.is_some()
            {
                self.flash = Some("One moment.".to_string());
                return;
            }
        }
        let Some(run) = self.run.as_mut() else { return };
        match run.input(&self.content, Input::Choose(choice)) {
            Ok(ops) => self.after_step(ops),
            Err(why) => {
                let mut text = why.to_string();
                if let Some(first) = text.get(..1) {
                    text = first.to_uppercase() + &text[1..];
                }
                self.flash = Some(format!("{text}."));
            }
        }
    }

    /// Queues what the run asked of the game, saves, and closes the run if it ended.
    fn after_step(&mut self, ops: Vec<GameOp>) {
        let count = ops
            .iter()
            .filter(|op| matches!(op, GameOp::Spawn(_)))
            .count() as u32;
        let mut slot = 0;
        for op in ops {
            match op {
                GameOp::ClearUnits => {
                    self.watch.clear();
                    self.queue.push_back(Command::Clear);
                }
                GameOp::Heal { pct } => self.queue.push_back(Command::Heal { pct }),
                GameOp::Hurt { pct } => self.queue.push_back(Command::Hurt { pct }),
                GameOp::Spawn(spec) => {
                    self.next_handle += 1;
                    let handle = format!("u{}", self.next_handle);
                    self.watch.push(Watched {
                        handle: handle.clone(),
                        unit_id: spec.unit_id.clone(),
                        summon: Summon::Pending,
                    });
                    self.queue.push_back(Command::Spawn {
                        handle,
                        unit_id: spec.unit_id,
                        level: spec.level,
                        health_pct: spec.health_pct,
                        slot,
                        count,
                    });
                    slot += 1;
                }
            }
        }
        let over = self.run.as_ref().is_some_and(Run::is_over);
        if over {
            if let Some(run) = self.run.as_ref() {
                self.unlocked_heat = self.profile.record(&self.content, run);
                self.events.push(format!(
                    "run over: {}",
                    if run.phase == Phase::Victory {
                        "victory"
                    } else {
                        "defeat"
                    }
                ));
            }
            self.save_profile();
            self.store.remove("run.json");
        } else {
            self.save_run();
        }
    }

    fn save_run(&mut self) {
        if let Some(run) = self.run.as_ref() {
            if let Err(why) = self.store.write("run.json", &run.to_json()) {
                self.events.push(why);
            }
        }
    }

    fn save_profile(&mut self) {
        if let Err(why) = self.store.write("profile.json", &self.profile.to_json()) {
            self.events.push(why);
        }
    }

    /// Decides the fight in progress from what the bridge reports.
    fn watch_fight(&mut self) {
        let active = self.run.as_ref().is_some_and(|r| {
            matches!(
                r.phase,
                Phase::Fight {
                    stage: FightStage::Active,
                    ..
                }
            )
        });
        if !active || !self.connected || self.watch.is_empty() {
            return;
        }
        // Wait until every summon is answered.
        if self.watch.iter().any(|w| w.summon == Summon::Pending) {
            return;
        }
        let Some(state) = self.state.as_ref() else {
            return;
        };

        let failed: Vec<String> = self
            .watch
            .iter()
            .filter(|w| w.summon == Summon::Failed)
            .map(|w| w.unit_id.clone())
            .collect();
        if !failed.is_empty() {
            let result = self
                .run
                .as_mut()
                .map(|r| r.spawn_failed(&self.content, &failed));
            if let Some(Ok(ops)) = result {
                self.after_step(ops);
            }
            return;
        }

        // The player's death comes first: a trade is a loss.
        if state
            .player
            .as_ref()
            .is_some_and(|p| !p.alive || p.hp <= 0.0)
        {
            let result = self
                .run
                .as_mut()
                .map(|r| r.input(&self.content, Input::FightLost));
            if let Some(Ok(ops)) = result {
                self.events.push("the player fell".to_string());
                self.after_step(ops);
            }
            return;
        }

        let (mut alive, mut dead) = (0usize, 0usize);
        for watched in &self.watch {
            match state
                .units
                .iter()
                .find(|u| u.handle == watched.handle)
                .map(|u| u.status)
            {
                Some(UnitStatus::Alive) => alive += 1,
                Some(UnitStatus::Dead) => dead += 1,
                // Gone, or not listed at all: it left without dying.
                Some(UnitStatus::Gone) | None => {}
            }
        }
        if alive > 0 {
            return;
        }
        // Nobody is left standing. It is a win if they were killed; enemies that merely vanished
        // (a map change, left far behind) win nothing, though a straggler does not void a pack.
        let total = self.watch.len();
        let result = if dead >= 1 && dead * 2 >= total {
            self.run
                .as_mut()
                .map(|r| r.input(&self.content, Input::FightWon))
        } else {
            self.events
                .push("the enemies vanished: the fight is to be begun again".to_string());
            self.run.as_mut().map(|r| r.fight_interrupted())
        };
        if let Some(Ok(ops)) = result {
            self.after_step(ops);
        }
    }

    fn refresh_screen(&mut self) {
        if !self.connected {
            return;
        }
        let text = self.panel_text();
        if self.screen_sent.as_deref() == Some(text.as_str()) {
            return;
        }
        // Only the newest text matters.
        self.queue.retain(|c| !matches!(c, Command::Screen { .. }));
        // The panel goes ahead of game work so a key press shows at once, but after a greeting.
        let at = self
            .queue
            .iter()
            .position(|c| !matches!(c, Command::Hello { .. }))
            .unwrap_or(self.queue.len());
        self.queue.insert(
            at,
            Command::Screen {
                visible: true,
                text: text.clone(),
            },
        );
        self.screen_sent = Some(text);
    }

    fn pump(&mut self, now_ms: u64) {
        if self.flight.is_some() || !self.connected {
            return;
        }
        let ready = self.game_ready();
        // Commands go in order. One that needs the player waits for the player, and holds back
        // the game work behind it; the panel and the greeting may pass.
        let needs_player = |c: &Command| {
            matches!(
                c,
                Command::Spawn { .. } | Command::Heal { .. } | Command::Hurt { .. }
            )
        };
        let passes = |c: &Command| {
            matches!(
                c,
                Command::Screen { .. } | Command::Hello { .. } | Command::Probe
            )
        };
        let pick = match self.queue.front() {
            None => None,
            Some(first) if ready || !needs_player(first) => Some(0),
            Some(_) => self.queue.iter().position(passes),
        };
        let Some(command) = pick.and_then(|i| self.queue.remove(i)) else {
            return;
        };
        self.next_id += 1;
        let id = format!("{}-{}", self.id_prefix, self.next_id);
        let line = match command.encode(&id) {
            Ok(line) => line,
            Err(why) => {
                self.events.push(format!("a command was dropped: {why}"));
                if let Command::Spawn { handle, .. } = &command {
                    if let Some(watched) = self.watch.iter_mut().find(|w| &w.handle == handle) {
                        watched.summon = Summon::Failed;
                    }
                }
                return;
            }
        };
        match self.transport.send(&line) {
            Ok(()) => {
                self.flight = Some(Flight {
                    id,
                    line,
                    command,
                    sent_ms: now_ms,
                })
            }
            Err(why) => {
                self.events
                    .push(format!("the inbox cannot be written: {why}"));
                self.queue.push_front(command);
            }
        }
    }

    /// Title, lines and numbered options of the current screen.
    fn screen(&self) -> (String, Vec<String>, Vec<String>, Vec<String>) {
        match self.run.as_ref() {
            None => {
                let p = &self.profile;
                let mut body =
                    vec!["A roguelike boss rush. Cross three acts in one life.".to_string()];
                if p.runs > 0 {
                    body.push(format!(
                        "Runs {}   Wins {}   Furthest act {}   Fights won {}",
                        p.runs, p.wins, p.best_act, p.fights_won
                    ));
                }
                let mut options = vec![format!("Begin a new run (heat {})", p.heat)];
                if p.heat_unlocked > 0 {
                    options.push(format!(
                        "Change heat (0 to {}; each point raises enemy levels)",
                        p.heat_unlocked
                    ));
                }
                ("PBZ Roguelike".to_string(), body, options, Vec::new())
            }
            Some(run) => {
                let v = view(run, &self.content);
                let mut body = vec![v.status];
                body.extend(v.body);
                let mut options = v.options;
                if run.is_over() {
                    if self.unlocked_heat {
                        body.push(format!("Heat {} is unlocked.", self.profile.heat_unlocked));
                    }
                    options = vec![
                        "Begin a new run".to_string(),
                        "Back to the title".to_string(),
                    ];
                }
                (v.title, body, options, v.map_art)
            }
        }
    }

    fn layout(&self, with_map: bool) -> String {
        let (title, body, options, map_art) = self.screen();
        let mut lines = vec![title, String::new()];
        lines.extend(body);
        if with_map && !map_art.is_empty() {
            lines.push(String::new());
            lines.extend(map_art);
        }
        if !options.is_empty() {
            lines.push(String::new());
            for (i, option) in options.iter().enumerate() {
                lines.push(format!(" {}. {}", i + 1, option));
            }
        }
        if let Some(flash) = &self.flash {
            lines.push(String::new());
            lines.push(flash.clone());
        }
        lines.join("\n")
    }

    /// What the in-game panel shows.
    pub fn panel_text(&self) -> String {
        self.layout(false)
    }

    /// What the director's own window shows: the panel, the map, and how the game is doing.
    pub fn console_text(&self) -> String {
        let link = if !self.connected {
            "Game: not answering. Start Phantom Blade Zero with the mod installed and load into a map.".to_string()
        } else if let Some(state) = self.state.as_ref().filter(|s| !s.ready) {
            format!("Game: connected, waiting ({}).", state.reason)
        } else {
            "Game: connected.".to_string()
        };
        let mut text = self.layout(true);
        text.push_str("\n\n");
        text.push_str(&link);
        if let Some(why) = &self.panel_problem {
            text.push_str(&format!(
                "\nThe in-game panel is not shown ({why}). Use this window."
            ));
        }
        text
    }
}
