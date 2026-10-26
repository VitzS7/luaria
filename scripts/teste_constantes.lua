-- Teste ao vivo de debug.getconstant/getconstants (nativa getconstant em nativas.rs).
-- Imprime OK/FALHA por checagem e fecha com LUARIA_CONST_FIM.
--
-- Uso: tools/test_exec.sh scripts/teste_constantes.lua LUARIA_CONST_FIM

local function chk(nome, ok) print((ok and "OK " or "FALHA ") .. nome) end
local function falha(f, ...) local ok = coroutine.resume(coroutine.create(f), ...) return not ok end

local function f() return print("oi", 42) end

chk("funcoes existem", type(debug.getconstant) == "function" and type(debug.getconstants) == "function")
local lista = debug.getconstants(f)
local achou_oi, achou_42 = false, false
for _, v in ipairs(lista) do
    if v == "oi" then achou_oi = true elseif v == 42 then achou_42 = true end
end
chk("getconstants traz 'oi' e 42", achou_oi and achou_42)
chk("getconstant(f,1) devolve constante", debug.getconstant(f, 1) == lista[1])
chk("getconstant(f,n) bate com a lista", debug.getconstant(f, #lista) == lista[#lista])
chk("indice 0 erra", falha(debug.getconstant, f, 0))
chk("indice acima do total erra", falha(debug.getconstant, f, #lista + 1))
chk("indice fracionario erra", falha(debug.getconstant, f, 1.5))
chk("arg nao funcao erra", falha(debug.getconstant, 5, 1))
chk("getconstants nao funcao erra", falha(debug.getconstants, {}))
chk("funcao sem constantes devolve vazio", #debug.getconstants(function() end) == 0)
print("LUARIA_CONST_FIM")
