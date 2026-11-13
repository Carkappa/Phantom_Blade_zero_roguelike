-- Key presses, read in the game thread.
--
-- UE4SS runs RegisterKeyBind callbacks on a thread of its own. PhantomGladiator traced four crashes
-- to a key callback and a game-thread callback running Lua at the same moment, so nothing here uses
-- RegisterKeyBind: each bound key's state is read from the player controller on the game thread and
-- a press is the change from up to down.
local Game = require('game')
local valid = Game.valid

local Keys = {}
Keys.__index = Keys

-- options: game, bindings ({ { name = '1', key = 'One' }, ... }), toggle (key name),
-- on_toggle (function), choices_enabled (function -> boolean), log
function Keys.new(options)
    local self = setmetatable({}, Keys)
    self.game = options.game
    self.bindings = options.bindings or {}
    self.toggle = options.toggle
    self.on_toggle = options.on_toggle or function() end
    self.choices_enabled = options.choices_enabled or function() return true end
    self.log = options.log or function() end
    self.fkeys = {}
    self.down = {}      -- key name -> was down at the last look (nil: not looked at yet)
    self.queue = {}
    self.failures = 0
    return self
end

function Keys:fkey(name)
    if not self.fkeys[name] then self.fkeys[name] = { KeyName = FName(name) } end
    return self.fkeys[name]
end

-- True when `name` went from up to down since the last look. A key already held when first seen is
-- not a press.
function Keys:pressed(pc, name)
    local ok, now = pcall(function() return pc:IsInputKeyDown(self:fkey(name)) end)
    if not ok or type(now) ~= 'boolean' then
        self.failures = self.failures + 1
        if self.failures == 1 then self.log('IsInputKeyDown cannot be read: ' .. tostring(now)) end
        return false
    end
    local before = self.down[name]
    self.down[name] = now
    return now and before == false
end

-- Looks at every bound key once. Call often (every frame or every few milliseconds).
function Keys:sample()
    local pc = self.game:controller()
    if not valid(pc) then
        -- No controller (loading screen): forget what was held, so nothing fires on return.
        self.down = {}
        return
    end
    if self.toggle and self:pressed(pc, self.toggle) then
        pcall(self.on_toggle)
    end
    local enabled = self.choices_enabled() == true
    for _, binding in ipairs(self.bindings) do
        -- Read even when choices are off, so a key held across the change is not taken for a press.
        if self:pressed(pc, binding.key) and enabled and #self.queue < 16 then
            self.queue[#self.queue + 1] = binding.name
        end
    end
end

-- The presses since the last call, oldest first.
function Keys:poll()
    local out = self.queue
    self.queue = {}
    return out
end

return Keys
