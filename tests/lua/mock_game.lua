-- A pretend game that runs the real bridge scripts, for the director's end-to-end test:
--   lua tests/lua/mock_game.lua <repository root> <work folder>
-- The work folder holds Scripts/ (a copy of the mod's scripts), Runtime/ and units.txt (the units
-- the codex knows, one id per line). The test steers it through Runtime/control.txt, one line
-- "<number> <word> [argument]"; a line is acted on when its number changes.
local ROOT, WORK = arg[1], arg[2]
assert(ROOT and WORK, 'usage: mock_game.lua <repository root> <work folder>')
package.path = ROOT .. '/tests/lua/?.lua;' .. package.path
local Mock = require('mock_ue')

local units = {}
for line in io.lines(WORK .. '/units.txt') do
    if line ~= '' then units[line] = 1000 end
end

local real_print = print
local env, step
local function start()
    env = Mock.install({ units = units })
    for _, name in ipairs({ 'config', 'proto', 'bridge', 'game', 'hud', 'keys' }) do package.loaded[name] = nil end
    local saved = package.path
    print = function() end
    assert(loadfile(WORK .. '/Scripts/main.lua'))()
    package.path = saved
    assert(#env.loops == 1, 'main.lua did not start its loop')
    step = env.loops[1].fn
end
start()

local damage, last_seq, press = 0.34, nil, nil
local function control()
    local file = io.open(WORK .. '/Runtime/control.txt', 'rb')
    if not file then return end
    local line = file:read('*l')
    file:close()
    if not line then return end
    local seq, word, argument = line:match('^(%d+) (%a+) ?(.*)$')
    if not seq or seq == last_seq then return end
    last_seq = seq
    if word == 'press' then
        press = { key = argument, stage = 0 }
    elseif word == 'kill' then
        Mock.set_health(env.pawn, 0)
    elseif word == 'revive' then
        Mock.set_health(env.pawn, 1000)
    elseif word == 'damage' then
        damage = tonumber(argument) or damage
    elseif word == 'vanish' then
        for _, npc in ipairs(env.npcs) do npc._valid = false end
    elseif word == 'restart' then
        start()
    elseif word == 'stop' then
        os.exit(0)
    end
end

local started = os.time()
while os.time() - started < 180 do
    control()
    -- A key is seen up for one bridge tick, then down for one, then up again.
    if press then
        press.stage = press.stage + 1
        env.keys_down[press.key] = press.stage == 2
        if press.stage >= 3 then press = nil end
    end
    -- The fight: every active enemy loses health while the player stands.
    if damage > 0 and Mock.health(env.pawn) > 0 then
        for _, npc in ipairs(env.npcs) do
            if npc._valid and npc.active and npc.AttributeSetBase then
                local hp = Mock.health(npc)
                if hp > 0 then
                    Mock.set_health(npc, math.max(0, hp - npc.AttributeSetBase.Health_Max.CurrentValue * damage))
                end
            end
        end
    end
    for _ = 1, 6 do step() end
    -- Yield a little so the director gets to read between ticks.
    local t = os.clock()
    while os.clock() - t < 0.001 do end
end
real_print('mock game: time limit reached')
os.exit(3)
