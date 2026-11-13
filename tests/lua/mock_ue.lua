-- A stand-in for the parts of UE4SS and the game that the bridge uses, for tests outside the game.
--
-- It follows what the bridge expects of the engine (see mod/PBZRoguelike/Scripts/game.lua), which in
-- turn follows calls the PhantomGladiator mod tested in the game. It proves the bridge's own logic;
-- it cannot prove the real engine behaves this way.
local M = {}

local next_address = 0x1000

local Object = {}
Object.__index = Object
function Object:IsValid() return self._valid == true end
function Object:GetAddress() return self._address end
function Object:GetFullName() return (self._class_name or 'Object') .. ' ' .. (self._path or self._name or '?') end
function Object:GetClass() return self._class end
function Object:GetOuter() return self._outer end
function Object:type() return 'UObject' end

function M.object(fields)
    next_address = next_address + 16
    local obj = setmetatable(fields or {}, Object)
    obj._valid, obj._address = true, next_address
    return obj
end

local function fname(text)
    return { _text = text, ToString = function(self) return self._text end }
end

local function attribute(value) return { BaseValue = value, CurrentValue = value } end

-- A fresh world: one map, one player, the libraries, a codex with `units` (id -> base health).
function M.install(options)
    options = options or {}
    local E = { objects = {}, keys_down = {}, npcs = {}, log = {}, destroyed = 0, widgets = {} }
    M.env = E

    local function register(path, obj)
        obj._path = path
        E.objects[path] = obj
        return obj
    end

    E.world = M.object({ _name = 'World', _class_name = 'World' })
    E.pawn = M.object({ _name = 'BP_Renewal_Hun_C_0', _class_name = 'BP_Renewal_Hun_C',
        AttributeSetBase = M.object({ Health = attribute(1000), Health_Max = attribute(1000) }),
        location = { X = 100, Y = 200, Z = 50 }, forward = { X = 1, Y = 0, Z = 0 } })
    function E.pawn:K2_GetActorLocation() return self.location end
    function E.pawn:GetActorForwardVector() return self.forward end
    function E.pawn:IsActorBeingDestroyed() return false end

    local controller_class = M.object({ _path = '/Script/ProjectPBZ.PBZPlayerController', _class_name = 'Class' })
    E.controller = M.object({ _name = 'PC_0', _class = controller_class, Pawn = E.pawn })
    E.controller.Player = M.object({ PlayerController = E.controller })
    function E.controller:GetWorld() return E.world end
    function E.controller:IsInputKeyDown(fkey) return E.keys_down[fkey.KeyName:ToString()] == true end

    -- The AI library.
    E.library = register('/SGameAI/BP_SGameAIFunctionLibrary.Default__BP_SGameAIFunctionLibrary_C', M.object({}))
    function E.library:SpawnAIDefered(cls, transform, owner, world)
        E.log[#E.log + 1] = 'begin ' .. cls._unit
        if E.fail_spawn and E.fail_spawn[cls._unit] then return nil end
        local npc = M.object({ _name = cls._unit .. '_C_' .. (#E.npcs + 1), _class = cls, unit = cls._unit,
            transform = transform, Level = 0, started = false, active = false, destroying = false })
        function npc:IsActorBeingDestroyed() return self.destroying end
        function npc:K2_DestroyActor()
            self.destroying, self._valid = true, false
            E.destroyed = E.destroyed + 1
        end
        E.npcs[#E.npcs + 1] = npc
        return npc
    end
    function E.library:FinishSpawnAIDefered(npc, transform, world)
        E.log[#E.log + 1] = 'finish ' .. npc.unit .. ' level ' .. tostring(npc.Level)
        if E.fail_finish and E.fail_finish[npc.unit] then error('finish refused') end
        -- Like the game, health comes from the level the actor holds as it starts.
        local max = (E.units[npc.unit] or 1000) * (1 + npc.Level)
        npc.AttributeSetBase = M.object({ Health = attribute(max), Health_Max = attribute(max) })
        npc.started = true
    end
    function E.library:ActivateAI(npc, world)
        E.log[#E.log + 1] = 'activate ' .. npc.unit
        npc.active = true
    end

    -- The codex.
    E.units = options.units or { BP_Monster_MaleThug_Sword_Blue = 800, BP_Monster_MoTian = 12000,
        BP_Monster_AHua_GuiTouDao = 9000 }
    local rows = {}
    for unit in pairs(E.units) do
        local path = '/PBZCombatAsset/ActorData/Mobs/' .. unit .. '/' .. unit .. '.' .. unit .. '_C'
        register(path, M.object({ _unit = unit, _class_name = 'BlueprintGeneratedClass' }))
        rows[#rows + 1] = {
            EnemyClass = {
                type = function() return 'TSoftObjectPtrUserdata' end,
                GetObjectID = function()
                    return { GetAssetPathName = function() return fname(path) end,
                        GetSubPathString = function() return fname('') end }
                end,
            },
            AliasEnemyClasses = {},
        }
    end
    table.sort(rows, function(a, b)
        return a.EnemyClass.GetObjectID():GetAssetPathName():ToString() < b.EnemyClass.GetObjectID():GetAssetPathName():ToString()
    end)
    E.codex = M.object({ _outer = E.world })
    function E.codex:GetAllEnemyCodexEntries() return rows end

    -- Widgets.
    local function widget_class(path, methods)
        local cls = register(path, M.object({ _class_name = 'Class' }))
        cls.methods = methods or {}
        return cls
    end
    local function make_widget(cls, outer, name)
        local w = M.object({ _class = cls, _outer = outer, _name = name, calls = {} })
        setmetatable(w, { __index = function(self, k)
            if Object[k] then return Object[k] end
            if cls.methods[k] then return cls.methods[k] end
            -- Any other widget call is recorded and accepted.
            return function(_, ...) self.calls[#self.calls + 1] = { k, ... } end
        end })
        E.widgets[#E.widgets + 1] = w
        return w
    end
    local user_widget = widget_class('/Script/UMG.UserWidget', {
        AddToViewport = function(self, z) self.in_viewport, self.z = true, z end,
        RemoveFromParent = function(self) self.in_viewport = false end,
        SetVisibility = function(self, v) self.visibility = v end,
    })
    widget_class('/Script/UMG.WidgetTree')
    widget_class('/Script/UMG.TextBlock', {
        SetText = function(self, text) self.text = text end,
        SetFont = function(self, font) self.font = font end,
    })
    widget_class('/Script/UMG.Border', { SetContent = function(self, child) self.content = child end })
    local widget_library = register('/Script/UMG.Default__WidgetBlueprintLibrary', M.object({}))
    function widget_library:Create(world, cls, pc)
        if E.fail_widget then return nil end
        local w = make_widget(cls, world, 'Widget')
        if E.widget_has_tree then rawset(w, 'WidgetTree', make_widget(E.objects['/Script/UMG.WidgetTree'], w, 'Tree')) end
        return w
    end
    local text_library = register('/Script/Engine.Default__KismetTextLibrary', M.object({}))
    function text_library:Conv_StringToText(text) return text end
    register('/Script/Engine.Default__KismetSystemLibrary', M.object({}))

    -- UE4SS globals.
    FName = fname
    function StaticFindObject(path)
        local obj = E.objects[path]
        if obj and obj._valid then return obj end
        return nil
    end
    function LoadAsset(path) return StaticFindObject(path) end
    function FindObject() return nil end
    function FindAllOf(name)
        if name == 'PlayerController' and not E.no_controller then return { E.controller } end
        if name == 'PBZCodexSubsystem' and not E.no_codex then return { E.codex } end
        return nil
    end
    function StaticConstructObject(cls, outer, name)
        local w = make_widget(cls, outer, name:ToString())
        if cls._path == '/Script/UMG.TextBlock' then rawset(w, 'Font', { Size = 10 }) end
        return w
    end
    function IsInGameThread() return true end
    E.loops = {}
    function LoopInGameThreadWithDelay(ms, fn)
        E.loops[#E.loops + 1] = { ms = ms, fn = fn }
        return #E.loops
    end
    LoopAsync, ExecuteInGameThread = nil, nil
    package.loaded['UEHelpers'] = { GetGameViewportClient = function() return M.object({ World = E.world }) end }
    return E
end

-- Health helpers for tests.
function M.set_health(actor, value)
    actor.AttributeSetBase.Health.BaseValue = value
    actor.AttributeSetBase.Health.CurrentValue = value
end
function M.health(actor) return actor.AttributeSetBase.Health.CurrentValue end

return M
