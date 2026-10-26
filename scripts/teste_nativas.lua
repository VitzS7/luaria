-- Teste ao vivo das nativas (getrawmetatable/setrawmetatable/setreadonly), contra o
-- cliente aberto. Imprime OK/FALHA por checagem e fecha com LUARIA_NATIVAS_FIM.
--
-- Uso: tools/test_exec.sh scripts/teste_nativas.lua LUARIA_NATIVAS_FIM
-- Esperado: 16 linhas OK por execução.

local function chk(nome, ok) print((ok and "OK " or "FALHA ") .. nome) end
local function falha(f, ...) local ok = coroutine.resume(coroutine.create(f), ...) return not ok end

chk("funcoes existem", type(getrawmetatable) == "function" and type(setrawmetatable) == "function" and type(setreadonly) == "function")
local mtg = getrawmetatable(game)
chk("mt de game e tabela com __index", type(mtg) == "table" and mtg.__index ~= nil)
local trav = setmetatable({}, { __metatable = "LOCK" })
chk("getmetatable respeita trava", getmetatable(trav) == "LOCK")
chk("getrawmetatable ignora trava", getrawmetatable(trav).__metatable == "LOCK")
chk("sem mt -> nil", getrawmetatable({}) == nil)

local c = table.freeze({ a = 1 })
chk("isreadonly true", isreadonly(c))
chk("escrita em congelada falha", falha(function() c.a = 2 end) and c.a == 1)
setreadonly(c, false)
chk("isreadonly false", not isreadonly(c))
c.a = 3
chk("escreve apos destravar", c.a == 3)
setreadonly(c, true)
chk("trava de novo", falha(function() c.a = 4 end) and c.a == 3)

local p = {}
chk("setrawmetatable devolve o objeto", setrawmetatable(p, { __index = function(_, k) return k .. "!" end }) == p)
chk("mt aplicado", p.zzz == "zzz!")
setrawmetatable(trav, { __index = { v = 7 } })
chk("rompe __metatable", trav.v == 7 and type(getmetatable(trav)) == "table")
setrawmetatable(trav, nil)
chk("nil remove", getrawmetatable(trav) == nil)
local f = table.freeze({})
setrawmetatable(f, { __index = { w = 1 } })
chk("congelada aceita e continua congelada", f.w == 1 and isreadonly(f))
chk("arg invalido da erro", falha(getrawmetatable, 5) and falha(setreadonly, {}, 1))
print("LUARIA_NATIVAS_FIM")
