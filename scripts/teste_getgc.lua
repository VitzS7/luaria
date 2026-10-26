-- Teste ao vivo de getgc (nativa getgc em nativas.rs), contra o cliente aberto.
-- Imprime OK/FALHA por checagem e fecha com LUARIA_GC_FIM.
--
-- Uso: tools/test_exec.sh scripts/teste_getgc.lua LUARIA_GC_FIM

local function chk(nome, ok) print((ok and "OK " or "FALHA ") .. nome) end

chk("funcao existe", type(getgc) == "function")
local lista = getgc()
chk("getgc() devolve tabela", type(lista) == "table")
chk("getgc() tem mais de 1000 itens", #lista > 1000)

local so_funcao_ou_userdata = true
for _, v in ipairs(lista) do
    local t = type(v)
    if t ~= "function" and t ~= "userdata" then
        so_funcao_ou_userdata = false
        break
    end
end
chk("getgc() so traz function e userdata", so_funcao_ou_userdata)

local com_tabelas = getgc(true)
local achou_tabela = false
for _, v in ipairs(com_tabelas) do
    if type(v) == "table" then
        achou_tabela = true
        break
    end
end
chk("getgc(true) contem tabelas", achou_tabela)
chk("getgc(true) maior que getgc()", #com_tabelas > #lista)

local alvo = function() return "marca" end
local achou_alvo = false
for _, v in ipairs(getgc()) do
    if v == alvo then
        achou_alvo = true
        break
    end
end
chk("acha funcao criada no proprio script", achou_alvo)
print("LUARIA_GC_FIM")
