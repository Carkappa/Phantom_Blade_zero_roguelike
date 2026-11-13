local P = require('proto')
local Bridge = require('bridge')
local tests = {}

-- A bridge over in-memory files, with a game, panel and keys that record what they are asked.
local function setup(T, options)
    options = options or {}
    local store, open = T.files()
    local calls = {}
    local game = {
        ready = function() return true, 'ready' end,
        player = function() return { hp = 500, max = 1000, alive = true } end,
        units = function() return { { handle = 'u1', status = 'alive', hp = 10, max = 20, unit_id = 'BP_X' } } end,
        spawn = function(_, ...) calls[#calls + 1] = { 'spawn', ... }; return true, 'summoned' end,
        clear = function() calls[#calls + 1] = { 'clear' }; return true, '0 removed' end,
        heal = function(_, pct) calls[#calls + 1] = { 'heal', pct }; return true end,
        hurt = function(_, pct) calls[#calls + 1] = { 'hurt', pct }; return false, 'no health to read' end,
        probe = function() return { { name = 'world', status = 'ok', detail = 'x' } } end,
    }
    local hud = { show = function(_, visible, text) calls[#calls + 1] = { 'screen', visible, text }; return true end }
    local pressed = {}
    local keys = { poll = function() local out = pressed; pressed = {}; return out end }
    if options.before then options.before(store) end
    local bridge = Bridge.new({ dir = 'rt', game = options.game or game, hud = hud, keys = keys, open = open,
        session = 'test-session' })
    return bridge, store, calls, function(name) pressed[#pressed + 1] = name end, game
end

local function ack_line(store)
    return store['rt/state.tsv']:match('\n(ack\t[^\n]*)\n')
end

function tests.a_command_runs_once_and_is_answered(T)
    local bridge, store, calls = setup(T)
    store['rt/inbox.tsv'] = 'PBZR1\tc1\tspawn\tu1\tBP_Monster_MoTian\t12\t90\t0\t1\n'
    T.ok(bridge:tick())
    T.eq(#calls, 1)
    T.eq(calls[1][1], 'spawn'); T.eq(calls[1][2], 'u1'); T.eq(calls[1][3], 'BP_Monster_MoTian')
    T.eq(calls[1][4], 12); T.eq(calls[1][5], 90); T.eq(calls[1][6], 0); T.eq(calls[1][7], 1)
    T.eq(ack_line(store), 'ack\tc1\tok\tsummoned')
    -- The director has not cleared the inbox yet: the command must not run again.
    bridge:tick(); bridge:tick()
    T.eq(#calls, 1, 'a command ran twice')
    T.eq(ack_line(store), 'ack\tc1\tok\tsummoned', 'the answer stays until the next command')
    -- A new id runs.
    store['rt/inbox.tsv'] = 'PBZR1\tc2\tclear\n'
    bridge:tick()
    T.eq(#calls, 2); T.eq(calls[2][1], 'clear')
    T.eq(ack_line(store), 'ack\tc2\tok\t0 removed')
end

function tests.each_action_reaches_its_handler(T)
    local bridge, store, calls = setup(T)
    local function send(id, rest)
        store['rt/inbox.tsv'] = 'PBZR1\t' .. id .. '\t' .. rest .. '\n'
        bridge:tick()
        return ack_line(store)
    end
    T.eq(send('a1', 'hello\t9.9.9'), 'ack\ta1\tok\t' .. P.VERSION)
    T.eq(bridge.director_version, '9.9.9')
    T.eq(send('a2', 'screen\t1\tTitle\\n 1. Go'), 'ack\ta2\tok\tok')
    T.eq(calls[#calls][2], true); T.eq(calls[#calls][3], 'Title\n 1. Go')
    T.eq(send('a3', 'heal\t35'), 'ack\ta3\tok\tok')
    T.eq(calls[#calls][2], 35)
    -- A handler that reports failure is answered with its reason.
    T.eq(send('a4', 'hurt\t12'), 'ack\ta4\terror\tno health to read')
    T.eq(send('a5', 'probe'), 'ack\ta5\tok\t1 checks')
    T.ok(store['rt/state.tsv']:find('\nprobe\tworld\tok\tx\n', 1, true), 'probe results are published')
end

function tests.an_empty_or_unfinished_inbox_is_left_alone(T)
    local bridge, store, calls = setup(T)
    bridge:tick()
    T.eq(ack_line(store), nil, 'no command, no answer')
    store['rt/inbox.tsv'] = ''
    bridge:tick()
    store['rt/inbox.tsv'] = 'PBZR1\tc1\theal\t3'      -- the director is still writing
    bridge:tick()
    T.eq(#calls, 0); T.eq(ack_line(store), nil)
    store['rt/inbox.tsv'] = 'PBZR1\tc1\theal\t35\n'   -- now complete
    bridge:tick()
    T.eq(#calls, 1); T.eq(calls[1][2], 35)
end

function tests.a_command_that_cannot_be_read_is_refused_once(T)
    local bridge, store, calls = setup(T)
    store['rt/inbox.tsv'] = 'PBZR1\tc9\tdance\n'
    bridge:tick()
    T.eq(ack_line(store), 'ack\tc9\terror\tunknown action')
    store['rt/inbox.tsv'] = 'PBZR1\tc10\tspawn\tu1\t../etc\t5\t100\t0\t1\n'
    bridge:tick()
    T.eq(ack_line(store), 'ack\tc10\terror\tbad unit id')
    T.eq(#calls, 0, 'a refused command reached the game')
    -- Without a readable id there is nobody to answer.
    store['rt/inbox.tsv'] = 'PBZR1\tbad id\tclear\n'
    bridge:tick()
    T.eq(ack_line(store), 'ack\tc10\terror\tbad unit id')
end

function tests.a_handler_that_raises_is_answered_as_an_error(T)
    local game = {
        ready = function() error('engine object went away') end,
        player = function() error('boom') end,
        units = function() error('boom') end,
        clear = function() error('K2_DestroyActor exploded') end,
    }
    local bridge, store = setup(T, { game = game })
    store['rt/inbox.tsv'] = 'PBZR1\tc1\tclear\n'
    T.ok(bridge:tick(), 'the bridge keeps publishing')
    T.ok(ack_line(store):find('^ack\tc1\terror\t.*K2_DestroyActor exploded'), ack_line(store))
    T.ok(store['rt/state.tsv']:find('\nready\t0\t', 1, true), 'a failing ready() reads as not ready')
    T.eq(store['rt/state.tsv']:find('\nplayer\t', 1, true), nil)
    -- A game object without the method at all.
    store['rt/inbox.tsv'] = 'PBZR1\tc2\theal\t5\n'
    bridge:tick()
    T.eq(ack_line(store), 'ack\tc2\terror\tthe bridge has no heal')
end

function tests.a_reloaded_bridge_does_not_repeat_the_last_command(T)
    local bridge, store, calls = setup(T)
    store['rt/inbox.tsv'] = 'PBZR1\tc1\tspawn\tu1\tBP_X\t5\t100\t0\t1\n'
    bridge:tick()
    T.eq(#calls, 1)
    -- Mods reloaded: a new bridge over the same files, the inbox not yet cleared.
    local _, open = nil, nil
    local reloaded = Bridge.new({ dir = 'rt', game = bridge.game, hud = bridge.hud, keys = bridge.keys,
        open = bridge.open, session = 'second-session' })
    T.eq(reloaded.last_id, 'c1')
    reloaded:tick()
    T.eq(#calls, 1, 'the reloaded bridge summoned a second enemy')
    T.ok(store['rt/state.tsv']:find('\nsession\tsecond-session\n', 1, true))
end

function tests.keys_are_numbered_and_the_last_sixteen_kept(T)
    local bridge, store, _, press = setup(T)
    press('1'); press('2')
    bridge:tick()
    T.ok(store['rt/state.tsv']:find('\nkey\t1\t1\nkey\t2\t2\n', 1, true))
    bridge:tick()
    T.ok(store['rt/state.tsv']:find('\nkey\t1\t1\nkey\t2\t2\n', 1, true), 'keys stay listed for a late reader')
    for _ = 1, 20 do press('3') end
    bridge:tick()
    local count, first = 0, nil
    for seq in store['rt/state.tsv']:gmatch('\nkey\t(%d+)\t') do
        count = count + 1
        first = first or tonumber(seq)
    end
    T.eq(count, 16); T.eq(first, 7)
    T.ok(store['rt/state.tsv']:find('\nkey\t22\t3\n', 1, true))
end

function tests.the_state_file_is_complete_every_tick(T)
    local bridge, store = setup(T)
    for tick = 1, 3 do
        T.ok(bridge:tick())
        local text = store['rt/state.tsv']
        local lines = {}
        for line in text:gmatch('[^\n]+') do lines[#lines + 1] = line end
        T.eq(lines[1], 'begin\t' .. tick)
        T.eq(lines[#lines], 'end\t' .. tick .. '\t' .. #lines)
        T.ok(text:find('\nsession\ttest-session\n', 1, true))
        T.ok(text:find('\nready\t1\tready\n', 1, true))
        T.ok(text:find('\nplayer\t500.0\t1000.0\t1\n', 1, true))
        T.ok(text:find('\nunit\tu1\talive\t10.0\t20.0\tBP_X\n', 1, true))
    end
end

function tests.an_unwritable_folder_is_reported_not_fatal(T)
    local messages = {}
    local store, open = T.files()
    store.__readonly = true
    local bridge = Bridge.new({ dir = 'rt', game = {}, hud = {}, keys = {}, open = open, session = 's',
        log = function(m) messages[#messages + 1] = m end })
    T.eq(bridge:tick(), false)
    T.eq(bridge:tick(), false)
    local about_folder = 0
    for _, m in ipairs(messages) do if m:find('cannot write', 1, true) then about_folder = about_folder + 1 end end
    T.eq(about_folder, 1, 'the folder problem is logged once')
    store.__readonly = nil
    T.eq(bridge:tick(), true)
end

function tests.sessions_differ_between_bridges(T)
    local _, open = T.files()
    local a = Bridge.new({ dir = 'rt', game = {}, hud = {}, keys = {}, open = open })
    local b = Bridge.new({ dir = 'rt', game = {}, hud = {}, keys = {}, open = open })
    T.ok(a.session:match('^[%w_%-]+$') and #a.session <= 64, 'a session is a token: ' .. a.session)
    T.ok(a.session ~= b.session, 'two bridges share a session')
end

return tests
