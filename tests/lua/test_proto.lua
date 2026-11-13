local P = require('proto')
local tests = {}

function tests.escape_round_trips(T)
    for _, text in ipairs({ '', 'plain', 'tab\there', 'two\nlines\r\n', 'back\\slash and \\n literal',
        '中文 名字', '\\', '\\\\t' }) do
        local escaped = P.escape(text)
        T.ok(not escaped:find('[\t\n\r]'), 'escaped text keeps a control character')
        T.eq(P.unescape(escaped), text, 'round trip')
    end
    T.eq(P.escape(nil), '')
    T.eq(P.escape(12), '12')
    T.eq(P.unescape('trailing\\'), 'trailing')
end

-- The same lines the director's tests encode (tests/golden/commands.tsv).
function tests.golden_commands_parse(T)
    local lines = {}
    for line in T.read('tests/golden/commands.tsv'):gmatch('[^\n]*\n') do lines[#lines + 1] = line end
    T.eq(#lines, 8, 'golden command count')

    local spawn = assert(P.parse_command(lines[1]))
    T.eq(spawn.id, 'd1-5'); T.eq(spawn.action, 'spawn'); T.eq(spawn.handle, 'u7')
    T.eq(spawn.unit_id, 'BP_Monster_MoTian'); T.eq(spawn.level, 12); T.eq(spawn.health_pct, 90)
    T.eq(spawn.slot, 0); T.eq(spawn.count, 1)
    T.eq(assert(P.parse_command(lines[2])).action, 'clear')
    local heal = assert(P.parse_command(lines[3]))
    T.eq(heal.action, 'heal'); T.eq(heal.pct, 35)
    local hurt = assert(P.parse_command(lines[4]))
    T.eq(hurt.action, 'hurt'); T.eq(hurt.pct, 12)
    T.eq(assert(P.parse_command(lines[5])).action, 'probe')
    local screen = assert(P.parse_command(lines[6]))
    T.eq(screen.visible, true); T.eq(screen.text, 'Title\n 1. A\tB \\ end')
    local hello = assert(P.parse_command(lines[7]))
    T.eq(hello.action, 'hello'); T.eq(hello.version, '0.1.0')
    local hidden = assert(P.parse_command(lines[8]))
    T.eq(hidden.visible, false); T.eq(hidden.text, '')
    -- A file written with Windows line ends still parses.
    T.eq(assert(P.parse_command('PBZR1\ta\theal\t35\r\n')).pct, 35)
end

function tests.nothing_to_do_is_not_an_error(T)
    local cmd, why, id = P.parse_command('')
    T.eq(cmd, nil); T.eq(why, 'empty'); T.eq(id, nil)
    cmd, why = P.parse_command(nil)
    T.eq(why, 'empty')
    cmd, why = P.parse_command('\n')
    T.eq(why, 'empty')
    -- Caught while the director was writing: no final newline yet.
    cmd, why, id = P.parse_command('PBZR1\ta\theal\t3')
    T.eq(cmd, nil); T.eq(why, 'incomplete'); T.eq(id, nil)
end

function tests.every_prefix_of_a_command_is_refused(T)
    local line = 'PBZR1\td1-5\tspawn\tu7\tBP_Monster_MoTian\t12\t90\t0\t1\n'
    for cut = 0, #line - 1 do
        T.eq(P.parse_command(line:sub(1, cut)), nil, 'a command cut at ' .. cut .. ' parsed')
    end
    T.ok(P.parse_command(line))
end

function tests.bad_commands_are_refused_with_their_id(T)
    local function refused(line, expect_id)
        local cmd, why, id = P.parse_command(line)
        T.eq(cmd, nil, 'parsed: ' .. line)
        T.ok(type(why) == 'string' and why ~= 'empty' and why ~= 'incomplete', 'no reason for: ' .. line)
        T.eq(id, expect_id, 'id of: ' .. line)
    end
    refused('OTHER\ta\tclear\n', 'a')
    refused('PBZR1\ta\tdance\n', 'a')
    refused('PBZR1\ta\tclear\textra\n', 'a')
    refused('PBZR1\ta\theal\n', 'a')
    refused('PBZR1\ta\theal\t0\n', 'a')
    refused('PBZR1\ta\theal\t101\n', 'a')
    refused('PBZR1\ta\theal\t3.5\n', 'a')
    refused('PBZR1\ta\theal\t-3\n', 'a')
    refused('PBZR1\ta\tscreen\t2\ttext\n', 'a')
    refused('PBZR1\tbad id\tclear\n', nil)
    refused('PBZR1\t\tclear\n', nil)
    refused('PBZR1\ta\tclear\nPBZR1\tb\tclear\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\t../x\t5\t100\t0\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP Monster\t5\t100\t0\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu 1\tBP_X\t5\t100\t0\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP_X\t0\t100\t0\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP_X\t5\t0\t0\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP_X\t5\t100\t1\t1\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP_X\t5\t100\t0\t0\n', 'a')
    refused('PBZR1\ta\tspawn\tu1\tBP_X\t5\t100\t0\t17\n', 'a')
    refused('PBZR1\ta\tscreen\t1\t' .. string.rep('x', P.MAX_COMMAND) .. '\n', 'a')
end

local SNAPSHOT = {
    tick = 41, session = 's-9', ready = true, reason = 'ready',
    player = { hp = 812.5, max = 1000, alive = true },
    ack = { id = 'd1-5', ok = true, message = 'summoned' },
    units = {
        { handle = 'u7', status = 'alive', hp = 5000, max = 9000, unit_id = 'BP_Monster_MoTian' },
        { handle = 'u8', status = 'dead', hp = 0, max = 800, unit_id = 'BP_Monster_Dog' },
        { handle = 'u9', status = 'gone', hp = 12.3, max = 800, unit_id = 'BP_Monster_Dog' },
    },
    keys = { { seq = 3, name = '1' }, { seq = 4, name = '2' } },
    probes = { { name = 'world', status = 'ok', detail = 'World /Game/Map' },
        { name = 'codex', status = 'fail', detail = 'two\nlines' } },
}

-- The director's tests parse this same file (tests/golden/state.tsv).
function tests.state_matches_the_golden_file(T)
    T.eq(P.format_state(SNAPSHOT), T.read('tests/golden/state.tsv'), 'state text')
end

function tests.state_is_framed_and_survives_odd_values(T)
    local nan = 0 / 0
    local text = P.format_state({ tick = 7, session = 'has space', ready = false,
        player = { hp = nan, max = math.huge, alive = false },
        ack = { id = 'bad id', ok = true },
        units = { { handle = 'u1', status = 'sleeping', hp = 1, max = 1 }, { handle = 'u2', status = 'alive', hp = nan, max = 5 } },
        keys = { { seq = 1, name = 'bad name' }, { seq = 2.0, name = '3' } } })
    local lines = {}
    for line in text:gmatch('[^\n]+') do lines[#lines + 1] = line end
    T.eq(lines[1], 'begin\t7')
    T.eq(lines[#lines], 'end\t7\t' .. #lines, 'the end line counts every line')
    T.eq(lines[3], 'session\tunknown', 'a session that is not a token is not sent')
    T.eq(lines[4], 'ready\t0\t')
    T.eq(lines[5], 'player\t0.0\t0.0\t0', 'NaN and infinity become 0.0')
    T.eq(lines[6], 'unit\tu2\talive\t0.0\t5.0\t', 'a unit with an unknown status is left out')
    T.eq(lines[7], 'key\t2\t3', 'a key sequence is a whole number')
    T.eq(#lines, 8)
    T.eq(text:sub(-1), '\n')
    -- A state with nothing optional.
    local bare = P.format_state({ tick = 1, session = 's' })
    T.eq(bare, 'begin\t1\nproto\t1\t' .. P.VERSION .. '\nsession\ts\nready\t0\t\nend\t1\t5\n')
end

function tests.last_ack_id_reads_a_state_file(T)
    T.eq(P.last_ack_id(P.format_state(SNAPSHOT)), 'd1-5')
    T.eq(P.last_ack_id(P.format_state({ tick = 1, session = 's' })), nil)
    T.eq(P.last_ack_id(nil), nil)
    T.eq(P.last_ack_id(''), nil)
    T.eq(P.last_ack_id('garbage\n\0\nack\t\n'), nil)
end

tests.SNAPSHOT = nil
return setmetatable(tests, { __index = { snapshot = SNAPSHOT } })
