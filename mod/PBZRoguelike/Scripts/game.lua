-- Everything that touches the game: finding the player, summoning and removing enemies, reading and
-- writing health. All of it must run in the game thread.
--
-- The engine calls here are the ones the open-source PhantomGladiator mod (MIT) uses and tested in
-- the game: the AI library's SpawnAIDefered / FinishSpawnAIDefered / ActivateAI, the `Level`
-- property written before the spawn finishes, health in AttributeSetBase, and the codex subsystem for
-- class paths. This file has not itself been run in the game: `probe` reports which calls work.
--
-- What the mod writes into the game: enemies it summons (removed again by `clear`), and current
-- health of the player and of those enemies. It never writes story progress, items, or character
-- stats, and never touches a save file.
local Game = {}
Game.__index = Game

local AI_LIBRARIES = {
    '/SGameAI/BP_SGameAIFunctionLibrary.Default__BP_SGameAIFunctionLibrary_C',
    '/SGameAI/Utils/BP_SGameAIFunctionLibrary.Default__BP_SGameAIFunctionLibrary_C',
}

-- Units never summoned, whatever the director asks. Each of these writes story progress when it is
-- fought (PhantomGladiator, story_writes.lua) or needs a mount this mod does not provide. The
-- director's roster excludes them as well; this is the second lock.
local DENY = {
    'HorseCartCombat', 'Rider', '^BP_Monster_LeiDang', '^BP_Monster_LengTu', '^BP_Monster_LeiHuang$',
    '^BP_Monster_LeiMing$', '_NPC$', '^BP_Monster_ZuoShang$', '^BP_Monster_ZuoShang_Young_Tutorial$',
    '^BP_Monster_MuXiaoKui_P2$', '^BP_Monster_MoTian_LongZhu_01$', '^BP_Monster_JingYe$',
    '^BP_Monster_ChangHong_P2$',
}

local function valid(obj)
    if obj == nil then return false end
    local ok, value = pcall(function() return obj:IsValid() end)
    return ok and value == true
end

local function kind(value)
    if type(value) ~= 'table' and type(value) ~= 'userdata' then return type(value) end
    local ok, name = pcall(function() return value:type() end)
    return ok and type(name) == 'string' and name or type(value)
end

-- Out-parameters and array elements arrive wrapped.
local function unwrap(value)
    local name = kind(value)
    if name == 'RemoteUnrealParam' or name == 'LocalUnrealParam' then
        local ok, inner = pcall(function() return value:get() end)
        return ok and inner or nil
    end
    return value
end

local function field(obj, name)
    if not valid(obj) then return nil end
    local ok, value = pcall(function() return obj[name] end)
    if ok then return unwrap(value) end
    return nil
end

local function member(struct, name)
    struct = unwrap(struct)
    if struct == nil then return nil end
    local ok, value = pcall(function() return struct[name] end)
    if ok then return unwrap(value) end
    return nil
end

local function key(obj)
    if not valid(obj) then return nil end
    local ok, address = pcall(function() return obj:GetAddress() end)
    return ok and address ~= nil and tostring(address) or nil
end

local function number(value)
    if type(value) == 'number' and value == value and value ~= math.huge and value ~= -math.huge then
        return value
    end
    return nil
end

local function text(value)
    value = unwrap(value)
    if type(value) == 'string' then return value end
    if value == nil then return nil end
    local ok, result = pcall(function() return value:ToString() end)
    return ok and type(result) == 'string' and result or nil
end

local function full_name(obj)
    if not valid(obj) then return nil end
    local ok, name = pcall(function() return obj:GetFullName() end)
    return ok and type(name) == 'string' and name or nil
end

local function destroying(actor)
    local ok, value = pcall(function() return actor:IsActorBeingDestroyed() end)
    return not ok or value ~= false
end

local function live(actor)
    return valid(actor) and not destroying(actor)
end

