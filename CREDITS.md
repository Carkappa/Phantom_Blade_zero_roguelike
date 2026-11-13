# Credits

PBZ Roguelike is a fan-made mod. It is not affiliated with or endorsed by S-GAME, the makers of
Phantom Blade Zero. It contains no game files: everything it summons is already in your copy of the
game.

## What this mod builds on

- **[RogueRing](https://github.com/xjoje6/RogueRing)** by Mionel Lessi (MIT). The idea of a Slay the
  Spire style boss rush laid over a game's own combat, and the split between run logic that is tested
  without the game and a thin layer that drives the game. No RogueRing code is used.
- **[PhantomGladiator](https://github.com/LingYi-Liang/PhantomGladiator)** by 泠依_01 (LingYi-Liang)
  (MIT). Everything this mod knows about Phantom Blade Zero's engine side comes from its source and
  its notes, tested by its author in the game:
  - how to summon an enemy (`SpawnAIDefered`, `FinishSpawnAIDefered`, `ActivateAI` on the game's AI
    library, with `Level` written before the enemy starts);
  - where health lives (`AttributeSetBase`) and how class paths are read from the game's codex;
  - why keys are read on the game thread and not with `RegisterKeyBind`;
  - which units write story progress when they are fought (the reason for this mod's exclusions);
  - the list of codex units in `data/roster.json` (ids, names, chapters, boss flag) and which of them
    it recorded real fights with.

  The bridge scripts here are written for this mod; they follow those calls, they are not copies.
- **[UE4SS](https://github.com/UE4SS-RE/RE-UE4SS)**, the mod loader that runs the bridge in the game.
- **Slay the Spire** by Mega Crit, for the shape of the map and of a run.
