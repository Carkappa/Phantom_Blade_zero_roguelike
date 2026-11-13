local Mock = require('mock_ue')
local tests = {}

local function setup()
    local env = Mock.install()
    for _, name in ipairs({ 'game', 'hud', 'keys' }) do package.loaded[name] = nil end
    local game = require('game').new({ config = {}, helpers = package.loaded['UEHelpers'] })
    local logged = {}
    local hud = require('hud').new({ game = game, config = { PanelFontSize = 18 }, log = function(m) logged[#logged + 1] = m end })
    return hud, env, game, logged
end

local function text_block(env)
    for i = #env.widgets, 1, -1 do
        if env.widgets[i]._class._path == '/Script/UMG.TextBlock' then return env.widgets[i] end
    end
end

function tests.the_panel_shows_text_and_hides(T)
    local hud, env = setup()
    T.eq(hud:show(true, 'Title\n 1. Go'), true)
    local block = text_block(env)
    T.eq(block.text, 'Title\n 1. Go')
    T.eq(block.font.Size, 18)
    T.eq(hud.widget.in_viewport, true)
    T.eq(hud.widget.visibility, 3)
    T.eq(hud:visible(), true)
    local made = #env.widgets

    T.eq(hud:show(true, 'Second'), true)
    T.eq(block.text, 'Second')
    T.eq(#env.widgets, made, 'new text reuses the panel')

    T.eq(hud:show(false, 'Second'), true)
    T.eq(hud.widget.visibility, 1)
    T.eq(hud:visible(), false)
    T.eq(hud:show(true, 'Third'), true)
    T.eq(block.text, 'Third'); T.eq(hud.widget.visibility, 3)
end

function tests.the_toggle_key_hides_without_losing_the_text(T)
    local hud, env = setup()
    hud:show(true, 'Kept')
    hud:toggle()
    T.eq(hud:visible(), false); T.eq(hud.widget.visibility, 1)
    -- The director sends a new screen while the player has the panel hidden.
    hud:show(true, 'Newer')
    T.eq(hud.widget.visibility, 1, 'a hidden panel stays hidden')
    hud:toggle()
    T.eq(hud.widget.visibility, 3)
    T.eq(text_block(env).text, 'Newer')
end

function tests.a_map_change_rebuilds_the_panel(T)
    local hud, env = setup()
    hud:show(true, 'Before')
    local first = hud.widget
    -- The world is replaced and the old widget dies with it.
    env.world = Mock.object({ _name = 'World2' })
    function env.controller:GetWorld() return env.world end
    package.loaded['UEHelpers'].GetGameViewportClient = function() return Mock.object({ World = env.world }) end
    first._valid = false
    T.eq(hud:apply(), true)
    T.ok(hud.widget ~= first, 'the panel was not rebuilt')
    T.eq(text_block(env).text, 'Before')
    T.eq(hud.widget.in_viewport, true)
end

function tests.a_panel_that_cannot_be_made_is_reported_not_fatal(T)
    local hud, env, _, logged = setup()
    env.fail_widget = true
    local ok, why = hud:show(true, 'Text')
    T.eq(ok, false); T.ok(why:find('could not be made', 1, true), why)
    hud:apply(); hud:apply()
    T.eq(#logged, 1, 'the failure is logged once')
    -- It works once the engine cooperates, and still knows the text.
    env.fail_widget = nil
    T.eq(hud:apply(), true)
    T.eq(text_block(env).text, 'Text')
    -- Nothing to do while hidden, even with no world at all.
    local hidden = setup()
    T.eq(hidden:show(false, 'x'), true)
end

function tests.a_widget_that_brings_its_own_tree_is_used(T)
    local hud, env = setup()
    env.widget_has_tree = true
    T.eq(hud:show(true, 'Tree'), true)
    T.eq(text_block(env).text, 'Tree')
    local trees = 0
    for _, w in ipairs(env.widgets) do if w._class._path == '/Script/UMG.WidgetTree' then trees = trees + 1 end end
    T.eq(trees, 1, 'no second tree was built')
end

local function keys_setup(enabled)
    local env = Mock.install()
    for _, name in ipairs({ 'game', 'keys' }) do package.loaded[name] = nil end
    local game = require('game').new({ config = {}, helpers = package.loaded['UEHelpers'] })
    local toggles = 0
    local state = { enabled = enabled ~= false }
    local keys = require('keys').new({ game = game,
        bindings = { { name = '1', key = 'One' }, { name = '2', key = 'Two' } }, toggle = 'F6',
        on_toggle = function() toggles = toggles + 1 end,
        choices_enabled = function() return state.enabled end })
    return keys, env, function() return toggles end, state
end

function tests.a_press_is_the_change_from_up_to_down(T)
    local keys, env = keys_setup()
    keys:sample()
    env.keys_down.One = true
    keys:sample(); keys:sample(); keys:sample()
    local pressed = keys:poll()
    T.eq(#pressed, 1, 'a held key counts once'); T.eq(pressed[1], '1')
    T.eq(#keys:poll(), 0, 'poll empties the queue')
    env.keys_down.One = false
    keys:sample()
    env.keys_down.One, env.keys_down.Two = true, true
    keys:sample()
    pressed = keys:poll()
    T.eq(#pressed, 2); T.eq(pressed[1], '1'); T.eq(pressed[2], '2')
end

function tests.a_key_held_when_first_seen_is_not_a_press(T)
    local keys, env = keys_setup()
    env.keys_down.One = true
    keys:sample(); keys:sample()
    T.eq(#keys:poll(), 0)
    -- Nor after a loading screen, when the controller comes back with the key still down.
    env.no_controller = true
    keys:sample()
    env.no_controller = nil
    keys:sample(); keys:sample()
    T.eq(#keys:poll(), 0)
end

function tests.choices_count_only_while_the_panel_is_up(T)
    local keys, env, toggles, state = keys_setup(false)
    keys:sample()
    env.keys_down.One = true
    keys:sample()
    T.eq(#keys:poll(), 0, 'a choice key did something with the panel hidden')
    -- The panel appears while the key is still held: still not a press.
    state.enabled = true
    keys:sample()
    T.eq(#keys:poll(), 0)
    -- The toggle key always works.
    env.keys_down.F6 = true
    keys:sample(); keys:sample()
    T.eq(toggles(), 1)
end

function tests.a_controller_that_cannot_be_read_presses_nothing(T)
    local keys, env = keys_setup()
    function env.controller:IsInputKeyDown() error('no such function') end
    keys:sample(); keys:sample()
    T.eq(#keys:poll(), 0)
    function env.controller:IsInputKeyDown() return 'yes' end
    keys:sample(); keys:sample()
    T.eq(#keys:poll(), 0)
end

return tests
