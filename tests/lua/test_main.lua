-- Loads the real main.lua against the mock engine and real files.
local Mock = require('mock_ue')
local tests = {}

local function temp_dir(T)
    local path = os.tmpname()
    os.remove(path)
    assert(os.execute('mkdir -p "' .. path .. '/Scripts" "' .. path .. '/Runtime"'))
    for _, name in ipairs({ 'main', 'config', 'proto', 'bridge', 'game', 'hud', 'keys' }) do
        local source = T.read('mod/PBZRoguelike/Scripts/' .. name .. '.lua')
        local file = assert(io.open(path .. '/Scripts/' .. name .. '.lua', 'wb'))
        file:write(source); file:close()
    end
    return path
end

local function read(path)
    local file = io.open(path, 'rb')
    if not file then return nil end
    local content = file:read('*a'); file:close()
    return content
end

function tests.the_bridge_starts_and_answers_through_real_files(T)
    local dir = temp_dir(T)
    local env = Mock.install()
    for _, name in ipairs({ 'config', 'proto', 'bridge', 'game', 'hud', 'keys' }) do package.loaded[name] = nil end
    local saved_path, saved_print = package.path, print
    print = function() end
    local chunk = assert(loadfile(dir .. '/Scripts/main.lua'))
    local ok, why = pcall(chunk)
    print = saved_print
    package.path = saved_path
    T.ok(ok, 'main.lua raised: ' .. tostring(why))
    T.eq(#env.loops, 1, 'main.lua starts one game-thread loop')
    T.eq(env.loops[1].ms, 16)
    local step = env.loops[1].fn

    print = function() end
    for _ = 1, 6 do step() end
    local state = read(dir .. '/Runtime/state.tsv')
    T.ok(state, 'no state file after six steps (the bridge talks every 100 ms)')
    T.ok(state:find('^begin\t1\n'), 'the first state has tick 1')
    T.ok(state:find('\nready\t1\tready\n', 1, true))

    local inbox = assert(io.open(dir .. '/Runtime/inbox.tsv', 'wb'))
    inbox:write('PBZR1\tm1\tspawn\tu1\tBP_Monster_MoTian\t7\t100\t0\t1\n'); inbox:close()
    for _ = 1, 6 do step() end
    state = read(dir .. '/Runtime/state.tsv')
    T.ok(state:find('\nack\tm1\tok\tsummoned\n', 1, true), state)
    T.ok(state:find('\nunit\tu1\talive\t96000.0\t96000.0\tBP_Monster_MoTian\n', 1, true), state)
    T.eq(env.npcs[1].Level, 7)

    inbox = assert(io.open(dir .. '/Runtime/inbox.tsv', 'wb'))
    inbox:write('PBZR1\tm2\tscreen\t1\tThe Road\\n 1. Fight\n'); inbox:close()
    for _ = 1, 6 do step() end
    T.ok(read(dir .. '/Runtime/state.tsv'):find('\nack\tm2\tok\tok\n', 1, true))

    -- A choice key pressed while the panel is up reaches the state file.
    env.keys_down.Two = false
    step()
    env.keys_down.Two = true
    for _ = 1, 6 do step() end
    T.ok(read(dir .. '/Runtime/state.tsv'):find('\nkey\t1\t2\n', 1, true), 'the key press was not reported')
    print = saved_print

    local log = read(dir .. '/Runtime/bridge.log')
    T.ok(log and log:find('bridge 0.1.0 started; loop=LoopInGameThreadWithDelay', 1, true), 'no start line in bridge.log')
    T.ok(log:find(dir .. '/Runtime', 1, true), 'the Runtime folder next to Scripts is used')
    os.execute('rm -rf "' .. dir .. '"')
end

function tests.an_old_ue4ss_falls_back_to_the_async_loop(T)
    local dir = temp_dir(T)
    local env = Mock.install()
    for _, name in ipairs({ 'config', 'proto', 'bridge', 'game', 'hud', 'keys' }) do package.loaded[name] = nil end
    LoopInGameThreadWithDelay = nil
    local async, queued = nil, {}
    LoopAsync = function(ms, fn) async = fn end
    ExecuteInGameThread = function(fn) queued[#queued + 1] = fn end
    local saved_path, saved_print = package.path, print
    print = function() end
    local ok, why = pcall(assert(loadfile(dir .. '/Scripts/main.lua')))
    package.path = saved_path
    T.ok(ok, tostring(why))
    T.ok(async, 'LoopAsync was not used')
    -- The timer thread only queues; a stalled game thread gets one step, not a pile.
    T.eq(async(), false); async(); async()
    T.eq(#queued, 1)
    for _ = 1, 6 do
        local fn = table.remove(queued, 1)
        fn()
        async()
    end
    print = saved_print
    T.ok(read(dir .. '/Runtime/state.tsv'), 'the fallback loop writes the state file')
    os.execute('rm -rf "' .. dir .. '"')
end

return tests