-- A TArray or a Lua table as a plain list.
local function items(value, limit)
    value = unwrap(value)
    local name, out = kind(value), {}
    if name == 'table' then
        for _, item in ipairs(value) do
            if #out >= limit then break end
            out[#out + 1] = unwrap(item)
        end
        return out
    end
    if name ~= 'TArray' then return nil end
    local ok = pcall(function()
        value:ForEach(function(_, item)
            if #out < limit then out[#out + 1] = unwrap(item) end
        end)
    end)
    return ok and out or nil
end

-- "/Package/Path/Asset.Asset_C" and nothing else.
local function class_path(value)
    if type(value) ~= 'string' or #value > 512 then return nil end
    if not value:match('^/[%w_/%-%.]+$') or value:find('..', 1, true) or value:find('//', 1, true) then
        return nil
    end
    local asset, name = value:match('/([%w_%-]+)%.([%w_%-]+)_C$')
    if not asset or asset ~= name then return nil end
    return value, name
end

local function atan2(y, x)
    if x > 0 then return math.atan(y / x) end
    if x < 0 then return math.atan(y / x) + (y >= 0 and math.pi or -math.pi) end
    if y > 0 then return math.pi / 2 end
    if y < 0 then return -math.pi / 2 end
    return 0
end

-- options: config, log; helpers (the UEHelpers module, optional)
function Game.new(options)
    local self = setmetatable({}, Game)
    self.config = options.config or {}
    self.log = options.log or function() end
    self.helpers = options.helpers
    self.spawned = {}       -- handle -> { actor, unit_id, hp, max, dead, last_share, pending_pct }
    self.order = {}         -- handles in the order they were summoned
    self.paths = {}         -- unit id -> class path, from the game codex
    self.path_count = 0
    self.catalog_at = nil
    self.library = nil
    return self
end

function Game.is_denied(unit_id)
    for _, pattern in ipairs(DENY) do
        if unit_id:find(pattern) then return true end
    end
    return false
end

function Game:controller()
    local ok, list = pcall(FindAllOf, 'PlayerController')
    if ok and type(list) == 'table' then
        for _, pc in ipairs(list) do
            if valid(pc) then
                local got, cls = pcall(function() return pc:GetClass() end)
                local class_name = got and full_name(cls) or ''
                -- A controller kept from an earlier map stays valid; the current one owns its player.
                local owner = field(field(pc, 'Player'), 'PlayerController')
                if not class_name:find('DebugCameraController', 1, true) and key(owner) ~= nil
                    and key(owner) == key(pc) then
                    return pc
                end
            end
        end
    end
    return nil
end

function Game:world()
    local helpers = self.helpers
    if helpers and type(helpers.GetGameViewportClient) == 'function' then
        local ok, viewport = pcall(helpers.GetGameViewportClient)
        if ok and valid(viewport) then
            local world = field(viewport, 'World')
            if valid(world) then return world end
        end
    end
    local pc = self:controller()
    if pc then
        local ok, world = pcall(function() return pc:GetWorld() end)
        if ok and valid(world) then return world end
    end
    return nil
end

function Game:pawn()
    local pawn = field(self:controller(), 'Pawn')
    if live(pawn) then return pawn end
    return nil
end

local function attribute(actor, name)
    local set = field(actor, 'AttributeSetBase')
    if not valid(set) then return nil end
    return number(member(field(set, name), 'CurrentValue'))
end

local function close(a, b)
    return a ~= nil and math.abs(a - b) <= math.max(0.5, math.abs(b) * 0.001)
end

-- Writes current health and reads it back.
local function set_health(actor, value)
    local set = field(actor, 'AttributeSetBase')
    if not valid(set) then return false, 'no attribute set' end
    pcall(function()
        local data = set.Health
        data.BaseValue = value
        data.CurrentValue = value
    end)
    if close(attribute(actor, 'Health'), value) then return true end
    pcall(function() set.Health = { BaseValue = value, CurrentValue = value } end)
    if close(attribute(actor, 'Health'), value) then return true end
    return false, 'the health write did not read back'
end

function Game:ready()
    if not valid(self:world()) then return false, 'no map is loaded' end
    local pawn = self:pawn()
    if not pawn then return false, 'no player character' end
    local hp = attribute(pawn, 'Health')
    if not hp then return false, 'the player health cannot be read' end
    if hp <= 0 then return false, 'the player is down' end
    return true, 'ready'
end

function Game:player()
    local pawn = self:pawn()
    if not pawn then return nil end
    local hp, max = attribute(pawn, 'Health'), attribute(pawn, 'Health_Max')
    if not hp then return nil end
    return { hp = hp, max = max or 0, alive = hp > 0 }
end

function Game:heal(pct)
    local pawn = self:pawn()
    if not pawn then return false, 'no player character' end
    local hp, max = attribute(pawn, 'Health'), attribute(pawn, 'Health_Max')
    if not hp or not max or max <= 0 then return false, 'the player health cannot be read' end
    if hp <= 0 then return false, 'the player is down' end
    local target = math.min(max, hp + max * pct / 100)
    if target <= hp then return true, 'already at full health' end
    return set_health(pawn, target)
end

-- Takes health and always leaves at least one point: an event never kills.
function Game:hurt(pct)
    local pawn = self:pawn()
    if not pawn then return false, 'no player character' end
    local hp, max = attribute(pawn, 'Health'), attribute(pawn, 'Health_Max')
    if not hp or not max or max <= 0 then return false, 'the player health cannot be read' end
    if hp <= 1 then return true, 'no health to take' end
    return set_health(pawn, math.max(1, hp - max * pct / 100))
end

function Game:ai_library()
    if valid(self.library) then return self.library end
    for _, path in ipairs(AI_LIBRARIES) do
        local ok, obj = pcall(StaticFindObject, path)
        if ok and valid(obj) then
            self.library = obj
            return obj
        end
    end
    if type(FindObject) == 'function' then
        local ok, obj = pcall(FindObject, 'BP_SGameAIFunctionLibrary_C', 'Default__BP_SGameAIFunctionLibrary_C')
        if ok and valid(obj) then
            self.library = obj
            return obj
        end
    end
    return nil
end

-- The class path behind a codex reference (a soft class pointer), without loading anything.
local function soft_path(reference, kismet)
    reference = unwrap(reference)
    if reference == nil then return nil end
    if kind(reference) == 'TSoftObjectPtrUserdata' then
        local ok, id = pcall(function() return reference:GetObjectID() end)
        if ok and id ~= nil then
            local named, asset = pcall(function() return id:GetAssetPathName() end)
            local subbed, sub = pcall(function() return id:GetSubPathString() end)
            local sub_text = subbed and text(sub) or nil
            if named and (sub_text == nil or sub_text == '') then
                local path, name = class_path(text(asset))
                if path then return path, name end
            end
        end
    end
    if valid(kismet) then
        local ok, value = pcall(function() return kismet:Conv_SoftClassReferenceToString(reference) end)
        if ok then
            local path, name = class_path(text(value))
            if path then return path, name end
        end
    end
    return nil
end

-- Reads every enemy class path from the game's own codex. Read-only: reveals nothing, loads nothing.
function Game:load_catalog()
    local world = self:world()
    local ok, list = pcall(FindAllOf, 'PBZCodexSubsystem')
    if not ok or type(list) ~= 'table' then return 0, 'the codex subsystem was not found' end
    local codex
    for _, candidate in ipairs(list) do
        if valid(candidate) then
            local got, outer = pcall(function() return candidate:GetOuter() end)
            if got and valid(outer) and key(outer) == key(world) then
                codex = candidate
                break
            end
            codex = codex or candidate
        end
    end
    if not valid(codex) then return 0, 'the codex subsystem was not found' end
    local called, entries = pcall(function() return codex:GetAllEnemyCodexEntries() end)
    if not called then return 0, 'GetAllEnemyCodexEntries failed: ' .. tostring(entries) end
    local rows = items(entries, 4096)
    if not rows then return 0, 'the codex entries could not be read' end
    local found, kismet = pcall(StaticFindObject, '/Script/Engine.Default__KismetSystemLibrary')
    if not found then kismet = nil end

    local paths, count = {}, 0
    local function add(reference)
        local path, name = soft_path(reference, kismet)
        if path and not paths[name] then
            paths[name] = path
            count = count + 1
        end
    end
    for _, row in ipairs(rows) do
        add(member(row, 'EnemyClass'))
        add(member(row, 'ReplacedByEnemyClass'))
        local aliases = items(member(row, 'AliasEnemyClasses'), 64)
        for _, alias in ipairs(aliases or {}) do add(alias) end
    end
    if count > 0 then
        self.paths, self.path_count = paths, count
    end
    return count, count > 0 and 'ok' or 'the codex gave no class paths'
end

function Game:path_for(unit_id)
    local path = self.paths[unit_id]
    if path then return path end
    -- Not known yet: read the codex, at most once every few seconds.
    local now = os.clock()
    if self.catalog_at == nil or now - self.catalog_at >= 3 or now < self.catalog_at then
        self.catalog_at = now
        local count, why = self:load_catalog()
        self.log('codex read: ' .. tostring(count) .. ' class paths (' .. tostring(why) .. ')')
    end
    return self.paths[unit_id]
end

function Game:class_for(unit_id)
    local path = self:path_for(unit_id)
    if not path then return nil, unit_id .. ' is not in the game codex' end
    local ok, cls = pcall(StaticFindObject, path)
    if ok and valid(cls) then return cls end
    local loaded, asset = pcall(LoadAsset, path)
    ok, cls = pcall(StaticFindObject, path)
    if ok and valid(cls) then return cls end
    if loaded and valid(asset) then return asset end
    return nil, unit_id .. ' could not be loaded'
end

local function vector(value)
    value = unwrap(value)
    local x, y, z = number(member(value, 'X')), number(member(value, 'Y')), number(member(value, 'Z'))
    if x and y and z then return { X = x, Y = y, Z = z } end
    return nil
end

-- Where slot `slot` of `count` stands: in front of the player, side by side, facing the player.
function Game:spawn_transform(pawn, slot, count)
    local got_at, at = pcall(function() return pawn:K2_GetActorLocation() end)
    local got_forward, forward = pcall(function() return pawn:GetActorForwardVector() end)
    at, forward = got_at and vector(at) or nil, got_forward and vector(forward) or nil
    if not at or not forward then return nil end
    local length = math.sqrt(forward.X * forward.X + forward.Y * forward.Y)
    local fx, fy = 1, 0
    if length > 0.001 then fx, fy = forward.X / length, forward.Y / length end
    local distance = self.config.SummonDistance or 600
    local spacing = self.config.SummonSpacing or 250
    local side = (slot - (count - 1) / 2) * spacing
    local yaw = atan2(-fy, -fx)
    return {
        Rotation = { X = 0, Y = 0, Z = math.sin(yaw / 2), W = math.cos(yaw / 2) },
        Translation = {
            X = at.X + fx * distance - fy * side,
            Y = at.Y + fy * distance + fx * side,
            Z = at.Z + (self.config.SummonHeight or 50),
        },
        Scale3D = { X = 1, Y = 1, Z = 1 },
    }
end

function Game:forget(handle)
    self.spawned[handle] = nil
    for i = #self.order, 1, -1 do
        if self.order[i] == handle then table.remove(self.order, i) end
    end
end

local function destroy(actor)
    if not valid(actor) then return true end
    return (pcall(function() actor:K2_DestroyActor() end))
end

function Game:spawn(handle, unit_id, level, health_pct, slot, count)
    -- The same command may arrive twice after a reload; one enemy per handle.
    if self.spawned[handle] then return true, 'already summoned' end
    if Game.is_denied(unit_id) then return false, unit_id .. ' is never summoned by this mod' end
    local world, pawn = self:world(), self:pawn()
    if not valid(world) or not pawn then return false, 'no map or no player character' end
    local library = self:ai_library()
    if not library then return false, 'the AI library was not found' end
    local cls, why = self:class_for(unit_id)
    if not cls then return false, why end
    local transform = self:spawn_transform(pawn, slot, count)
    if not transform then return false, 'the player position cannot be read' end

    local actor
    local began, begin_error = pcall(function() actor = library:SpawnAIDefered(cls, transform, nil, world) end)
    actor = unwrap(actor)
    if not began or not valid(actor) then
        return false, 'SpawnAIDefered gave no enemy' .. (began and '' or ': ' .. tostring(begin_error))
    end
    -- Known from here on, so `clear` removes it whatever happens next.
    local unit = { actor = actor, unit_id = unit_id, hp = 0, max = 0 }
    self.spawned[handle] = unit
    self.order[#self.order + 1] = handle

    -- A map enemy gets its level from its spawner before it starts; a summoned one gets it here.
    local leveled = pcall(function() actor.Level = level end)
    local finished, finish_error = pcall(function() library:FinishSpawnAIDefered(actor, transform, world) end)
    if not finished or not live(actor) then
        destroy(actor)
        self:forget(handle)
        return false, 'FinishSpawnAIDefered failed' .. (finished and '' or ': ' .. tostring(finish_error))
    end
    local activated = pcall(function() library:ActivateAI(actor, world) end)
    if not live(actor) then
        self:forget(handle)
        return false, 'the enemy vanished as it was activated'
    end
    if health_pct < 100 then unit.pending_pct = health_pct end
    local notes = {}
    if not leveled then notes[#notes + 1] = 'level not set' end
    if not activated then notes[#notes + 1] = 'ActivateAI failed' end
    return true, #notes > 0 and ('summoned (' .. table.concat(notes, ', ') .. ')') or 'summoned'
end

-- The state of every summoned enemy. A unit seen at zero health stays `dead` after its body is
-- removed; one that disappears without dying is `gone`.
function Game:units()
    local out = {}
    for _, handle in ipairs(self.order) do
        local unit = self.spawned[handle]
        if unit then
            local status
            if unit.dead then
                status = 'dead'
            elseif not live(unit.actor) then
                -- Removed between two looks while nearly dead: it died.
                if unit.last_share and unit.last_share <= 0.05 then
                    unit.dead, unit.hp = true, 0
                    status = 'dead'
                else
                    status = 'gone'
                end
            else
                local hp, max = attribute(unit.actor, 'Health'), attribute(unit.actor, 'Health_Max')
                if hp then
                    unit.hp = hp
                    if max and max > 0 then
                        unit.max = max
                        unit.last_share = hp / max
                        if unit.pending_pct and hp > 0 then
                            local wanted = max * unit.pending_pct / 100
                            if hp > wanted then set_health(unit.actor, wanted) end
                            unit.pending_pct = nil
                            unit.hp = attribute(unit.actor, 'Health') or hp
                        end
                    end
                end
                if hp and hp <= 0 then
                    unit.dead = true
                    status = 'dead'
                else
                    status = 'alive'
                end
            end
            out[#out + 1] = { handle = handle, status = status, hp = unit.hp, max = unit.max, unit_id = unit.unit_id }
        end
    end
    return out
end

-- Removes every enemy this mod summoned.
function Game:clear()
    local removed, failed = 0, 0
    for _, handle in ipairs(self.order) do
        local unit = self.spawned[handle]
        if unit and valid(unit.actor) then
            if destroy(unit.actor) then removed = removed + 1 else failed = failed + 1 end
        end
    end
    self.spawned, self.order = {}, {}
    if failed > 0 then return false, tostring(failed) .. ' enemies could not be removed' end
    return true, tostring(removed) .. ' removed'
end

-- Checks, read-only, each engine call this file relies on. Shown by the director's `probe` command.
function Game:probe()
    local out = {}
    local function report(name, ok, detail)
        out[#out + 1] = { name = name, status = ok and 'ok' or 'fail', detail = tostring(detail or '') }
        return ok
    end
    if type(IsInGameThread) == 'function' then
        local ok, inside = pcall(IsInGameThread)
        report('game_thread', ok and inside == true, ok and tostring(inside) or 'IsInGameThread failed')
    else
        out[#out + 1] = { name = 'game_thread', status = 'skip', detail = 'IsInGameThread is not available' }
    end
    local world = self:world()
    report('world', valid(world), full_name(world) or 'no world')
    local pc = self:controller()
    report('controller', pc ~= nil, full_name(pc) or 'no player controller')
    local pawn = self:pawn()
    report('pawn', pawn ~= nil, full_name(pawn) or 'no player character')
    local hp, max = pawn and attribute(pawn, 'Health'), pawn and attribute(pawn, 'Health_Max')
    report('player_health', hp ~= nil and max ~= nil, 'Health=' .. tostring(hp) .. ' Health_Max=' .. tostring(max))
    local got_at, at = pcall(function() return pawn:K2_GetActorLocation() end)
    report('player_position', pawn ~= nil and got_at and vector(at) ~= nil, 'K2_GetActorLocation')
    local library = self:ai_library()
    report('ai_library', library ~= nil, full_name(library) or 'BP_SGameAIFunctionLibrary not found')
    for _, name in ipairs({ 'SpawnAIDefered', 'FinishSpawnAIDefered', 'ActivateAI' }) do
        local fn = library and field(library, name)
        report('ai_' .. name, fn ~= nil, fn ~= nil and 'found' or 'not found')
    end
    local count, why = self:load_catalog()
    report('codex', count > 0, tostring(count) .. ' class paths (' .. tostring(why) .. ')')
    local sample = 'BP_Monster_MaleThug_Sword_Blue'
    report('codex_sample', self.paths[sample] ~= nil, self.paths[sample] or (sample .. ' not in the codex'))
    for _, path in ipairs({ '/Script/UMG.UserWidget', '/Script/UMG.WidgetTree', '/Script/UMG.TextBlock',
        '/Script/UMG.Default__WidgetBlueprintLibrary', '/Script/Engine.Default__KismetTextLibrary' }) do
        local ok, obj = pcall(StaticFindObject, path)
        report('object_' .. path:match('([%w_]+)$'), ok and valid(obj), path)
    end
    return out
end

-- For the other bridge files.
Game.valid, Game.field, Game.key, Game.full_name = valid, field, key, full_name

return Game
