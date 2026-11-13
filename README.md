# PBZ Roguelike

A roguelike boss rush for Phantom Blade Zero, in the style of Slay the Spire. Cross a branching map
of fights, elites, shrines, merchants and events, collect blessings that change the run, and beat the
lord of each of three acts in one life. Combat is the game's own; everything around it is the mod.

The idea follows [RogueRing](https://www.nexusmods.com/eldenring/mods/11459) for Elden Ring.

> **Status: not yet run in the game.** The run logic, the director and the bridge's own logic are
> tested (see [Tests](#tests)), but every test ran outside the game, against a stand-in for the
> engine. The calls into the game follow ones another mod tested there; this mod's use of them has
> not been tried. Expect the first sessions to need fixes. [First run](#first-run) says how to check
> what works.

## How a run goes

- **Three acts**, each a map of ten floors and a lord. You choose a path; each node is a fight, an
  elite, a shrine, a merchant, an event or a treasure.
- **Fights** summon enemies from the game's own roster in front of you: a pack of ordinary enemies,
  or a duel with a boss. Elites are bosses above the act's level. Enemy levels rise floor by floor.
- **Blessings** are chosen after fights, bought from merchants or found in events: healing after
  fights, weaker or lower-level enemies, more gold, cheaper merchants, forgiven deaths, and trades
  such as stronger enemies for more gold.
- **Death ends the run**, unless you hold a forgiven death: then you fight that battle again.
- **Heat**: winning a run unlocks the next heat. Each point raises every enemy's level.
- The same **seed** gives the same maps, enemies and rewards.

Your character is the one in your save, with its own gear and skills. The mod does not change it.

## What the mod touches

- It summons enemies and removes them again, and it changes current health: yours (healing, and
  events that cost health but never the last point) and that of enemies it summoned.
- It never writes items, stats, skills or story progress, and it never opens a save file. The run
  and your record are kept in the mod's own `Runtime` folder.
- Some enemies write story progress when they are fought, wherever they stand, and summoning them
  could mark parts of the story as played in your save. These are never summoned: they are excluded
  in `data/roster.json` (each with the reason) and refused a second time by the bridge itself. The
  list comes from [PhantomGladiator](https://github.com/LingYi-Liang/PhantomGladiator), whose author
  found them in the game files.
- Whether killing a summoned boss yourself counts for the game's own records (codex entries,
  achievements) has not been checked. **Back up your save before you try the mod.**

## Requirements

- Phantom Blade Zero on Steam (Windows).
- UE4SS for Phantom Blade Zero, the mod loader. The UE4SS packages on the game's Nexus page work
  with the current game build; older ones stop loading Lua mods after a game update.

## Install

1. Install UE4SS for Phantom Blade Zero and start the game once.
2. Copy the `PBZRoguelike` folder into `PBZ\platform\steam\ue4ss\Mods\`.
3. Start `PBZRoguelike\pbz-roguelike.exe`. It is the director: it runs the roguelike and shows the
   same screens as the in-game panel. Windows may warn about an unsigned program.
4. Start the game and load into a map. The director's window says `Game: connected.`

To remove the mod, delete the `PBZRoguelike` folder.

## Playing

The panel appears at the top left of the game. Choose with the number keys **1** to **9**; **F6**
hides and shows the panel, and the number keys do nothing for the mod while it is hidden. Keys and
the panel's place and size are set in `Scripts\config.lua`.

The director's window shows the same screen, plus the act map, and takes the same choices: type the
number and press Enter. It also takes `abandon` (give up the run), `probe` (see below) and `quit`.
Closing the director keeps the run; start it again to go on.

When a fight is announced, stand on open ground and choose **Begin the fight**: the enemies appear a
few steps in front of you.

To try the mod's screens without the game: `pbz-roguelike.exe --sim`.

## First run

Because the mod has not been run in the game yet, check it in this order:

1. In the director's window type `probe`. The bridge checks, without changing anything, each engine
   call it relies on and prints one line per check: the map, your character, your health, the AI
   library and its summon functions, the game's codex, the widget classes for the panel.
2. If the director never says `Game: connected.`, look at `UE4SS.log` for lines starting with
   `[PBZRoguelike]`. If the log says the `Runtime` folder cannot be written, set
   `Config.RuntimePath` in `Scripts\config.lua` to the folder's full path.
3. If the panel does not appear, the director says so and its window still works.
4. Begin a run and a first fight on open ground, away from story areas.

`Runtime\bridge.log` and `Runtime\director.log` hold what each side saw. Please attach both to a bug
report, with the output of `probe`.

## Known limits

- Enemies marked untested in `data/roster.json` (`"verified": false`, which is all but five) were
  not confirmed to fight properly when summoned. An enemy the game cannot summon is replaced by
  another, and never chosen again in that run.
- A fight is won when the game reports every summoned enemy at zero health. If the enemies vanish
  instead (you travel away, the map changes), the fight waits to be begun again.
- Restarting the game or reloading mods in the middle of a fight returns the run to just before
  that fight.
- Names of enemies are shown as the game's codex has them, in Chinese, next to a label made from
  the game's internal name.

## Changing the content

The four files in `data` beside the program are the content: `roster.json` (which enemies may
appear, and in which role), `acts.json` (acts, levels and the numbers of a run), `blessings.json`
and `events.json`. Edit them and restart the director. Data that could break a run is refused at
start-up with the reason.

## Building

The mod is two parts.

| Part | Where | What it does |
| --- | --- | --- |
| Director | `crates/core`, `crates/director` (Rust) | Decides everything: map, encounters, rewards, saves. `pbz-roguelike.exe`. |
| Bridge | `mod/PBZRoguelike/Scripts` (Lua, run by UE4SS) | Does what the director says in the game thread and reports what it sees. |

They exchange two small text files in `Runtime`; `crates/director/src/protocol.rs` describes them.
Lua is unavoidable for the bridge, since UE4SS is the game's mod loader and runs Lua; it is kept to
summoning, health, the panel and keys.

```
cargo build --release -p pbz-rogue-director
tools/package.sh target/release/pbz-roguelike.exe
```

The package must hold the Windows program. `.github/workflows/build.yml` builds it on Windows and
uploads the `PBZRoguelike` folder as an artifact.

## Tests

```
cargo test --workspace
lua tests/lua/run_tests.lua .
```

- `crates/core`: the map generator over thousands of seeds, and whole runs played by a bot with
  every rule checked at each step, including that excluded enemies are never chosen.
- `crates/director`: the director against a simulated game: fights, deaths, enemies that cannot be
  summoned or that vanish, a game restart in a fight, lost commands, saving and resuming.
- `tests/lua`: the bridge scripts themselves, over a stand-in for the engine
  (`tests/lua/mock_ue.lua`).
- `crates/director/tests/lua_bridge.rs`: the director and the real bridge scripts playing a whole
  run through real files. It needs a Lua interpreter (`PBZR_LUA`, or `lua5.4`, `lua` or `luajit` on
  the PATH) and is skipped without one.

The stand-in for the engine is this project's reading of the game. These tests show the two halves
agree with each other and that the bridge's logic holds; they cannot show that the game behaves as
the stand-in does.

## License and credits

MIT, see [LICENSE](LICENSE). The mod builds on other people's work: see [CREDITS.md](CREDITS.md).
This is a fan-made mod, not affiliated with S-GAME.
