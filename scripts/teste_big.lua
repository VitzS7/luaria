-- Quatro funções aninhadas + tabela + laço: exercita 5 protos e um chunk
-- acima de 280 bytes (o limite da verificação de assinatura do loadsafe).
local function soma1(x) return x + 1 end
local function dobra(x) return soma1(x) * 2 end
local function tira3(x) return dobra(x) - 3 end
local function soma10(x) return tira3(x) + 10 end

local t = {x = 5, y = 7}
local s = 0
for i = 1, 3 do
    s = s + soma10(i) + t.x
end
print("BIG_OK " .. s)
