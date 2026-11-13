-- The command loop: reads the director's inbox, runs each command once, and writes the state file.
-- It makes no engine calls of its own. Everything that touches the game is in the `game`, `hud` and
-- `keys` objects it is given, so this file is tested outside the game (tests/lua).
--
-- tick() must be called from the game thread.
local P = require('proto')

local Bridge = {}
Bridge.__index = Bridge

local KEEP_KEYS = 16

local function read_all(open, path)
    local file = open(path, 'rb')
    if not file then return nil end
    local ok, content = pcall(function() return file:read('*a') end)
    pcall(function() file:close() end)
    return ok and content or nil
end

-- options: dir, game, hud, keys; optional open (io.open), log, session, now (os.time)
function Bridge.new(options)
    local self = setmetatable({}, Bridge)
    self.dir = options.dir
    self.game = options.game
    self.hud = options.hud
    self.keys = options.keys
    self.open = options.open or io.open
    self.log = options.log or function() end
    self.inbox = self.dir .. '/inbox.tsv'
    self.state = self.dir .. '/state.tsv'
    self.tick_count = 0
    self.key_seq = 0
    self.key_log = {}
    self.probes = {}
    self.ack = nil
    self.write_failed = false
    local now = options.now or os.time
    -- A new token each time the bridge starts: the director then knows its enemies are gone.
    self.session = options.session
        or ('s' .. string.format('%.0f', now()) .. '-' .. string.format('%.0f', (os.clock() * 1000000) % 1000000)
            .. '-' .. tostring(math.random(100000, 999999)))
    -- After a mod reload the inbox may still hold a command the previous bridge already ran.
    self.last_id = P.last_ack_id(read_all(self.open, self.state))
    return self
end

local function call(target, method, ...)
    if not target or type(target[method]) ~= 'function' then
        return false, 'the bridge has no ' .. method
    end
    local results = { pcall(target[method], target, ...) }
    if not results[1] then return false, tostring(results[2]) end
    return results[2], results[3]
end

function Bridge:run(cmd)
    local action = cmd.action
    if action == 'hello' then
        self.director_version = cmd.version
        return true, P.VERSION
    elseif action == 'screen' then
        return call(self.hud, 'show', cmd.visible, cmd.text)
    elseif action == 'spawn' then
        return call(self.game, 'spawn', cmd.handle, cmd.unit_id, cmd.level, cmd.health_pct, cmd.slot, cmd.count)
    elseif action == 'clear' then
        return call(self.game, 'clear')
    elseif action == 'heal' then
        return call(self.game, 'heal', cmd.pct)
    elseif action == 'hurt' then
        return call(self.game, 'hurt', cmd.pct)
    elseif action == 'probe' then
        local probes, why = call(self.game, 'probe')
        if type(probes) ~= 'table' then return false, why or 'the probe returned nothing' end
        self.probes = probes
        return true, tostring(#probes) .. ' checks'
    end
    return false, 'unknown action'
end

-- Reads the inbox and runs the command in it, unless it was run before.
function Bridge:receive()
    local cmd, why, id = P.parse_command(read_all(self.open, self.inbox))
    if not cmd then
        -- A command that cannot be understood is answered once, so the director does not wait.
        if id and id ~= self.last_id then
            self.last_id = id
            self.ack = { id = id, ok = false, message = why }
            self.log('refused command ' .. id .. ': ' .. tostring(why))
        end
        return
    end
    if cmd.id == self.last_id then return end
    self.last_id = cmd.id
    local ok, message = self:run(cmd)
    self.ack = { id = cmd.id, ok = ok == true, message = message and tostring(message) or (ok == true and 'ok' or 'failed') }
    if ok ~= true then self.log(cmd.action .. ' failed: ' .. self.ack.message) end
end

function Bridge:collect_keys()
    local pressed = call(self.keys, 'poll')
    if type(pressed) ~= 'table' then return end
    for _, name in ipairs(pressed) do
        self.key_seq = self.key_seq + 1
        self.key_log[#self.key_log + 1] = { seq = self.key_seq, name = name }
    end
    while #self.key_log > KEEP_KEYS do table.remove(self.key_log, 1) end
end

function Bridge:snapshot()
    local ready, reason = call(self.game, 'ready')
    local player = call(self.game, 'player')
    local units = call(self.game, 'units')
    return {
        tick = self.tick_count,
        session = self.session,
        ready = ready == true,
        reason = reason or (ready == true and 'ready' or 'not ready'),
        player = type(player) == 'table' and player or nil,
        ack = self.ack,
        units = type(units) == 'table' and units or {},
        keys = self.key_log,
        probes = self.probes,
    }
end

function Bridge:publish()
    local text = P.format_state(self:snapshot())
    local file = self.open(self.state, 'wb')
    if not file then
        if not self.write_failed then
            self.write_failed = true
            self.log('cannot write ' .. self.state .. ' (does the Runtime folder exist?)')
        end
        return false
    end
    local ok = pcall(function() file:write(text) end)
    pcall(function() file:close() end)
    if ok and self.write_failed then self.write_failed = false end
    return ok
end

-- One step: keys, at most one command, then the state file.
function Bridge:tick()
    self.tick_count = self.tick_count + 1
    self:collect_keys()
    self:receive()
    return self:publish()
end

return Bridge
