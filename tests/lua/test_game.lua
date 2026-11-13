local Mock = require('mock_ue')
local tests = {}

local function setup(options)
    local env = Mock.install(options)
    package.loaded['game'] = nil
    local Game = require('game')
    local logged = {}
    local game = Game.new({ config = { SummonDistance = 600, SummonSpacing = 250, SummonHeight = 50 },
        helpers = package.loaded['UEHelpers'], log = function(m) logged[#logged + 1] = m end })
    return game, env, Game, logged
end

function tests.ready_follows_the_player(T)
    local game, env = setup()
    local ok, why = game:ready()
    T.eq(ok, true); T.eq(why, 'ready')
    local player = game:player()
    T.eq(player.hp, 1000); T.eq(player.max, 1000); T.eq(player.alive, true)

    Mock.set_health(env.pawn, 0)
    ok, why = game:ready()
    T.eq(ok, false); T.eq(why, 'the player is down')
    T.eq(game:player().alive, false)

    env.pawn._valid = false
    ok, why = game:ready()
    T.eq(ok, false); T.eq(why, 'no player character')
    T.eq(game:player(), nil)

    env.no_controller = true
    ok = game:ready()
    T.eq(ok, false)
end

function tests.a_stale_controller_is_not_the_player(T)
    local game, env = setup()
    -- A controller left over from an earlier map: its player now belongs to another controller.
    env.controller.Player.PlayerController = Mock.object({})
    T.eq(game:controller(), nil)
    T.eq(game:pawn(), nil)
end

function tests.heal_and_hurt_stay_in_bounds(T)
    local game, env = setup()
    Mock.set_health(env.pawn, 400)
    T.eq(game:heal(35), true)
    T.near(Mock.health(env.pawn), 750)
    T.eq(game:heal(35), true)
    T.near(Mock.health(env.pawn), 1000, 'healing stops at the maximum')
    local ok, note = game:heal(10)
    T.eq(ok, true); T.eq(note, 'already at full health')

    T.eq(game:hurt(12), true)
    T.near(Mock.health(env.pawn), 880)
    Mock.set_health(env.pawn, 50)
    T.eq(game:hurt(60), true)
    T.near(Mock.health(env.pawn), 1, 'an event never takes the last point of health')
    ok, note = game:hurt(60)
    T.eq(ok, true); T.eq(note, 'no health to take')
    T.near(Mock.health(env.pawn), 1)

    -- A dead player is not healed back to life.
    Mock.set_health(env.pawn, 0)
    ok = game:heal(50)
    T.eq(ok, false)
    T.near(Mock.health(env.pawn), 0)
end

function tests.a_health_write_that_does_not_stick_is_reported(T)
    local game, env = setup()
    Mock.set_health(env.pawn, 400)
    -- The game refuses the write: reads keep returning the old value.
    env.pawn.AttributeSetBase = setmetatable({ _valid = true }, { __index = function(self, k)
        if k == 'IsValid' then return function() return true end end
        if k == 'Health' then return { BaseValue = 400, CurrentValue = 400 } end
        if k == 'Health_Max' then return { BaseValue = 1000, CurrentValue = 1000 } end
    end, __newindex = function() end })
    local ok, why = game:heal(35)
    T.eq(ok, false); T.eq(why, 'the health write did not read back')
end

function tests.spawn_sets_the_level_before_the_enemy_starts(T)
    local game, env = setup()
    local ok, note = game:spawn('u1', 'BP_Monster_MoTian', 12, 100, 0, 1)
    T.eq(ok, true); T.eq(note, 'summoned')
    T.eq(#env.npcs, 1)
    local npc = env.npcs[1]
    T.eq(npc.Level, 12)
    T.eq(table.concat(env.log, '; '), 'begin BP_Monster_MoTian; finish BP_Monster_MoTian level 12; activate BP_Monster_MoTian')
    T.eq(npc.active, true)
    -- Health came from the level held at start, as in the game.
    T.near(Mock.health(npc), 12000 * 13)
    -- In front of the player (facing +X), facing back at the player.
    T.near(npc.transform.Translation.X, 700); T.near(npc.transform.Translation.Y, 200); T.near(npc.transform.Translation.Z, 100)
    T.near(math.abs(npc.transform.Rotation.Z), 1); T.near(npc.transform.Rotation.W, 0)
    T.near(npc.transform.Scale3D.X, 1)
end

function tests.a_group_is_spread_side_by_side(T)
    local game, env = setup()
    env.pawn.forward = { X = 0, Y = 1, Z = 0 }
    for slot = 0, 2 do
        T.eq(game:spawn('u' .. slot, 'BP_Monster_MaleThug_Sword_Blue', 3, 100, slot, 3), true)
    end
    local xs = {}
    for i, npc in ipairs(env.npcs) do
        xs[i] = npc.transform.Translation.X
        T.near(npc.transform.Translation.Y, 800, 'all stand the same distance ahead')
    end
    table.sort(xs)
    T.near(xs[1], -150); T.near(xs[2], 100); T.near(xs[3], 350)
    -- Facing -Y, back at the player: yaw -90 degrees.
    T.near(env.npcs[1].transform.Rotation.Z, math.sin(-math.pi / 4))
    T.near(env.npcs[1].transform.Rotation.W, math.cos(-math.pi / 4))
end

function tests.one_enemy_per_handle(T)
    local game, env = setup()
    T.eq(game:spawn('u1', 'BP_Monster_MoTian', 5, 100, 0, 1), true)
    local ok, note = game:spawn('u1', 'BP_Monster_MoTian', 5, 100, 0, 1)
    T.eq(ok, true); T.eq(note, 'already summoned')
    T.eq(#env.npcs, 1)
end

function tests.denied_and_unknown_units_are_never_summoned(T)
    local game, env, Game = setup({ units = { BP_Monster_LeiDang = 1, BP_Monster_MoTian = 1, BP_Monster_ZuoShang = 1,
        BP_Monster_ZuoShang_Ending = 1, BP_Monster_Bandit_Sword_Rider = 1 } })
    for _, unit in ipairs({ 'BP_Monster_LeiDang', 'BP_Monster_LeiDang_P2', 'BP_Monster_LengTu', 'BP_Monster_ZuoShang',
        'BP_Monster_Bandit_Sword_Rider', 'BP_Monster_HookMan_HorseCartCombat', 'BP_Monster_PengMeng_Axe_NPC',
        'BP_Monster_MuXiaoKui_P2', 'BP_Monster_MoTian_LongZhu_01', 'BP_Monster_JingYe' }) do
        T.eq(Game.is_denied(unit), true, unit .. ' must be denied')
        local ok, why = game:spawn('x', unit, 5, 100, 0, 1)
        T.eq(ok, false, unit .. ' was summoned')
        T.ok(why:find('never summoned', 1, true), why)
    end
    -- Names that only look alike are allowed.
    for _, unit in ipairs({ 'BP_Monster_ZuoShang_Ending', 'BP_Monster_MoTian', 'BP_Monster_MuXiaoKui', 'BP_Monster_HookMan_P2' }) do
        T.eq(Game.is_denied(unit), false, unit .. ' must be allowed')
    end
    local ok, why = game:spawn('x', 'BP_Monster_NotInThisGame', 5, 100, 0, 1)
    T.eq(ok, false); T.eq(why, 'BP_Monster_NotInThisGame is not in the game codex')
    T.eq(#env.npcs, 0)
end

function tests.a_failed_spawn_leaves_nothing_behind(T)
    local game, env = setup()
    env.fail_spawn = { BP_Monster_MoTian = true }
    local ok, why = game:spawn('u1', 'BP_Monster_MoTian', 5, 100, 0, 1)
    T.eq(ok, false); T.ok(why:find('SpawnAIDefered', 1, true), why)
    T.eq(#game:units(), 0)

    env.fail_spawn = nil
    env.fail_finish = { BP_Monster_MoTian = true }
    ok, why = game:spawn('u2', 'BP_Monster_MoTian', 5, 100, 0, 1)
    T.eq(ok, false); T.ok(why:find('FinishSpawnAIDefered', 1, true), why)
    T.eq(#game:units(), 0)
    T.eq(env.destroyed, 1, 'the half-made enemy was removed')

    -- Without a map or a player nothing is tried.
    env.fail_finish = nil
    env.pawn._valid = false
    ok = game:spawn('u3', 'BP_Monster_MoTian', 5, 100, 0, 1)
    T.eq(ok, false)
    T.eq(#env.npcs, 1)
end

function tests.units_report_alive_dead_and_gone(T)
    local game, env = setup()
    game:spawn('a', 'BP_Monster_MaleThug_Sword_Blue', 0, 100, 0, 3)
    game:spawn('b', 'BP_Monster_MaleThug_Sword_Blue', 0, 100, 1, 3)
    game:spawn('c', 'BP_Monster_MaleThug_Sword_Blue', 0, 100, 2, 3)
    local units = game:units()
    T.eq(#units, 3)
    T.eq(units[1].handle, 'a'); T.eq(units[1].status, 'alive'); T.near(units[1].hp, 800); T.near(units[1].max, 800)
    T.eq(units[1].unit_id, 'BP_Monster_MaleThug_Sword_Blue')

    -- a is killed; its body is then removed: it stays dead.
    Mock.set_health(env.npcs[1], 0)
    T.eq(game:units()[1].status, 'dead')
    env.npcs[1]._valid = false
    T.eq(game:units()[1].status, 'dead', 'a dead enemy stays dead after its body is removed')

    -- b disappears at high health (left behind, unloaded): gone, not dead.
    Mock.set_health(env.npcs[2], 500)
    game:units()
    env.npcs[2]._valid = false
    T.eq(game:units()[2].status, 'gone')

    -- c is removed between two looks while almost dead: it died.
    Mock.set_health(env.npcs[3], 20)
    game:units()
    env.npcs[3].destroying = true
    T.eq(game:units()[3].status, 'dead')
end

function tests.reduced_starting_health_is_applied_once(T)
    local game, env = setup()
    game:spawn('u1', 'BP_Monster_AHua_GuiTouDao', 0, 80, 0, 1)
    local npc = env.npcs[1]
    T.near(Mock.health(npc), 9000, 'health is full until the first look')
    local unit = game:units()[1]
    T.near(Mock.health(npc), 7200)
    T.near(unit.hp, 7200); T.near(unit.max, 9000)
    -- The player hits it; the next look must not put the health back.
    Mock.set_health(npc, 5000)
    T.near(game:units()[1].hp, 5000)
    T.near(Mock.health(npc), 5000)
end

function tests.clear_removes_every_summoned_enemy(T)
    local game, env = setup()
    game:spawn('a', 'BP_Monster_MaleThug_Sword_Blue', 0, 100, 0, 2)
    game:spawn('b', 'BP_Monster_MoTian', 0, 100, 1, 2)
    Mock.set_health(env.npcs[1], 0)
    local ok, note = game:clear()
    T.eq(ok, true); T.eq(note, '2 removed')
    T.eq(env.destroyed, 2)
    T.eq(#game:units(), 0)
    ok, note = game:clear()
    T.eq(ok, true); T.eq(note, '0 removed')
    -- Handles can be used again after a clear.
    T.eq(game:spawn('a', 'BP_Monster_MoTian', 0, 100, 0, 1), true)
    T.eq(#game:units(), 1)
end

function tests.the_codex_gives_class_paths(T)
    local game, env, _, logged = setup()
    local count, why = game:load_catalog()
    T.eq(count, 3); T.eq(why, 'ok')
    T.eq(game.paths.BP_Monster_MoTian,
        '/PBZCombatAsset/ActorData/Mobs/BP_Monster_MoTian/BP_Monster_MoTian.BP_Monster_MoTian_C')
    -- Without a codex a spawn says so instead of guessing a path.
    local bare, bare_env = setup()
    bare_env.no_codex = true
    local ok, reason = bare:spawn('u1', 'BP_Monster_MoTian', 5, 100, 0, 1)
    T.eq(ok, false); T.eq(reason, 'BP_Monster_MoTian is not in the game codex')
end

function tests.the_probe_checks_every_call(T)
    local game, env = setup()
    local results = game:probe()
    T.ok(#results >= 14, 'the probe reports ' .. #results .. ' checks')
    local by_name = {}
    for _, r in ipairs(results) do
        by_name[r.name] = r
        T.eq(r.status, 'ok', r.name .. ': ' .. r.detail)
    end
    for _, name in ipairs({ 'world', 'controller', 'pawn', 'player_health', 'player_position', 'ai_library',
        'ai_SpawnAIDefered', 'ai_FinishSpawnAIDefered', 'ai_ActivateAI', 'codex', 'codex_sample' }) do
        T.ok(by_name[name], 'the probe does not check ' .. name)
    end
    T.eq(#env.npcs, 0, 'the probe summons nothing')

    -- With the game half missing it reports failures and does not raise.
    env.no_codex, env.no_controller = true, true
    env.objects['/SGameAI/BP_SGameAIFunctionLibrary.Default__BP_SGameAIFunctionLibrary_C']._valid = false
    local fresh = setup and require('game').new({ config = {}, helpers = package.loaded['UEHelpers'] })
    local failed = {}
    for _, r in ipairs(fresh:probe()) do if r.status == 'fail' then failed[r.name] = true end end
    T.ok(failed.controller and failed.pawn and failed.ai_library and failed.codex, 'missing pieces are reported')
end

return tests
