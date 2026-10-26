--[[
Luaria - suite própria no molde sUNC (harness em inglês).

Copyright (c) 2026 Luaria contributors
Licensed under the MIT License (see LICENSE).

Original tests written from the public contract at docs.sunc.io; not affiliated with sUNC.

Cobertura: 79 funções, uma T(...) por função.
  Closures (11): checkcaller, clonefunction, getfunctionhash, hookfunction, hookmetamethod,
    iscclosure, isexecutorclosure, islclosure, loadstring, newcclosure, restorefunction
  Debug (10): debug.getconstant, debug.getconstants, debug.getproto, debug.getprotos,
    debug.getstack, debug.getupvalue, debug.getupvalues, debug.setconstant, debug.setstack, debug.setupvalue
  Drawing (4): isrenderobj, setrenderproperty, getrenderproperty, cleardrawcache
  Encoding (4): base64encode, base64decode, lz4compress, lz4decompress
  Environment (5): getgc, getgenv, getreg, getrenv, filtergc
  Filesystem (11): writefile, readfile, appendfile, delfile, delfolder, isfile, isfolder,
    listfiles, loadfile, makefolder, getcustomasset
  Instances (9): cloneref, compareinstances, fireclickdetector, fireproximityprompt, firetouchinterest,
    getcallbackvalue, gethui, getinstances, getnilinstances
  Metatable (5): getnamecallmethod, getrawmetatable, isreadonly, setrawmetatable, setreadonly
  Misc (2): identifyexecutor, request
  Reflection (6): gethiddenproperty, sethiddenproperty, getthreadidentity, setthreadidentity,
    isscriptable, setscriptable
  Scripts (9): getcallingscript, getloadedmodules, getrunningscripts, getscriptbytecode,
    getscriptclosure, getscriptfromthread, getscripthash, getscripts, getsenv
  Signals (3): firesignal, getconnections, replicatesignal
]]

local TOTAL = 79
passed = 0
failed = 0
skipped = 0

-- Teste em execução dentro de T; SKIP e eq/expectError marcam aqui.
local testeAtual = nil

-- Accepts "name" or a dotted path such as "debug.getconstant" / "Drawing.new".
local function exists(name)
	local current = getgenv()
	for part in string.gmatch(name, "[^%.]+") do
		if type(current) ~= "table" then
			return false
		end
		current = current[part]
	end
	return type(current) == "function"
end

-- pcall keeps one broken check from aborting the batch; a `false` return counts as a failure.
-- Teste com SKIP nunca conta como passe: vira "partial" (havia checagem negativa) ou "no test".
local function T(name, fn)
	testeAtual = { checks = 0, skipReason = nil }
	local ok, result = pcall(fn)
	local teste = testeAtual
	testeAtual = nil
	if not ok or result == false then
		failed = failed + 1
		local reason = ok and "returned false" or tostring(result)
		print("⛔ " .. name .. ": " .. reason)
		return
	end
	if not teste.skipReason then
		passed = passed + 1
		print("✅ " .. name)
		return
	end
	skipped = skipped + 1
	local tipo = teste.checks > 0 and "partial" or "no test"
	print("⏺️ " .. name .. " (" .. tipo .. ": " .. teste.skipReason .. ")")
end

-- Limpeza de arquivos temporários: roda fora da contagem, não é um teste de função.
local function CLEANUP(fn)
	local ok, err = pcall(fn)
	if not ok then
		print("⚠️ cleanup: " .. tostring(err))
	end
end

local function expectError(f, ...)
	if testeAtual then
		testeAtual.checks = testeAtual.checks + 1
	end
	return not pcall(f, ...)
end

local function SKIP(reason)
	if testeAtual then
		testeAtual.skipReason = reason
	end
end

local function eq(a, b, msg)
	if testeAtual then
		testeAtual.checks = testeAtual.checks + 1
	end
	if a ~= b then
		error((msg or "values differ") .. ": expected " .. tostring(b) .. ", got " .. tostring(a), 2)
	end
	return true
end

local function assert(condition, msg)
	if not condition then
		error(msg or "assertion failed", 2)
	end
	return condition
end

-- Batches run inside a spawned thread so a stuck test cannot block the report forever.
local finished = false
local TIMEOUT = getgenv().SUNC_TIMEOUT or 120

task.spawn(function()
--@@BATCH_A@@
--@@BATCH_B@@
--@@BATCH_C@@
	finished = true
end)

local waited = 0
while not finished and waited < TIMEOUT do
	task.wait(0.25)
	waited = waited + 0.25
end

if not finished then
	failed = failed + 1
	print("⛔ timeout: suite did not finish in " .. TIMEOUT .. "s")
end

print(("sUNC (own): %d/%d passed, %d skipped, %d failed"):format(passed, TOTAL, skipped, failed))
print("sUNC_OWN_DONE")
