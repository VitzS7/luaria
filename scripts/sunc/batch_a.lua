-- Batch A: closures, debug, environment and metatable functions.
-- Locals are wrapped in do-block so they do not leak into the harness chunk.
do
	local function contains(list, target)
		for _, value in pairs(list) do
			if value == target then
				return true
			end
		end
		return false
	end

	-- tostring/tonumber keep the value out of constant folding, so the upvalue/local stays real.
	local function threeInnerFunctions()
		local function innerOne() return 1 end
		local function innerTwo() return 2 end
		return innerOne, innerTwo
	end

	T('checkcaller', function()
		assert(exists('checkcaller'), 'does not exist')
		eq(checkcaller(), true, 'checkcaller in script must be true')
	end)

	T('clonefunction', function()
		assert(exists('clonefunction'), 'does not exist')
		local captured = tonumber('7')
		local function base(x) return x + captured end
		local copy = clonefunction(base)
		assert(copy ~= base, 'clone must be a different function')
		eq(copy(1), base(1), 'clone must return the same result')
		eq(copy(1), 8, 'clone must keep the upvalue')
	end)

	T('getfunctionhash', function()
		assert(exists('getfunctionhash'), 'does not exist')
		-- same proto from a factory: identical bytecode, so the hash must match
		local function factory() return function() return 1 end end
		local hashA = getfunctionhash(factory())
		local hashB = getfunctionhash(factory())
		assert(type(hashA) == 'string' and #hashA == 96 and hashA:match('^%x+$'), 'hash must be 96 hex chars')
		eq(hashA, hashB, 'identical functions must share a hash')
		local other = getfunctionhash(function() return 2 end)
		assert(other ~= hashA, 'different body must have a different hash')
		local ok = pcall(getfunctionhash, print)
		assert(not ok, 'C closure must raise an error')
	end)

	T('hookfunction', function()
		assert(exists('hookfunction'), 'does not exist')
		local function target() return 'original' end
		local old = hookfunction(target, function() return 'hook' end)
		eq(target(), 'hook', 'calling target must reach the hook')
		eq(old(), 'original', 'returned old must be the original')
	end)

	T('hookmetamethod', function()
		assert(exists('hookmetamethod'), 'does not exist')
		local tbl = setmetatable({}, { __index = function(_, key) return 'real:' .. key end })
		local old = hookmetamethod(tbl, '__index', function(_, key) return 'hook:' .. key end)
		eq(tbl.x, 'hook:x', 'hooked __index must be used')
		eq(old(tbl, 'x'), 'real:x', 'returned old must be the original __index')
	end)

	T('iscclosure', function()
		assert(exists('iscclosure'), 'does not exist')
		eq(iscclosure(print), true, 'print is a C closure')
		eq(iscclosure(function() end), false, 'Luau function is not a C closure')
		eq(iscclosure(newcclosure(function() end)), true, 'newcclosure result is a C closure')
	end)

	T('isexecutorclosure', function()
		assert(exists('isexecutorclosure'), 'does not exist')
		local function localFunction() end
		eq(isexecutorclosure(localFunction), true, 'script function is an executor closure')
		eq(isexecutorclosure(print), false, 'print is not an executor closure')
	end)

	T('islclosure', function()
		assert(exists('islclosure'), 'does not exist')
		eq(islclosure(print), false, 'print is not a Lua closure')
		eq(islclosure(function() end), true, 'script function is a Lua closure')
	end)

	T('loadstring', function()
		assert(exists('loadstring'), 'does not exist')
		local chunk = loadstring('x__=1')
		assert(type(chunk) == 'function', 'valid source must return a function')
		chunk()
		eq(getgenv().x__, 1, 'chunk must set the global')
		getgenv().x__ = nil
		local ok, compiled, err = pcall(loadstring, 'x =')
		assert(ok, 'invalid source must not throw')
		eq(compiled, nil, 'invalid source must return nil first')
		assert(type(err) == 'string', 'second return must be an error message')
	end)

	T('newcclosure', function()
		assert(exists('newcclosure'), 'does not exist')
		local wrapped = newcclosure(function(a, b) return b, a end)
		local r1, r2 = wrapped(1, 2)
		eq(r1, 2, 'arguments must be forwarded')
		eq(r2, 1, 'returns must be forwarded')
		eq(iscclosure(wrapped), true, 'result must be a C closure')
		local waiting = newcclosure(function() task.wait(0.01); return 'after' end)
		eq(waiting(), 'after', 'yield inside the wrapper must work')
	end)

	T('restorefunction', function()
		assert(exists('restorefunction'), 'does not exist')
		local function target() return 1 end
		hookfunction(target, function() return 2 end)
		restorefunction(target)
		eq(target(), 1, 'restorefunction must bring back the original')
		local ok = pcall(restorefunction, function() end)
		assert(not ok, 'restoring a function that was never hooked must error')
	end)

	T('debug.getconstant', function()
		assert(exists('debug.getconstant'), 'does not exist')
		local function usesSplit() return string.split('foo bar', ' ') end
		eq(debug.getconstant(usesSplit, 1), 'string', 'constant 1 must be the string global name')
		eq(debug.getconstant(usesSplit, 999), nil, 'out of range index must be nil')
		local ok = pcall(debug.getconstant, print, 1)
		assert(not ok, 'C closure must raise an error')
	end)

	T('debug.getconstants', function()
		assert(exists('debug.getconstants'), 'does not exist')
		local function usesSplit() return string.split('foo bar', ' ') end
		local constants = debug.getconstants(usesSplit)
		assert(type(constants) == 'table', 'getconstants must return a table')
		assert(contains(constants, 'string'), 'constants must contain "string"')
		assert(contains(constants, 'split'), 'constants must contain "split"')
		assert(contains(constants, 'foo bar'), 'constants must contain "foo bar"')
	end)

	T('debug.getproto', function()
		assert(exists('debug.getproto'), 'does not exist')
		local list = debug.getproto(threeInnerFunctions, 1, true)
		assert(type(list) == 'table' and type(list[1]) == 'function', 'getproto(f, 1, true)[1] must be a function')
	end)

	T('debug.getprotos', function()
		assert(exists('debug.getprotos'), 'does not exist')
		local protos = debug.getprotos(threeInnerFunctions)
		eq(#protos, 2, 'must find the two inner functions')
	end)

	T('debug.getstack', function()
		assert(exists('debug.getstack'), 'does not exist')
		local function level()
			local localValue = tostring('marker')
			-- no tail call: a return would drop this frame and shift the level
			local found = debug.getstack(1, 1)
			return found
		end
		eq(level(), 'marker', 'getstack(1, 1) must return the local of that level')
		local ok = pcall(debug.getstack, 0)
		assert(not ok, 'getstack(0) must raise an error')
	end)

	T('debug.getupvalue', function()
		assert(exists('debug.getupvalue'), 'does not exist')
		local alpha = tostring('a')
		local beta = tostring('b')
		local function captures() return alpha .. beta end
		eq(debug.getupvalue(captures, 1), 'a', 'upvalue 1 must be alpha')
		eq(debug.getupvalue(captures, 2), 'b', 'upvalue 2 must be beta')
	end)

	T('debug.getupvalues', function()
		assert(exists('debug.getupvalues'), 'does not exist')
		local alpha = tostring('a')
		local beta = tostring('b')
		local function captures() return alpha .. beta end
		local upvalues = debug.getupvalues(captures)
		eq(upvalues[1], 'a', 'first upvalue must be alpha')
		eq(upvalues[2], 'b', 'second upvalue must be beta')
	end)

	T('debug.setconstant', function()
		assert(exists('debug.setconstant'), 'does not exist')
		local function constant() return 'old' end
		debug.setconstant(constant, 1, 'new')
		eq(constant(), 'new', 'call must reflect the new constant')
		eq(debug.getconstant(constant, 1), 'new', 'getconstant must reflect the new constant')
	end)

	T('debug.setstack', function()
		assert(exists('debug.setstack'), 'does not exist')
		local function alter()
			-- level 2 is the caller of alter, so this writes into chooser's local
			debug.setstack(2, 1, 'after')
		end
		local function chooser()
			local value = tostring('before')
			alter()
			return value
		end
		eq(chooser(), 'after', 'setstack(2, 1, v) must change the caller local')
	end)

	T('debug.setupvalue', function()
		assert(exists('debug.setupvalue'), 'does not exist')
		local factor = tonumber('1')
		local function usesFactor() return factor end
		debug.setupvalue(usesFactor, 1, 99)
		eq(usesFactor(), 99, 'setupvalue must change the upvalue seen by the function')
	end)

	T('getgc', function()
		assert(exists('getgc'), 'does not exist')
		local function liveFunction() return tostring('live') end
		local localTable = {}
		task.wait(0.05)
		local withoutTables = getgc()
		assert(contains(withoutTables, liveFunction), 'getgc() must contain a live local function')
		assert(not contains(withoutTables, localTable), 'getgc() without argument must not contain tables')
		local withTables = getgc(true)
		assert(contains(withTables, liveFunction), 'getgc(true) must contain the live function')
		assert(contains(withTables, localTable), 'getgc(true) must contain the local table')
	end)

	T('getgenv', function()
		assert(exists('getgenv'), 'does not exist')
		getgenv().luariaGetgenvValue = 42
		eq(luariaGetgenvValue, 42, 'global must see a value set through getgenv()')
		luariaGetgenvValue = 43
		eq(getgenv().luariaGetgenvValue, 43, 'plain global write must show up in getgenv()')
		getgenv().luariaGetgenvValue = nil
	end)

	T('getreg', function()
		assert(exists('getreg'), 'does not exist')
		local spawnedThread
		task.spawn(function()
			spawnedThread = coroutine.running()
			task.wait(0.05)
		end)
		assert(spawnedThread, 'task.spawn must run the body right away')
		local registry = getreg()
		eq(type(registry), 'table', 'getreg must return a table')
		assert(contains(registry, spawnedThread), 'registry must contain the task.spawn thread')
	end)

	T('getrenv', function()
		assert(exists('getrenv'), 'does not exist')
		local renv = getrenv()
		eq(type(renv), 'table', 'getrenv must return a table')
		eq(type(renv._G), 'table', '_G must exist in the game env')
		eq(type(renv.shared), 'table', 'shared must exist in the game env')
		renv._G.luariaRenvG = 5
		eq(luariaRenvG, 5, 'write to renv._G must reach the global')
		renv.luariaRenvValue = 7
		eq(luariaRenvValue, 7, 'write to renv must change the game env')
		renv.luariaRenvValue = nil
		renv._G.luariaRenvG = nil
	end)

	T('filtergc', function()
		assert(exists('filtergc'), 'does not exist')
		local function uniqueFilterFunction() return tostring('x') end
		local uniqueTable = { luariaUniqueKey = tostring('v') }
		local functions = filtergc('function', { Name = 'uniqueFilterFunction' })
		assert(contains(functions, uniqueFilterFunction), 'filter by Name must find the function')
		local tables = filtergc('table', { Keys = { 'luariaUniqueKey' } })
		assert(contains(tables, uniqueTable), 'filter by Keys must find the table')
	end)

	T('getnamecallmethod', function()
		assert(exists('getnamecallmethod'), 'does not exist')
		local captured
		local old
		old = hookmetamethod(game, '__namecall', function(self, ...)
			if self == game then
				captured = getnamecallmethod()
			end
			return old(self, ...)
		end)
		local ok, err = pcall(function() game:GetService('Players') end)
		-- restore the hook even if the call errors, so later tests see the real namecall
		hookmetamethod(game, '__namecall', old)
		assert(ok, err)
		eq(captured, 'GetService', 'inside __namecall the method must be GetService')
		eq(getnamecallmethod(), nil, 'outside __namecall the result must be nil')
	end)

	T('getrawmetatable', function()
		assert(exists('getrawmetatable'), 'does not exist')
		local mt = getrawmetatable(game)
		eq(type(mt), 'table', 'game metatable must be a table')
		eq(mt.__index(game, 'Workspace'), workspace, '__index(game, "Workspace") must be workspace')
		eq(getrawmetatable(newproxy(false)), nil, 'userdata without metatable must return nil')
		local locked = setmetatable({}, { __metatable = 'locked' })
		local real = getrawmetatable(locked)
		eq(type(real), 'table', 'raw metatable must be a table even when locked')
		eq(real.__metatable, 'locked', 'raw metatable must be the real one')
	end)

	T('isreadonly', function()
		assert(exists('isreadonly'), 'does not exist')
		eq(isreadonly({}), false, 'plain table is not read-only')
		eq(isreadonly(getrawmetatable(game)), true, 'game metatable is read-only')
	end)

	T('setrawmetatable', function()
		assert(exists('setrawmetatable'), 'does not exist')
		local original = getrawmetatable('x')
		local replacement = { __index = { luariaTest = function() return 'ok' end } }
		local ok, err = pcall(function()
			setrawmetatable('x', replacement)
			eq(getrawmetatable('x'), replacement, 'string metatable must be replaced')
			eq(('x'):luariaTest(), 'ok', 'method must resolve through the new metatable')
		end)
		-- string methods depend on this metatable, so always put it back
		setrawmetatable('x', original)
		assert(ok, err)
	end)

	T('setreadonly', function()
		assert(exists('setreadonly'), 'does not exist')
		local tbl = {}
		setreadonly(tbl, true)
		eq(isreadonly(tbl), true, 'table must be read-only after setreadonly(true)')
		local wrote = pcall(function() tbl.x = 1 end)
		assert(not wrote, 'write to read-only table must fail')
		setreadonly(tbl, false)
		tbl.x = 1
		eq(tbl.x, 1, 'write must work after setreadonly(false)')
		eq(isreadonly(tbl), false, 'table must not be read-only after setreadonly(false)')
	end)
end
