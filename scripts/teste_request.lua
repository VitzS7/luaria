-- Teste ao vivo do request pelo canal do host. Precisa da UI aberta ou do luaria-cli com --ficar
-- (quem atende o canal). Sem pcall: o fast-pcall da Roblox não sobrevive à chamada remota.
-- Request espera com task.wait, então cada pedido roda numa thread do task.spawn; um erro
-- dentro dela aparece no log do jogo, e o marcador só sai quando todos os pedidos terminam.
-- Uso: tools/test_exec.sh scripts/teste_request.lua LUARIA_REQ_FIM

local falhas = 0
local pendentes = 2

local function tentar(f, ...)
    return coroutine.resume(coroutine.create(f), ...)
end

local function checa(nome, cond, detalhe)
    if cond then
        print("OK " .. nome)
    else
        falhas += 1
        print("FALHA " .. nome .. (detalhe and (": " .. tostring(detalhe)) or ""))
    end
end

local function concluir()
    pendentes -= 1
    if pendentes == 0 then
        print("LUARIA_REQ_FIM falhas=" .. falhas)
    end
end

task.spawn(function()
    local inicio = tick()
    local r = request({ Url = "https://httpbin.org/user-agent", Method = "GET" })
    print("TEMPO GET ida_volta_s=" .. string.format("%.3f", tick() - inicio))
    checa("GET 200", r.StatusCode == 200, r.StatusCode)
    checa("GET corpo tem user-agent", r.Body:find("user-agent", 1, true) ~= nil, r.Body)
    concluir()
end)

task.spawn(function()
    local inicio = tick()
    local r = request({
        Url = "https://httpbin.org/post",
        Method = "POST",
        Headers = { ["X-Teste"] = "luaria" },
        Body = "ola:mundo",
    })
    print("TEMPO POST ida_volta_s=" .. string.format("%.3f", tick() - inicio))
    checa("POST 200", r.StatusCode == 200, r.StatusCode)
    checa("POST eco do corpo", r.Body:find("ola:mundo", 1, true) ~= nil, r.Body)
    checa("POST eco do header", r.Body:find("luaria", 1, true) ~= nil, r.Body)
    concluir()
end)

-- Sem Url erra antes de qualquer espera, então o resume simples basta.
local ok, r = tentar(request, { Method = "GET" })
checa("sem Url erra", not ok and tostring(r):find("invalid argument #1", 1, true) ~= nil, r)

-- SSRF: o erro sai pelo log do jogo ("destino recusado"), não pelo marcador.
task.spawn(function()
    request({ Url = "http://127.0.0.1:1/" })
end)
