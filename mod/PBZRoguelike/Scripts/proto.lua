-- The wire format between the director (pbz-roguelike.exe) and this bridge.
-- The Rust side is crates/director/src/protocol.rs; tests/lua checks both against the same lines.
-- No engine calls here: this file runs in any Lua from 5.1 to 5.4.
local P = {}

P.PROTO = 'PBZR1'
P.VERSION = '0.1.0'
P.MAX_COMMAND = 16384

local ESCAPES = { ['\\'] = '\\\\', ['\t'] = '\\t', ['\n'] = '\\n', ['\r'] = '\\r' }
local UNESCAPES = { t = '\t', n = '\n', r = '\r', ['\\'] = '\\' }

-- Makes text safe for one tab-separated field.
function P.escape(text)
    return (tostring(text == nil and '' or text):gsub('[\\\t\n\r]', ESCAPES))
end

function P.unescape(text)
    return (text:gsub('\\(.?)', function(c)
        if c == '' then return '' end
        return UNESCAPES[c] or c
    end))
end

-- Splits on tabs and keeps empty fields.
function P.split(line)
    local fields, start = {}, 1
    while true do
        local at = line:find('\t', start, true)
        if not at then
            fields[#fields + 1] = line:sub(start)
            return fields
        end
        fields[#fields + 1] = line:sub(start, at - 1)
        start = at + 1
    end
end

local function token(text, max)
    return type(text) == 'string' and #text >= 1 and #text <= max and text:match('^[%w_%-]+$') ~= nil
end

-- A whole number from 0 to 9999, or nil.
local function int(text)
    if type(text) ~= 'string' or not text:match('^%d%d?%d?%d?$') then return nil end
    return tonumber(text)
end

local ARGS = { hello = 1, screen = 2, spawn = 6, clear = 0, heal = 1, hurt = 1, probe = 0 }

-- Reads the inbox. Returns the command, or nil, a reason and the command id when it could be read
-- (so a command that cannot be run is still answered). 'empty' and 'incomplete' are not errors: the
-- director has nothing to say, or is in the middle of saying it.
function P.parse_command(content)
    if type(content) ~= 'string' or content == '' then return nil, 'empty' end
    if content:sub(-1) ~= '\n' then return nil, 'incomplete' end
    local line = content:sub(1, -2)
    if line:sub(-1) == '\r' then line = line:sub(1, -2) end
    if line == '' then return nil, 'empty' end
    local fields = P.split(line)
    local id = token(fields[2], 64) and fields[2] or nil
    if #content > P.MAX_COMMAND then return nil, 'command too long', id end
    if line:find('[\r\n]') then return nil, 'more than one line', id end
    if fields[1] ~= P.PROTO then return nil, 'not a ' .. P.PROTO .. ' command', id end
    if not id then return nil, 'bad command id' end
    local action = fields[3]
    local wanted = ARGS[action]
    if not wanted then return nil, 'unknown action', id end
    if #fields ~= 3 + wanted then return nil, 'wrong number of fields for ' .. action, id end

    local cmd = { id = id, action = action }
    if action == 'hello' then
        cmd.version = P.unescape(fields[4])
    elseif action == 'screen' then
        if fields[4] ~= '0' and fields[4] ~= '1' then return nil, 'bad visible flag', id end
        cmd.visible = fields[4] == '1'
        cmd.text = P.unescape(fields[5])
    elseif action == 'spawn' then
        cmd.handle = fields[4]
        cmd.unit_id = fields[5]
        cmd.level, cmd.health_pct = int(fields[6]), int(fields[7])
        cmd.slot, cmd.count = int(fields[8]), int(fields[9])
        if not token(cmd.handle, 32) then return nil, 'bad handle', id end
        if #cmd.unit_id < 1 or #cmd.unit_id > 96 or not cmd.unit_id:match('^[%w_]+$') then
            return nil, 'bad unit id', id
        end
        if not cmd.level or cmd.level < 1 or cmd.level > 200 then return nil, 'bad level', id end
        if not cmd.health_pct or cmd.health_pct < 1 or cmd.health_pct > 100 then return nil, 'bad health', id end
        if not cmd.slot or not cmd.count or cmd.count < 1 or cmd.count > 16 or cmd.slot >= cmd.count then
            return nil, 'bad slot', id
        end
    elseif action == 'heal' or action == 'hurt' then
        cmd.pct = int(fields[4])
        if not cmd.pct or cmd.pct < 1 or cmd.pct > 100 then return nil, 'bad percentage', id end
    end
    return cmd
end

local function num(value)
    if type(value) ~= 'number' or value ~= value or value == math.huge or value == -math.huge then
        return '0.0'
    end
    return string.format('%.1f', value)
end

local function whole(value)
    if type(value) ~= 'number' or value ~= value or value < 0 or value >= 2 ^ 53 then return '0' end
    return string.format('%.0f', math.floor(value))
end

local STATUS = { alive = true, dead = true, gone = true }

-- Builds state.tsv. `s` holds: tick, session, ready, reason, player {hp, max, alive}, ack {id, ok,
-- message}, units {{handle, status, hp, max, unit_id}}, keys {{seq, name}}, probes {{name, status,
-- detail}}. Everything but tick and session may be missing.
function P.format_state(s)
    local tick = whole(s.tick)
    local lines = {
        'begin\t' .. tick,
        'proto\t1\t' .. P.escape(P.VERSION),
        'session\t' .. (token(s.session, 64) and s.session or 'unknown'),
        'ready\t' .. (s.ready and '1' or '0') .. '\t' .. P.escape(s.reason or ''),
    }
    if s.player then
        lines[#lines + 1] = table.concat({ 'player', num(s.player.hp), num(s.player.max), s.player.alive and '1' or '0' }, '\t')
    end
    if s.ack and token(s.ack.id, 64) then
        lines[#lines + 1] = table.concat({ 'ack', s.ack.id, s.ack.ok and 'ok' or 'error', P.escape(s.ack.message or '') }, '\t')
    end
    for _, unit in ipairs(s.units or {}) do
        if token(unit.handle, 32) and STATUS[unit.status] then
            lines[#lines + 1] = table.concat({ 'unit', unit.handle, unit.status, num(unit.hp), num(unit.max),
                P.escape(unit.unit_id or '') }, '\t')
        end
    end
    for _, key in ipairs(s.keys or {}) do
        if token(key.name, 32) then
            lines[#lines + 1] = table.concat({ 'key', whole(key.seq), key.name }, '\t')
        end
    end
    for _, probe in ipairs(s.probes or {}) do
        if token(probe.name, 48) then
            local status = (probe.status == 'ok' or probe.status == 'fail') and probe.status or 'skip'
            lines[#lines + 1] = table.concat({ 'probe', probe.name, status, P.escape(probe.detail or '') }, '\t')
        end
    end
    lines[#lines + 1] = 'end\t' .. tick .. '\t' .. tostring(#lines + 1)
    return table.concat(lines, '\n') .. '\n'
end

-- The id of the last command a state file answered, or nil.
function P.last_ack_id(state_text)
    if type(state_text) ~= 'string' then return nil end
    for line in state_text:gmatch('[^\n]+') do
        local id = line:match('^ack\t([%w_%-]+)\t')
        if id then return id end
    end
    return nil
end

return P
