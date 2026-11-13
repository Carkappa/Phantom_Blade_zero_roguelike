-- The in-game panel: one block of text on a dark backing, top left of the screen.
--
-- It is built from plain engine widgets (UserWidget, Border, TextBlock), so it needs no game asset.
-- This has not been run in the game. If the panel cannot be made, `show` says so, the director
-- reports it, and the same text is always in the director's own window.
local Game = require('game')
local valid, field, key = Game.valid, Game.field, Game.key

local Hud = {}
Hud.__index = Hud

local HIT_TEST_INVISIBLE = 3    -- ESlateVisibility
local COLLAPSED = 1

local function find(path)
    local ok, obj = pcall(StaticFindObject, path)
    if ok and valid(obj) then return obj end
    error('missing ' .. path)
end

-- options: game, config, log
function Hud.new(options)
    local self = setmetatable({}, Hud)
    self.game = options.game
    self.config = options.config or {}
    self.log = options.log or function() end
    self.text = ''
    self.wanted = false         -- the director wants the panel shown
    self.user_hidden = false    -- the player hid it with the toggle key
    self.serial = 0
    self.shown_text = nil
    self.shown_visible = nil
    self.failed = nil
    return self
end

function Hud:visible()
    return self.wanted and not self.user_hidden
end

function Hud:destroy()
    local widget = self.widget
    self.widget, self.block, self.world_key = nil, nil, nil
    self.shown_text, self.shown_visible = nil, nil
    if valid(widget) then pcall(function() widget:RemoveFromParent() end) end
end

function Hud:create()
    local world, pc = self.game:world(), self.game:controller()
    assert(valid(world) and valid(pc), 'no map or no player controller')
    self.serial = self.serial + 1
    local n = tostring(self.serial)
    local widget = find('/Script/UMG.Default__WidgetBlueprintLibrary'):Create(world, find('/Script/UMG.UserWidget'), pc)
    assert(valid(widget), 'the panel widget was not created')
    local made, why = pcall(function()
        local tree = field(widget, 'WidgetTree')
        if not valid(tree) then
            tree = StaticConstructObject(find('/Script/UMG.WidgetTree'), widget, FName('PBZR_Tree_' .. n))
            assert(valid(tree), 'the widget tree was not created')
            widget.WidgetTree = tree
        end
        local block = StaticConstructObject(find('/Script/UMG.TextBlock'), tree, FName('PBZR_Text_' .. n))
        assert(valid(block), 'the text block was not created')
        local root = block
        -- A dark backing makes the text readable over the game. Without it the shadow has to do.
        local backed = pcall(function()
            local border = StaticConstructObject(find('/Script/UMG.Border'), tree, FName('PBZR_Back_' .. n))
            assert(valid(border))
            border:SetBrushColor({ R = 0, G = 0, B = 0, A = self.config.PanelOpacity or 0.72 })
            border:SetPadding({ Left = 18, Top = 14, Right = 18, Bottom = 14 })
            border:SetContent(block)
            root = border
        end)
        if not backed then root = block end
        pcall(function()
            local font = block.Font
            font.Size = self.config.PanelFontSize or 16
            block:SetFont(font)
        end)
        pcall(function()
            block:SetShadowOffset({ X = 1, Y = 1 })
            block:SetShadowColorAndOpacity({ R = 0, G = 0, B = 0, A = 1 })
        end)
        pcall(function()
            block.WrapTextAt = self.config.PanelWidth or 760
            block:SetAutoWrapText(true)
        end)
        tree.RootWidget = root
        widget:AddToViewport(self.config.PanelZOrder or 40)
        pcall(function()
            widget:SetAlignmentInViewport({ X = 0, Y = 0 })
            widget:SetPositionInViewport({ X = self.config.PanelX or 48, Y = self.config.PanelY or 96 }, false)
        end)
        self.block = block
    end)
    if not made then
        pcall(function() widget:RemoveFromParent() end)
        error(why, 0)
    end
    self.widget, self.world_key = widget, key(world)
    self.shown_text, self.shown_visible = nil, nil
end

-- Brings the panel in line with what is wanted. Called after every change and once per bridge tick,
-- because a map change destroys the widget.
function Hud:apply()
    local show = self:visible()
    local stale = self.widget ~= nil
        and (not valid(self.widget) or not valid(self.block) or self.world_key ~= key(self.game:world()))
    if stale then self:destroy() end
    if not show then
        if self.widget and self.shown_visible ~= false then
            pcall(function() self.widget:SetVisibility(COLLAPSED) end)
            self.shown_visible = false
        end
        return true
    end
    if not self.widget then
        local ok, why = pcall(function() self:create() end)
        if not ok then
            if self.failed ~= tostring(why) then
                self.failed = tostring(why)
                self.log('the panel could not be made: ' .. self.failed)
            end
            return false, 'the panel could not be made: ' .. tostring(why)
        end
        self.failed = nil
    end
    if self.shown_text ~= self.text then
        local ok, why = pcall(function()
            local as_text = find('/Script/Engine.Default__KismetTextLibrary'):Conv_StringToText(self.text)
            self.block:SetText(as_text)
        end)
        if not ok then return false, 'the panel text could not be set: ' .. tostring(why) end
        self.shown_text = self.text
    end
    if self.shown_visible ~= true then
        pcall(function() self.widget:SetVisibility(HIT_TEST_INVISIBLE) end)
        self.shown_visible = true
    end
    return true
end

-- The director's `screen` command.
function Hud:show(visible, text)
    self.wanted = visible == true
    self.text = type(text) == 'string' and text or ''
    return self:apply()
end

-- The player's toggle key.
function Hud:toggle()
    self.user_hidden = not self.user_hidden
    return self:apply()
end

return Hud
