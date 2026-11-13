-- Runs the bridge's tests outside the game:  lua tests/lua/run_tests.lua [repository root]
-- Works in LuaJIT / Lua 5.1 and in Lua 5.4, the version UE4SS embeds.
local ROOT = arg and arg[1] or '.'
package.path = ROOT .. '/mod/PBZRoguelike/Scripts/?.lua;' .. ROOT .. '/tests/lua/?.lua;' .. package.path

local T = { passed = 0, failed = 0, root = ROOT }

local function show(value)
    if type(value) == 'string' then return string.format('%q', value) end
    return tostring(value)
end

function T.eq(actual, expected, what)
    if actual ~= expected then
        error((what or 'values differ') .. ': expected ' .. show(expected) .. ', got ' .. show(actual), 2)
    end
end

function T.ok(value, what)
    if not value then error(what or 'expected a true value', 2) end
end

function T.near(actual, expected, what)
    if type(actual) ~= 'number' or math.abs(actual - expected) > 0.001 then
        error((what or 'numbers differ') .. ': expected ' .. show(expected) .. ', got ' .. show(actual), 2)
    end
end

function T.read(path)
    local file = assert(io.open(ROOT .. '/' .. path, 'rb'), 'cannot open ' .. path)
    local content = file:read('*a')
    file:close()
    return content
end

-- An in-memory stand-in for io.open.
function T.files()
    local store = {}
    local function open(path, mode)
        if mode:sub(1, 1) == 'r' then
            if store[path] == nil then return nil end
            local content = store[path]
            return { read = function() return content end, close = function() end }
        end
        if store.__readonly then return nil end
        local parts = mode:sub(1, 1) == 'a' and { store[path] or '' } or {}
        return {
            write = function(_, text) parts[#parts + 1] = text end,
            close = function() store[path] = table.concat(parts) end,
        }
    end
    return store, open
end

local function run(name, tests)
    local names = {}
    for test in pairs(tests) do names[#names + 1] = test end
    table.sort(names)
    for _, test in ipairs(names) do
        local ok, why = xpcall(function() tests[test](T) end, function(e) return debug.traceback(tostring(e), 2) end)
        if ok then
            T.passed = T.passed + 1
        else
            T.failed = T.failed + 1
            io.write('FAIL ', name, '.', test, '\n  ', tostring(why):gsub('\n', '\n  '), '\n')
        end
    end
end

for _, name in ipairs({ 'test_proto', 'test_bridge', 'test_game', 'test_hud_keys', 'test_main' }) do
    run(name, require(name))
end
io.write(string.format('%s: %d passed, %d failed\n', _VERSION, T.passed, T.failed))
os.exit(T.failed == 0 and 0 or 1)
