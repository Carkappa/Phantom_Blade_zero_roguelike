-- PBZ Roguelike: the in-game bridge. UE4SS runs this file when the game starts.
--
-- The run itself (map, rewards, what to summon) is decided by pbz-roguelike.exe, the director. This
-- side only does what it is told, in the game thread, and reports what it sees. See README.md.
local function script_dir()
    local source = debug.getinfo(1, 'S').source or ''
    if source:sub(1, 1) == '@' then source = source:sub(2) end
    source = source:gsub('\\', '/')
    return source:match('^(.*)/[^/]*$') or '.'
end

local DIR = script_dir()
package.path = DIR .. '/?.lua;' .. package.path

local Config = require('config')
local Proto = require('proto')
local Game = require('game')
local Hud = require('hud')
local Keys = require('keys')
local Bridge = require('bridge')

local TAG = '[PBZRoguelike] '
local RUNTIME = Config.RuntimePath or (DIR:gsub('/[^/]*$', '') .. '/Runtime')

local log_bytes = 0
local function log(message)
    local line = TAG .. tostring(message) .. '\n'
    print(line)
    -- UE4SS.log is overwritten on every start; keep this session's lines beside the bridge files.
    if log_bytes > 2 * 1024 * 1024 then return end
    pcall(function()
        local file = io.open(RUNTIME .. '/bridge.log', 'ab')
        if not file then return end
        local stamped = os.date('%Y-%m-%d %H:%M:%S ') .. line
        file:write(stamped)
        file:close()
        log_bytes = log_bytes + #stamped
    end)
end

local helpers
do
    local ok, module = pcall(require, 'UEHelpers')
    if ok and type(module) == 'table' then helpers = module end
end

local game = Game.new({ config = Config, log = log, helpers = helpers })
local hud = Hud.new({ game = game, config = Config, log = log })

local bindings = {}
for index, key in ipairs(Config.ChoiceKeys or {}) do
    if index <= 9 then bindings[#bindings + 1] = { name = tostring(index), key = key } end
end
local keys = Keys.new({
    game = game,
    bindings = bindings,
    toggle = Config.ToggleKey,
    on_toggle = function() hud:toggle() end,
    choices_enabled = function() return hud:visible() end,
    log = log,
})
local bridge = Bridge.new({ dir = RUNTIME, game = game, hud = hud, keys = keys, log = log })

local key_interval = math.max(8, tonumber(Config.KeyIntervalMs) or 16)
local bridge_every = math.max(1, math.floor((tonumber(Config.BridgeIntervalMs) or 100) / key_interval + 0.5))
local steps, errors = 0, 0

-- One step in the game thread: read the keys; every few steps, keep the panel alive and talk to
-- the director.
local function step()
    local ok, why = pcall(function()
        steps = steps + 1
        keys:sample()
        if steps % bridge_every == 0 then
            hud:apply()
            bridge:tick()
        end
    end)
    if not ok then
        errors = errors + 1
        if errors <= 20 then log('step failed: ' .. tostring(why)) end
    end
end

local mode
if type(LoopInGameThreadWithDelay) == 'function' then
    local ok, handle = pcall(LoopInGameThreadWithDelay, key_interval, step)
    if ok and handle ~= nil and handle ~= false then mode = 'LoopInGameThreadWithDelay' end
end
if not mode and type(LoopAsync) == 'function' and type(ExecuteInGameThread) == 'function' then
    -- Older UE4SS: a timer thread that only queues the step for the game thread, one at a time.
    local queued = false
    LoopAsync(key_interval, function()
        if not queued then
            queued = true
            ExecuteInGameThread(function()
                queued = false
                step()
            end)
        end
        return false
    end)
    mode = 'LoopAsync + ExecuteInGameThread'
end

if mode then
    log('bridge ' .. Proto.VERSION .. ' started; loop=' .. mode .. '; runtime=' .. RUNTIME .. '; session=' .. bridge.session)
else
    log('bridge NOT started: this UE4SS has neither LoopInGameThreadWithDelay nor LoopAsync/ExecuteInGameThread')
end
