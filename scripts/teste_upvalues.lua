-- Teste ao vivo de debug.getupvalue/getupvalues (nativa getupvalue em nativas.rs).
-- Imprime OK/FALHA por checagem e fecha com LUARIA_UPV_FIM.
--
-- Uso: tools/test_exec.sh scripts/teste_upvalues.lua LUARIA_UPV_FIM

local function chk(nome, ok) print((ok and "OK " or "FALHA ") .. nome) end
local function falha(f, ...) local ok = coroutine.resume(coroutine.create(f), ...) return not ok end

local a = 1337
local b = "x"
local fn = function() end
local function f() return a, b, fn end

chk("funcoes existem", type(debug.getupvalue) == "function" and type(debug.getupvalues) == "function")
chk("getupvalue(f,1) == 1337", debug.getupvalue(f, 1) == 1337)
chk("getupvalue(f,2) == 'x'", debug.getupvalue(f, 2) == "x")
chk("getupvalue(f,3) == fn", debug.getupvalue(f, 3) == fn)
local lista = debug.getupvalues(f)
chk("getupvalues(f) tem 3 itens", #lista == 3)
chk("getupvalues(f) na ordem", lista[1] == 1337 and lista[2] == "x" and lista[3] == fn)
chk("indice 0 erra", falha(debug.getupvalue, f, 0))
chk("indice 4 (acima do total) erra", falha(debug.getupvalue, f, 4))
chk("indice fracionario erra", falha(debug.getupvalue, f, 1.5))
chk("arg nao funcao erra", falha(debug.getupvalue, 5, 1))
chk("getupvalues nao funcao erra", falha(debug.getupvalues, {}))
chk("funcao sem upvalues devolve vazio", #debug.getupvalues(function() end) == 0)
chk("upvalue atualiza depois da mudanca", (function() a = 7 return debug.getupvalue(f, 1) end)() == 7)
print("LUARIA_UPV_FIM")
