//! pbz-roguelike: the director's window.
//!
//! Run it beside the game. It shows the same screens as the in-game panel, takes the same number
//! keys (type the number and press Enter), and keeps the run saved in the mod's `Runtime` folder.

use pbz_rogue_core::content::Content;
use pbz_rogue_core::embedded;
use pbz_rogue_director::director::{Director, Store, UserKey, VERSION};
use pbz_rogue_director::sim::SimGame;
use pbz_rogue_director::transport::{FileTransport, Transport};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HELP: &str = "\
pbz-roguelike: the director of the PBZ Roguelike mod

USAGE
    pbz-roguelike [OPTIONS]

OPTIONS
    --runtime <DIR>   Folder shared with the in-game bridge and holding the saves.
                      Default: the Runtime folder next to this program.
    --seed <NUMBER>   Seed of the next new run (the same seed gives the same run).
    --probe           Once the game answers, check the engine calls the bridge relies on
                      and print the result.
    --sim             Play against a pretend game. No game needed; nothing is saved.
    -h, --help        Show this help.
    -V, --version     Show the version.

IN THE WINDOW
    1-9       choose an option (then Enter)
    abandon   give up the run in progress
    probe     check the bridge's engine calls
    quit      close the director (the run stays saved)
  With --sim also: win (kill the enemies), die, up (stand again), restart (restart the game)
";

struct Args {
    runtime: Option<PathBuf>,
    seed: Option<u64>,
    probe: bool,
    sim: bool,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
    let mut out = Args {
        runtime: None,
        seed: None,
        probe: false,
        sim: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("pbz-roguelike {VERSION}");
                return Ok(None);
            }
            "--sim" => out.sim = true,
            "--probe" => out.probe = true,
            "--runtime" => {
                out.runtime = Some(PathBuf::from(
                    args.next().ok_or("--runtime needs a folder")?,
                ));
            }
            "--seed" => {
                let text = args.next().ok_or("--seed needs a number")?;
                out.seed = Some(
                    text.parse()
                        .map_err(|_| format!("--seed needs a whole number, not {text:?}"))?,
                );
            }
            other => return Err(format!("unknown option {other:?}; try --help")),
        }
    }
    Ok(Some(out))
}

fn program_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The content compiled in, with any of the four files replaced by one in `data/` beside the
/// program. Edited data that does not validate stops the program instead of being ignored.
fn load_content(dir: &Path) -> Result<Content, String> {
    let data = dir.join("data");
    let pick = |name: &str, built_in: &str| -> Result<String, String> {
        let path = data.join(name);
        if path.is_file() {
            fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
        } else {
            Ok(built_in.to_string())
        }
    };
    Content::from_json(
        &pick("roster.json", embedded::ROSTER)?,
        &pick("acts.json", embedded::ACTS)?,
        &pick("blessings.json", embedded::BLESSINGS)?,
        &pick("events.json", embedded::EVENTS)?,
    )
}

fn stdin_lines() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line.trim().to_lowercase()).is_err() {
                break;
            }
        }
    });
    rx
}

struct Log {
    file: Option<fs::File>,
}

impl Log {
    fn open(dir: Option<&Path>) -> Log {
        let file = dir.and_then(|d| {
            let path = d.join("director.log");
            // Start over once the log has grown large.
            if fs::metadata(&path)
                .map(|m| m.len() > 1_000_000)
                .unwrap_or(false)
            {
                let _ = fs::remove_file(&path);
            }
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()
        });
        Log { file }
    }

    fn line(&mut self, text: &str) {
        println!("  - {text}");
        if let Some(file) = self.file.as_mut() {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(file, "{secs} {text}");
        }
    }
}

/// Runs the director until `quit`. `extra` handles words only some transports understand.
fn drive<T: Transport>(
    mut director: Director<T>,
    mut log: Log,
    mut probe_wanted: bool,
    farewell: &str,
    mut extra: impl FnMut(&mut Director<T>, &str) -> bool,
) {
    let lines = stdin_lines();
    let started = Instant::now();
    let base = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut shown = String::new();
    loop {
        while let Ok(line) = lines.try_recv() {
            match line.as_str() {
                "" => {}
                "q" | "quit" | "exit" => {
                    println!("{farewell}");
                    return;
                }
                "help" | "?" => print!("{HELP}"),
                "abandon" => director.key(UserKey::Abandon),
                "probe" => director.key(UserKey::Probe),
                word => match word.parse::<u8>() {
                    Ok(digit @ 1..=9) => director.key(UserKey::Digit(digit)),
                    _ => {
                        if !extra(&mut director, word) {
                            println!("  ? {word:?} is not a command here; type help");
                        }
                    }
                },
            }
        }
        director.tick(base + started.elapsed().as_millis() as u64);
        if probe_wanted && director.connected() {
            probe_wanted = false;
            director.key(UserKey::Probe);
        }
        for event in director.take_events() {
            log.line(&event);
        }
        let text = director.console_text();
        if text != shown {
            println!("\n{}\n{text}\n> ", "=".repeat(72));
            let _ = io::stdout().flush();
            shown = text;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
    };
    let dir = program_dir();
    let content = match load_content(&dir) {
        Ok(content) => content,
        Err(why) => {
            eprintln!("The mod's data cannot be used: {why}");
            return ExitCode::from(2);
        }
    };
    let start_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    println!("pbz-roguelike {VERSION}");

    if args.sim {
        println!("Pretend game: type win to kill the enemies, die to fall, up to stand again.");
        let mut director = Director::new(content, SimGame::new(), Store::in_memory(), start_ms);
        director.fixed_seed = args.seed;
        drive(
            director,
            Log::open(None),
            args.probe,
            "Goodbye.",
            |director, word| {
                let sim = director.transport_mut();
                match word {
                    "win" => sim.win(),
                    "die" => sim.kill_player(),
                    "up" => sim.revive_player(),
                    "restart" => sim.restart(),
                    _ => return false,
                }
                true
            },
        );
        return ExitCode::SUCCESS;
    }

    let runtime = args.runtime.unwrap_or_else(|| dir.join("Runtime"));
    let transport = match FileTransport::new(&runtime) {
        Ok(transport) => transport,
        Err(why) => {
            eprintln!("The folder {} cannot be used: {why}", runtime.display());
            return ExitCode::from(2);
        }
    };
    println!("Runtime folder: {}", runtime.display());
    let mut director = Director::new(content, transport, Store::at(runtime.clone()), start_ms);
    director.fixed_seed = args.seed;
    drive(
        director,
        Log::open(Some(&runtime)),
        args.probe,
        "The run stays saved. Goodbye.",
        |_, _| false,
    );
    ExitCode::SUCCESS
}
