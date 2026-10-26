-- Prelude da API do executor.
--
-- Roda no mesmo chunk do script do usuário, dentro do estado do JOGO (o motor
-- descarta threads do LuaApp antes de chegar aqui). Só entra o que é real: nada
-- de `Drawing` falso, `hookfunction` de mentira ou `getconnections` com campos
-- inventados. O que exige código nativo está listado em docs/limitacoes.md.
--
-- Sem `pcall` de propósito: o fast-pcall da Roblox não sobrevive à chamada
-- remota (já matou o cliente). Cada linha aqui é guardada por `type`/`nil` em
-- vez de proteção de erro.

local env = {}

local marcadas = {}
local invalidados = setmetatable({}, { __mode = "k" })
local refs = setmetatable({}, { __mode = "k" })
-- o motor injeta __luaria_identidade com o valor que de fato elevou a thread
-- (LUARIA_IDENTIDADE); sem elevação cai no 2 de um LocalScript comum. setthreadidentity
-- só muda esse estado do prelude: ele roda em Lua e não enxerga o extra-space.
local identidade = __luaria_identidade or 2

local function marcar(f)
    if type(f) == "function" then
        marcadas[f] = true
    end
    return f
end

local function publicar(nome, valor)
    env[nome] = valor
end

-- Fonte do chunk ("luaria"): toda closure Luau criada pelo executor tem essa fonte.
local FONTE = debug.info(1, "s")

-- Erro de argumento no formato da UNC/sUNC. Nível 3 = quem chamou a função da API.
local function checar(v, tipo, n, nome)
    if type(v) ~= tipo then
        error(string.format("invalid argument #%d to '%s' (%s expected, got %s)", n, nome, tipo, typeof(v)), 3)
    end
    return v
end

local function checar_instancia(v, n, nome)
    if typeof(v) ~= "Instance" then
        error(string.format("invalid argument #%d to '%s' (Instance expected, got %s)", n, nome, typeof(v)), 3)
    end
    return v
end

local function eh_c(f)
    return debug.info(f, "s") == "[C]"
end

-- Identidade ----------------------------------------------------------------

publicar("identifyexecutor", marcar(function()
    return "luaria", "0.1"
end))
publicar("getexecutorname", env.identifyexecutor)

publicar("getthreadidentity", marcar(function()
    return identidade
end))
publicar("getidentity", env.getthreadidentity)
publicar("getthreadcontext", env.getthreadidentity)
publicar("setthreadidentity", marcar(function(n)
    if type(n) == "number" then
        identidade = n
    end
    return identidade
end))
publicar("setidentity", env.setthreadidentity)
publicar("setthreadcontext", env.setthreadidentity)

publicar("checkcaller", marcar(function()
    return true
end))

-- Ambientes -----------------------------------------------------------------

-- getfenv é bloqueado pelo sandbox da Roblox para scripts comuns; esta versão
-- devolve o registro de funções do executor (não o ambiente do jogo).
publicar("getfenv", marcar(function()
    return env
end))

publicar("getgenv", marcar(function()
    return env
end))

-- `getrenv()` precisa de um `_G` diferente do nosso (é o que a UNC testa), mas
-- o resto do ambiente tem de ser o do jogo — daí o proxy.
publicar("getrenv", marcar(function()
    return setmetatable({ _G = {} }, { __index = env })
end))

publicar("isexecutorclosure", marcar(function(f)
    checar(f, "function", 1, "isexecutorclosure")
    return marcadas[f] == true or (not eh_c(f) and debug.info(f, "s") == FONTE)
end))
publicar("checkclosure", env.isexecutorclosure)
publicar("isourclosure", env.isexecutorclosure)

publicar("iscclosure", marcar(function(f)
    checar(f, "function", 1, "iscclosure")
    return eh_c(f)
end))

publicar("islclosure", marcar(function(f)
    checar(f, "function", 1, "islclosure")
    return not eh_c(f)
end))

-- coroutine.wrap devolve uma closure C de verdade (debug.info diz "[C]") que
-- chama f a cada resume. Limites: erro dentro de f mata a closure (próxima
-- chamada dá "cannot resume dead coroutine"), f não pode chamar a si mesma pela
-- versão C e yield dentro de f devolve cedo para quem chamou.
local function newcclosure(f)
    local c = coroutine.wrap(function(...)
        local args = table.pack(...)
        while true do
            args = table.pack(coroutine.yield(f(table.unpack(args, 1, args.n))))
        end
    end)
    marcadas[c] = true
    return c
end

publicar("newcclosure", marcar(function(f)
    checar(f, "function", 1, "newcclosure")
    return newcclosure(f)
end))

publicar("clonefunction", marcar(function(f)
    checar(f, "function", 1, "clonefunction")
    if eh_c(f) then
        return newcclosure(f)
    end
    return marcar(function(...)
        return f(...)
    end)
end))

-- Marcador do loadstring inlinado pelo host (remoto.rs): começa com identificador
-- para nunca colar numa expressão anterior nem exigir ";" (o Luau rejeita).
publicar("__luaria_fn", function(f)
    return f
end)

-- Instâncias ----------------------------------------------------------------

-- Erro de propriedade/serviço vira (false, msg) sem pcall (ver topo do arquivo).
local function protegido(f, ...)
    return coroutine.resume(coroutine.create(f), ...)
end

-- Sem acesso nativo ao container escondido: PlayerGui é onde uma ScreenGui de
-- script local pode ficar; CoreGui só quando não há LocalPlayer.
publicar("gethui", marcar(function()
    local ok, pg = protegido(function()
        return game:GetService("Players").LocalPlayer:FindFirstChildOfClass("PlayerGui")
    end)
    return ok and pg or game:GetService("CoreGui")
end))

-- Só o que está na árvore do DataModel; instância sem pai vem do GC (getnilinstances).
local function descendentes()
    local lista = {}
    for _, servico in game:GetChildren() do
        local ok, filhos = protegido(function()
            return servico:GetDescendants()
        end)
        if ok then
            lista[#lista + 1] = servico
            table.move(filhos, 1, #filhos, #lista + 1, lista)
        end
    end
    return lista
end

publicar("getinstances", marcar(function()
    local lista = descendentes()
    if env.getnilinstances then
        for _, i in env.getnilinstances() do
            lista[#lista + 1] = i
        end
    end
    return lista
end))

publicar("getscripts", marcar(function()
    local scripts = {}
    for _, i in descendentes() do
        if i:IsA("LocalScript") or i:IsA("ModuleScript") then
            scripts[#scripts + 1] = i
        end
    end
    return scripts
end))

-- Propriedade que o Lua consegue ler. Inexistente também dá false.
publicar("isscriptable", marcar(function(obj, prop)
    checar_instancia(obj, 1, "isscriptable")
    checar(prop, "string", 2, "isscriptable")
    return (protegido(function()
        return obj[prop]
    end))
end))

-- InputHoldBegin/End é a API pública do ProximityPrompt para o cliente; com
-- HoldDuration 0 o Triggered sai na hora. Respeita distância e Enabled.
publicar("fireproximityprompt", marcar(function(prompt, quantidade, pular)
    checar_instancia(prompt, 1, "fireproximityprompt")
    if not prompt:IsA("ProximityPrompt") then
        error("invalid argument #1 to 'fireproximityprompt' (ProximityPrompt expected, got " .. prompt.ClassName .. ")", 2)
    end
    local espera = prompt.HoldDuration
    prompt.HoldDuration = 0
    for _ = 1, math.max(1, math.floor(tonumber(quantidade) or 1)) do
        prompt:InputHoldBegin()
        if not pular then
            task.wait()
        end
        prompt:InputHoldEnd()
    end
    prompt.HoldDuration = espera
end))

publicar("isrbxactive", marcar(function()
    local uis = game:GetService("UserInputService")
    return uis ~= nil and uis.WindowFocused == true
end))
publicar("isgameactive", env.isrbxactive)

publicar("isreadonly", marcar(function(t)
    checar(t, "table", 1, "isreadonly")
    return table.isfrozen(t)
end))

-- Nativos C que o motor empurra como args do chunk (nativas.rs); nil se LUARIA_NATIVAS=0
-- ou se o payload mudou. Sem nativo, a função simplesmente não existe.
local n_grm, n_srm, n_sro, n_gk, n_gu, n_gg = __n_grm, __n_srm, __n_sro, __n_gk, __n_gu, __n_gg
local n_pe, n_pr = __n_pe, __n_pr
local n_sk, n_su = __n_sk, __n_su

local function checar_objeto(v, n, nome)
    local t = type(v)
    if t ~= "table" and t ~= "userdata" then
        error(string.format("invalid argument #%d to '%s' (table or userdata expected, got %s)", n, nome, typeof(v)), 3)
    end
    return t
end

if type(n_sro) == "function" then
    publicar("setreadonly", marcar(function(t, ligado)
        checar(t, "table", 1, "setreadonly")
        checar(ligado, "boolean", 2, "setreadonly")
        n_sro(t, ligado)
    end))
end

if type(n_grm) == "function" then
    publicar("getrawmetatable", marcar(function(o)
        checar_objeto(o, 1, "getrawmetatable")
        return n_grm(o)
    end))
end

-- Só tabelas e userdata vivos: o visitor do nativo descarta o resto (nativas.rs).
if type(n_gg) == "function" then
    publicar("getgc", marcar(function(incluir_tabelas)
        return n_gg(incluir_tabelas == true)
    end))
end

-- Instância sem pai não aparece no DataModel, só no GC. game também tem Parent nil e fica de fora.
if type(n_gg) == "function" then
    publicar("getnilinstances", marcar(function()
        local lista = {}
        for _, obj in n_gg(false) do
            if typeof(obj) == "Instance" and obj ~= game and obj.Parent == nil then
                lista[#lista + 1] = obj
            end
        end
        return lista
    end))
end

-- Env de script do jogo é a tabela com `script` (ModuleScript, LocalScript, CoreScript): só o
-- GC a enxerga. Heurística: uma tabela de usuário que guarde um script num campo `script`
-- também conta. Script sem env vivo (ainda não rodou ou já foi coletado) dá erro.
if type(n_gg) == "function" then
    local function scripts_com_env(aceita)
        local vistos, lista = {}, {}
        for _, t in n_gg(true) do
            local s = type(t) == "table" and rawget(t, "script")
            if typeof(s) == "Instance" and not vistos[s] and aceita(s) then
                vistos[s] = true
                lista[#lista + 1] = s
            end
        end
        return lista
    end

    publicar("getsenv", marcar(function(script)
        checar_instancia(script, 1, "getsenv")
        for _, t in n_gg(true) do
            if type(t) == "table" and rawget(t, "script") == script then
                return t
            end
        end
        error("invalid argument #1 to 'getsenv' (script has no environment)", 2)
    end))

    publicar("getrunningscripts", marcar(function()
        return scripts_com_env(function(s)
            return s:IsA("LocalScript") or s:IsA("ModuleScript")
        end)
    end))

    -- aproximação: módulo com env vivo foi carregado; require sem env não aparece
    publicar("getloadedmodules", marcar(function()
        return scripts_com_env(function(s)
            return s:IsA("ModuleScript")
        end)
    end))
end

-- FramerateCap do UserGameSettings aceita escrita e leitura de volta (sondado ao vivo:
-- escreve 60 -> lê 60, escreve 0 -> lê 0). 0 = sem limite.
publicar("setfpscap", marcar(function(fps)
    checar(fps, "number", 1, "setfpscap")
    UserSettings():GetService("UserGameSettings").FramerateCap = fps
end))

if type(n_srm) == "function" and type(n_sro) == "function" then
    publicar("setrawmetatable", marcar(function(o, mt)
        local t = checar_objeto(o, 1, "setrawmetatable")
        if mt ~= nil then
            checar(mt, "table", 2, "setrawmetatable")
        end
        -- o nativo ignora tabela congelada em silêncio, então destrava só na troca
        local congelada = t == "table" and table.isfrozen(o)
        if congelada then n_sro(o, false) end
        n_srm(o, mt) -- sempre 2 args: o nativo exige exatamente 2
        if congelada then n_sro(o, true) end
        return o
    end))
end

-- Troca o metamétodo direto na metatable real. Userdata (ex.: __namecall do game) não
-- passa por aqui: exige hook nativo no namecall, então erra em vez de fingir.
if type(n_grm) == "function" and type(n_sro) == "function" then
    publicar("hookmetamethod", marcar(function(o, nome, hook)
        checar_objeto(o, 1, "hookmetamethod")
        checar(nome, "string", 2, "hookmetamethod")
        checar(hook, "function", 3, "hookmetamethod")
        if type(o) ~= "table" then
            error("hookmetamethod: userdata not supported (__namecall needs the native)", 2)
        end
        local mt = n_grm(o)
        if type(mt) ~= "table" then
            error("invalid argument #1 to 'hookmetamethod' (table has no metatable)", 2)
        end
        local antigo = mt[nome]
        local congelada = table.isfrozen(mt)
        n_sro(mt, false)
        mt[nome] = hook
        n_sro(mt, congelada)
        return antigo
    end))
end

local function alvo(t)
    return refs[t] or t
end

-- cloneref devolve um proxy de tabela: identidade diferente, mesmo Instance
-- por trás. Não é o userdata original (isso só com código nativo), mas toda
-- leitura/escrita chega no objeto certo.
publicar("cloneref", marcar(function(obj)
    if typeof(obj) ~= "Instance" then
        return obj
    end
    return setmetatable({}, {
        __index = function(_, k)
            return alvo(obj)[k]
        end,
        __newindex = function(_, k, v)
            alvo(obj)[k] = v
        end,
        __tostring = function()
            return tostring(alvo(obj))
        end,
    })
end))

publicar("compareinstances", marcar(function(a, b)
    return alvo(a) == alvo(b) or tostring(alvo(a)) == tostring(alvo(b))
end))

-- Drawing -------------------------------------------------------------------

-- Desenho de verdade, mas com GUI: cada objeto vira Frame/TextLabel/ImageLabel
-- (+ UIStroke/UICorner) numa ScreenGui que ignora o inset, então a coordenada é
-- a do viewport, igual à Drawing nativa. Precisa de capability para criar
-- Instance (na thread de CorePackages não há).
-- shortcut: Triangle/Quad com Filled saem só com contorno (GUI 2D não tem
-- polígono); trocar por EditableImage se algum script depender disso.
do
    local vivos = {}
    local registro = setmetatable({}, { __mode = "k" })
    local tela

    local function container()
        if tela and tela.Parent then
            return tela
        end
        local g = Instance.new("ScreenGui")
        g.Name = tostring(math.random(1e8, 9e8))
        g.IgnoreGuiInset = true
        g.ResetOnSpawn = false
        g.DisplayOrder = 10000
        g.ZIndexBehavior = Enum.ZIndexBehavior.Sibling
        g.Parent = env.gethui()
        tela = g
        return g
    end

    local function criar(o, classe)
        local f = Instance.new(classe)
        f.BorderSizePixel = 0
        f.BackgroundTransparency = 1
        f.Visible = false
        f.Parent = container()
        o.insts[#o.insts + 1] = f
        return f
    end

    local function contorno(pai, modo)
        local s = Instance.new("UIStroke")
        s.ApplyStrokeMode = modo or Enum.ApplyStrokeMode.Border
        s.Parent = pai
        return s
    end

    local function px(v)
        return UDim2.fromOffset(v.X, v.Y)
    end

    local function preencher(o, p)
        local alfa = 1 - p.Transparency
        o.f.BackgroundColor3 = p.Color
        o.f.BackgroundTransparency = p.Filled and alfa or 1
        o.s.Enabled = not p.Filled
        o.s.Color = p.Color
        o.s.Thickness = p.Thickness
        o.s.Transparency = alfa
    end

    local function segmento(f, a, b, p)
        local d = b - a
        f.Position = UDim2.fromOffset((a.X + b.X) / 2, (a.Y + b.Y) / 2)
        f.Size = UDim2.fromOffset(d.Magnitude, p.Thickness)
        f.Rotation = math.deg(math.atan2(d.Y, d.X))
        f.BackgroundColor3 = p.Color
        f.BackgroundTransparency = 1 - p.Transparency
    end

    local function poligono(pontos)
        return {
            padrao = (function()
                local t = { Thickness = 1, Filled = false }
                for _, n in pontos do
                    t[n] = Vector2.zero
                end
                return t
            end)(),
            montar = function(o)
                o.linhas = {}
                for i = 1, #pontos do
                    o.linhas[i] = criar(o, "Frame")
                    o.linhas[i].AnchorPoint = Vector2.new(0.5, 0.5)
                end
            end,
            desenhar = function(o, p)
                for i, n in pontos do
                    segmento(o.linhas[i], p[n], p[pontos[i % #pontos + 1]], p)
                end
            end,
        }
    end

    local FONTES = { [0] = Enum.Font.SourceSans, [1] = Enum.Font.Arial, [2] = Enum.Font.Gotham, [3] = Enum.Font.Code }

    local TIPOS = {
        Line = {
            padrao = { From = Vector2.zero, To = Vector2.zero, Thickness = 1 },
            montar = function(o)
                o.f = criar(o, "Frame")
                o.f.AnchorPoint = Vector2.new(0.5, 0.5)
            end,
            desenhar = function(o, p)
                segmento(o.f, p.From, p.To, p)
            end,
        },
        Square = {
            padrao = { Position = Vector2.zero, Size = Vector2.zero, Thickness = 1, Filled = false },
            montar = function(o)
                o.f = criar(o, "Frame")
                o.s = contorno(o.f)
            end,
            desenhar = function(o, p)
                o.f.Position = px(p.Position)
                o.f.Size = px(p.Size)
                preencher(o, p)
            end,
        },
        Circle = {
            padrao = { Position = Vector2.zero, Radius = 0, Thickness = 1, Filled = false, NumSides = 0 },
            montar = function(o)
                o.f = criar(o, "Frame")
                o.f.AnchorPoint = Vector2.new(0.5, 0.5)
                local canto = Instance.new("UICorner")
                canto.CornerRadius = UDim.new(1, 0)
                canto.Parent = o.f
                o.s = contorno(o.f)
            end,
            desenhar = function(o, p)
                o.f.Position = px(p.Position)
                o.f.Size = UDim2.fromOffset(p.Radius * 2, p.Radius * 2)
                preencher(o, p)
            end,
        },
        Text = {
            padrao = {
                Text = "", Position = Vector2.zero, Size = 18, Center = false, Outline = false,
                OutlineColor = Color3.new(0, 0, 0), Font = 0,
            },
            montar = function(o)
                o.f = criar(o, "TextLabel")
                o.f.AutomaticSize = Enum.AutomaticSize.XY
                o.f.TextXAlignment = Enum.TextXAlignment.Left
                o.f.TextYAlignment = Enum.TextYAlignment.Top
                o.s = contorno(o.f, Enum.ApplyStrokeMode.Contextual)
            end,
            desenhar = function(o, p)
                o.f.Text = p.Text
                o.f.TextSize = p.Size
                o.f.Font = FONTES[p.Font] or (typeof(p.Font) == "EnumItem" and p.Font) or Enum.Font.SourceSans
                o.f.TextColor3 = p.Color
                o.f.TextTransparency = 1 - p.Transparency
                o.f.AnchorPoint = p.Center and Vector2.new(0.5, 0) or Vector2.zero
                o.f.Position = px(p.Position)
                o.s.Enabled = p.Outline
                o.s.Color = p.OutlineColor
                o.s.Transparency = 1 - p.Transparency
            end,
        },
        Image = {
            padrao = { Data = "", Position = Vector2.zero, Size = Vector2.zero, Rounding = 0 },
            montar = function(o)
                o.f = criar(o, "ImageLabel")
                o.c = Instance.new("UICorner")
                o.c.Parent = o.f
            end,
            desenhar = function(o, p)
                o.f.Position = px(p.Position)
                o.f.Size = px(p.Size)
                o.f.ImageColor3 = p.Color
                o.f.ImageTransparency = 1 - p.Transparency
                o.c.CornerRadius = UDim.new(0, p.Rounding)
                -- bytes crus de imagem precisariam de getcustomasset; só id/URL de asset
                if p.Data:match("^rbxasset") or p.Data:match("^https?://") then
                    o.f.Image = p.Data
                end
            end,
        },
        Triangle = poligono({ "PointA", "PointB", "PointC" }),
        Quad = poligono({ "PointA", "PointB", "PointC", "PointD" }),
    }

    local function novo(tipo)
        checar(tipo, "string", 1, "Drawing.new")
        local def = TIPOS[tipo]
        if not def then
            error("invalid argument #1 to 'Drawing.new' (invalid drawing type '" .. tipo .. "')", 2)
        end
        local p = { Visible = false, ZIndex = 1, Transparency = 1, Color = Color3.new(1, 1, 1) }
        for k, v in def.padrao do
            p[k] = v
        end
        local o = { insts = {} }
        def.montar(o)
        local obj, vivo = {}, true
        local function redesenhar()
            def.desenhar(o, p)
            for _, i in o.insts do
                i.Visible = p.Visible
                i.ZIndex = p.ZIndex
            end
        end
        local function remover()
            if vivo then
                vivo = false
                vivos[obj] = nil
                for _, i in o.insts do
                    i:Destroy()
                end
            end
        end
        setmetatable(obj, {
            __index = function(_, k)
                if k == "Remove" or k == "Destroy" then
                    return remover
                elseif k == "TextBounds" and tipo == "Text" then
                    return o.f.TextBounds
                elseif k == "__OBJECT_EXISTS" then
                    return vivo
                end
                return p[k]
            end,
            __newindex = function(_, k, v)
                local atual = p[k]
                if atual == nil then
                    error(tostring(k) .. " is not a valid member of " .. tipo, 2)
                end
                if k ~= "Font" and typeof(v) ~= typeof(atual) then
                    error(string.format("invalid value for %s.%s (%s expected, got %s)", tipo, k, typeof(atual), typeof(v)), 2)
                end
                p[k] = v
                if vivo then
                    redesenhar()
                end
            end,
            __tostring = function()
                return "Drawing"
            end,
        })
        registro[obj] = true
        vivos[obj] = true
        redesenhar()
        return obj
    end

    publicar("Drawing", {
        new = marcar(novo),
        Fonts = { UI = 0, System = 1, Plex = 2, Monospace = 3 },
    })

    local function checar_desenho(obj, nome)
        if not registro[obj] then
            error("invalid argument #1 to '" .. nome .. "' (Drawing expected, got " .. typeof(obj) .. ")", 3)
        end
    end

    publicar("isrenderobj", marcar(function(obj)
        return registro[obj] == true
    end))

    publicar("getrenderproperty", marcar(function(obj, prop)
        checar_desenho(obj, "getrenderproperty")
        return obj[prop]
    end))

    publicar("setrenderproperty", marcar(function(obj, prop, valor)
        checar_desenho(obj, "setrenderproperty")
        obj[prop] = valor
    end))

    publicar("cleardrawcache", marcar(function()
        for obj in vivos do
            obj:Remove()
        end
    end))
end

-- Cache ---------------------------------------------------------------------

publicar("cache", {
    iscached = marcar(function(obj)
        return invalidados[obj] ~= true
    end),
    invalidate = marcar(function(obj)
        invalidados[obj] = true
    end),
    replace = marcar(function(a, b)
        refs[b] = a
        return a
    end),
})

-- debug.getinfo no formato da UNC (o `debug` do Luau não tem esse nome) ------

-- A nativa devolve 0 valores fora da faixa (ou em C closure): o select conta isso sem
-- confundir com constante nil.
local function contar_valores(...)
    return select("#", ...), (...)
end

-- Import e tabela/closure de constante não são valor Lua estável: a UNC espera nil nesse slot.
local function constante_exposta(valor)
    local t = type(valor)
    if t == "nil" or t == "boolean" or t == "number" or t == "string" or t == "vector" then
        return valor
    end
    return nil
end

if type(debug) == "table" and type(debug.info) == "function" then
    local debug_real = debug
    local debug_api = setmetatable({
        getinfo = marcar(function(f)
            if type(f) ~= "function" then
                return nil
            end
            local info = {
                func = f,
                what = "Lua",
                nups = 0,
                currentline = 0,
                name = "",
                numparams = 0,
                is_vararg = 0,
            }
            local src = debug_real.info(f, "s")
            if type(src) == "string" then
                info.source = src
                info.short_src = src
            end
            local linha = debug_real.info(f, "l")
            if type(linha) == "number" then
                info.currentline = linha
            end
            local nome = debug_real.info(f, "n")
            if type(nome) == "string" then
                info.name = nome
            end
            local params, vararg = debug_real.info(f, "a")
            if type(params) == "number" then
                info.numparams = params
            end
            if vararg == true then
                info.is_vararg = 1
            end
            return info
        end),
    }, { __index = debug_real })

    if type(n_gk) == "function" then
        debug_api.getconstant = marcar(function(f, i)
            checar(f, "function", 1, "getconstant")
            checar(i, "number", 2, "getconstant")
            if i % 1 == 0 then
                local qtd, valor = contar_valores(n_gk(f, i))
                if qtd > 0 then
                    return constante_exposta(valor)
                end
            end
            error("invalid argument #2 to 'getconstant' (index out of range)", 2)
        end)

        debug_api.getconstants = marcar(function(f)
            checar(f, "function", 1, "getconstants")
            local lista = {}
            local i = 1
            while true do
                local qtd, valor = contar_valores(n_gk(f, i))
                if qtd == 0 then
                    return lista
                end
                lista[i] = constante_exposta(valor)
                i = i + 1
            end
        end)
    end

    if type(n_gu) == "function" then
        debug_api.getupvalue = marcar(function(f, i)
            checar(f, "function", 1, "getupvalue")
            checar(i, "number", 2, "getupvalue")
            if i % 1 == 0 then
                local qtd, valor = contar_valores(n_gu(f, i))
                if qtd > 0 then
                    return valor
                end
            end
            error("invalid argument #2 to 'getupvalue' (index out of range)", 2)
        end)

        debug_api.getupvalues = marcar(function(f)
            checar(f, "function", 1, "getupvalues")
            local lista = {}
            local i = 1
            while true do
                local qtd, valor = contar_valores(n_gu(f, i))
                if qtd == 0 then
                    return lista
                end
                lista[i] = valor
                i = i + 1
            end
        end)
    end

    -- Sem pcall: o nativo devolve 1 (true) ou 0 valores. O tipo é conferido antes só para a mensagem.
    if type(n_sk) == "function" and type(n_gk) == "function" then
        debug_api.setconstant = marcar(function(f, i, v)
            checar(f, "function", 1, "setconstant")
            checar(i, "number", 2, "setconstant")
            if i % 1 == 0 then
                local qtd, atual = contar_valores(n_gk(f, i))
                if qtd > 0 then
                    local exposta = constante_exposta(atual)
                    if exposta ~= nil and type(exposta) ~= type(v) then
                        error("setconstant: type incompatible with the constant", 2)
                    end
                    if contar_valores(n_sk(f, i, v)) > 0 then
                        return
                    end
                end
            end
            error("invalid argument #2 to 'setconstant' (index out of range)", 2)
        end)
    end

    if type(n_su) == "function" then
        debug_api.setupvalue = marcar(function(f, i, v)
            checar(f, "function", 1, "setupvalue")
            checar(i, "number", 2, "setupvalue")
            if i % 1 == 0 and contar_valores(n_su(f, i, v)) > 0 then
                return
            end
            error("invalid argument #2 to 'setupvalue' (index out of range)", 2)
        end)
    end

    publicar("debug", debug_api)
end

-- Crypt ---------------------------------------------------------------------

local b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local rev = {}
for i = 1, 64 do
    rev[b64:byte(i)] = i - 1
end

local function base64_encode(s)
    local out = {}
    for i = 1, #s, 3 do
        local a, b, c = s:byte(i, i + 2)
        local n = (a or 0) * 65536 + (b or 0) * 256 + (c or 0)
        local c1 = math.floor(n / 262144) % 64
        local c2 = math.floor(n / 4096) % 64
        local c3 = math.floor(n / 64) % 64
        local c4 = n % 64
        out[#out + 1] = b64:sub(c1 + 1, c1 + 1)
        out[#out + 1] = b64:sub(c2 + 1, c2 + 1)
        out[#out + 1] = b and b64:sub(c3 + 1, c3 + 1) or "="
        out[#out + 1] = c and b64:sub(c4 + 1, c4 + 1) or "="
    end
    return table.concat(out)
end

local function base64_decode(s)
    local out = {}
    for i = 1, #s, 4 do
        local c1, c2, c3, c4 = s:byte(i, i + 3)
        local n1, n2 = rev[c1] or 0, rev[c2] or 0
        local n3 = (c3 == 61 or c3 == nil) and 0 or (rev[c3] or 0)
        local n4 = (c4 == 61 or c4 == nil) and 0 or (rev[c4] or 0)
        local n = n1 * 262144 + n2 * 4096 + n3 * 64 + n4
        out[#out + 1] = string.char(math.floor(n / 65536) % 256)
        if c3 ~= 61 and c3 ~= nil then
            out[#out + 1] = string.char(math.floor(n / 256) % 256)
        end
        if c4 ~= 61 and c4 ~= nil then
            out[#out + 1] = string.char(n % 256)
        end
    end
    return table.concat(out)
end

local rng = Random.new()

local function bytes_aleatorios(n)
    local out = {}
    for i = 1, n do
        out[i] = string.char(rng:NextInteger(0, 255))
    end
    return table.concat(out)
end

-- Hashes (MD5, SHA-1, SHA-2, SHA-3) e AES em Luau puro, com bit32. Os SHA-2
-- de 64 bits andam em pares (alto, baixo) de 32 bits porque número do Luau é
-- double. Constantes geradas uma vez no primeiro uso.
local band, bor, bxor, bnot = bit32.band, bit32.bor, bit32.bxor, bit32.bnot
local lsh, rsh, lrot, rrot = bit32.lshift, bit32.rshift, bit32.lrotate, bit32.rrotate
local DOIS32 = 4294967296

local function hex(s)
    return (s:gsub(".", function(c)
        return string.format("%02x", c:byte())
    end))
end

-- K do SHA-512 (80), IV do SHA-512 (8) e IV do SHA-384 (8); o SHA-256 usa a
-- metade alta dos mesmos números e o SHA-224 a metade baixa do IV do 384.
local CONST_SHA2 = [[
428a2f98d728ae227137449123ef65cdb5c0fbcfec4d3b2fe9b5dba58189dbbc3956c25bf348b53859f111f1b605d019
923f82a4af194f9bab1c5ed5da6d8118d807aa98a303024212835b0145706fbe243185be4ee4b28c550c7dc3d5ffb4e2
72be5d74f27b896f80deb1fe3b1696b19bdc06a725c71235c19bf174cf692694e49b69c19ef14ad2efbe4786384f25e3
0fc19dc68b8cd5b5240ca1cc77ac9c652de92c6f592b02754a7484aa6ea6e4835cb0a9dcbd41fbd476f988da831153b5
983e5152ee66dfaba831c66d2db43210b00327c898fb213fbf597fc7beef0ee4c6e00bf33da88fc2d5a79147930aa725
06ca6351e003826f142929670a0e6e7027b70a8546d22ffc2e1b21385c26c9264d2c6dfc5ac42aed53380d139d95b3df
650a73548baf63de766a0abb3c77b2a881c2c92e47edaee692722c851482353ba2bfe8a14cf10364a81a664bbc423001
c24b8b70d0f89791c76c51a30654be30d192e819d6ef5218d69906245565a910f40e35855771202a106aa07032bbd1b8
19a4c116b8d2d0c81e376c085141ab532748774cdf8eeb9934b0bcb5e19b48a8391c0cb3c5c95a634ed8aa4ae3418acb
5b9cca4f7763e373682e6ff3d6b2b8a3748f82ee5defb2fc78a5636f43172f6084c87814a1f0ab728cc702081a6439ec
90befffa23631e28a4506cebde82bde9bef9a3f7b2c67915c67178f2e372532bca273eceea26619cd186b8c721c0c207
eada7dd6cde0eb1ef57d4f7fee6ed17806f067aa72176fba0a637dc5a2c898a6113f9804bef90dae1b710b35131c471b
28db77f523047d8432caab7b40c724933c9ebe0a15c9bebc431d67c49c100d4c4cc5d4becb3e42b6597f299cfc657e2a
5fcb6fab3ad6faec6c44198c4a4758176a09e667f3bcc908bb67ae8584caa73b3c6ef372fe94f82ba54ff53a5f1d36f1
510e527fade682d19b05688c2b3e6c1f1f83d9abfb41bd6b5be0cd19137e2179cbbb9d5dc1059ed8629a292a367cd507
9159015a3070dd17152fecd8f70e593967332667ffc00b318eb44a8768581511db0c2e0d64f98fa747b5481dbefa4fa4
]]
local CONST_MD5 = [[
d76aa478e8c7b756242070dbc1bdceeef57c0faf4787c62aa8304613fd469501698098d88b44f7afffff5bb1895cd7be
6b901122fd987193a679438e49b40821f61e2562c040b340265e5a51e9b6c7aad62f105d02441453d8a1e681e7d3fbc8
21e1cde6c33707d6f4d50d87455a14eda9e3e905fcefa3f8676f02d98d2a4c8afffa39428771f6816d9d6122fde5380c
a4beea444bdecfa9f6bb4b60bebfbc70289b7ec6eaa127fad4ef308504881d05d9d4d039e6db99e51fa27cf8c4ac5665
f4292244432aff97ab9423a7fc93a039655b59c38f0ccc92ffeff47d85845dd16fa87e4ffe2ce6e0a30143144e0811a1
f7537e82bd3af2352ad7d2bbeb86d391
]]
local MD5_S = { 7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21 }
local K2H, K2L, MD5_K

local function palavras(texto, digitos)
    local t = {}
    texto = texto:gsub("%s", "")
    for i = 1, #texto, digitos do
        t[#t + 1] = tonumber(texto:sub(i, i + digitos - 1), 16)
    end
    return t
end

local function carregar_hash()
    if K2H then
        return
    end
    local w = palavras(CONST_SHA2, 8)
    K2H, K2L = {}, {}
    for i = 1, #w / 2 do
        K2H[i], K2L[i] = w[2 * i - 1], w[2 * i]
    end
    MD5_K = palavras(CONST_MD5, 8)
end

-- Padding Merkle-Damgård: 0x80, zeros e o tamanho em bits no fim do bloco.
local function preencher(msg, bloco, little)
    local campo = bloco == 128 and 16 or 8
    local zeros = (bloco - (#msg + 1 + campo) % bloco) % bloco
    local bits = #msg * 8
    local tam = little and string.pack("<I4I4", bits % DOIS32, math.floor(bits / DOIS32))
        or string.pack(">I4I4", math.floor(bits / DOIS32), bits % DOIS32)
    return msg .. "\128" .. string.rep("\0", zeros + campo - 8) .. tam
end

local function md5(msg)
    local m = preencher(msg, 64, true)
    local h0, h1, h2, h3 = 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476
    for i = 1, #m, 64 do
        local x = { string.unpack("<I4I4I4I4I4I4I4I4I4I4I4I4I4I4I4I4", m, i) }
        local a, b, c, d = h0, h1, h2, h3
        for j = 0, 63 do
            local f, g
            if j < 16 then
                f, g = bor(band(b, c), band(bnot(b), d)), j
            elseif j < 32 then
                f, g = bor(band(d, b), band(bnot(d), c)), (5 * j + 1) % 16
            elseif j < 48 then
                f, g = bxor(b, c, d), (3 * j + 5) % 16
            else
                f, g = bxor(c, bor(b, bnot(d))), (7 * j) % 16
            end
            f = (f + a + MD5_K[j + 1] + x[g + 1]) % DOIS32
            a, d, c = d, c, b
            b = (b + lrot(f, MD5_S[math.floor(j / 16) * 4 + j % 4 + 1])) % DOIS32
        end
        h0, h1, h2, h3 = (h0 + a) % DOIS32, (h1 + b) % DOIS32, (h2 + c) % DOIS32, (h3 + d) % DOIS32
    end
    return hex(string.pack("<I4I4I4I4", h0, h1, h2, h3))
end

local function sha1(msg)
    local m = preencher(msg, 64)
    local h0, h1, h2, h3, h4 = 0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0
    local w = {}
    for i = 1, #m, 64 do
        for j = 1, 16 do
            w[j] = string.unpack(">I4", m, i + (j - 1) * 4)
        end
        for j = 17, 80 do
            w[j] = lrot(bxor(w[j - 3], w[j - 8], w[j - 14], w[j - 16]), 1)
        end
        local a, b, c, d, e = h0, h1, h2, h3, h4
        for j = 1, 80 do
            local f, k
            if j <= 20 then
                f, k = bor(band(b, c), band(bnot(b), d)), 0x5a827999
            elseif j <= 40 then
                f, k = bxor(b, c, d), 0x6ed9eba1
            elseif j <= 60 then
                f, k = bor(band(b, c), band(b, d), band(c, d)), 0x8f1bbcdc
            else
                f, k = bxor(b, c, d), 0xca62c1d6
            end
            a, b, c, d, e = (lrot(a, 5) + f + e + k + w[j]) % DOIS32, a, lrot(b, 30), c, d
        end
        h0, h1, h2 = (h0 + a) % DOIS32, (h1 + b) % DOIS32, (h2 + c) % DOIS32
        h3, h4 = (h3 + d) % DOIS32, (h4 + e) % DOIS32
    end
    return hex(string.pack(">I4I4I4I4I4", h0, h1, h2, h3, h4))
end

local function sha256(msg, iv, saida)
    local m = preencher(msg, 64)
    local h = table.clone(iv)
    local w = {}
    for i = 1, #m, 64 do
        for j = 1, 16 do
            w[j] = string.unpack(">I4", m, i + (j - 1) * 4)
        end
        for j = 17, 64 do
            local x, y = w[j - 15], w[j - 2]
            local s0 = bxor(rrot(x, 7), rrot(x, 18), rsh(x, 3))
            local s1 = bxor(rrot(y, 17), rrot(y, 19), rsh(y, 10))
            w[j] = (w[j - 16] + s0 + w[j - 7] + s1) % DOIS32
        end
        local a, b, c, d, e, f, g, hh = h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]
        for j = 1, 64 do
            local s1 = bxor(rrot(e, 6), rrot(e, 11), rrot(e, 25))
            local ch = bxor(band(e, f), band(bnot(e), g))
            local t1 = hh + s1 + ch + K2H[j] + w[j]
            local s0 = bxor(rrot(a, 2), rrot(a, 13), rrot(a, 22))
            local t2 = s0 + bxor(band(a, b), band(a, c), band(b, c))
            hh, g, f, e, d, c, b, a = g, f, e, (d + t1) % DOIS32, c, b, a, (t1 + t2) % DOIS32
        end
        for k, v in { a, b, c, d, e, f, g, hh } do
            h[k] = (h[k] + v) % DOIS32
        end
    end
    local partes = {}
    for k = 1, saida do
        partes[k] = string.pack(">I4", h[k])
    end
    return hex(table.concat(partes))
end

-- Rotação/deslocamento à direita de um inteiro de 64 bits em (alto, baixo).
local function rotr64(h, l, n)
    if n >= 32 then
        h, l, n = l, h, n - 32
    end
    if n == 0 then
        return h, l
    end
    return bor(rsh(h, n), lsh(l, 32 - n)), bor(rsh(l, n), lsh(h, 32 - n))
end

local function shr64(h, l, n)
    return rsh(h, n), bor(rsh(l, n), lsh(h, 32 - n))
end

local function sha512(msg, ivh, ivl, saida)
    local m = preencher(msg, 128)
    local hh, hl = table.clone(ivh), table.clone(ivl)
    local wh, wl = {}, {}
    for i = 1, #m, 128 do
        for j = 1, 16 do
            wh[j], wl[j] = string.unpack(">I4I4", m, i + (j - 1) * 8)
        end
        for j = 17, 80 do
            local xh, xl = wh[j - 15], wl[j - 15]
            local ah, al = rotr64(xh, xl, 1)
            local bh, bl = rotr64(xh, xl, 8)
            local ch, cl = shr64(xh, xl, 7)
            local s0h, s0l = bxor(ah, bh, ch), bxor(al, bl, cl)
            xh, xl = wh[j - 2], wl[j - 2]
            ah, al = rotr64(xh, xl, 19)
            bh, bl = rotr64(xh, xl, 61)
            ch, cl = shr64(xh, xl, 6)
            local s1h, s1l = bxor(ah, bh, ch), bxor(al, bl, cl)
            local l = wl[j - 16] + s0l + wl[j - 7] + s1l
            wl[j] = l % DOIS32
            wh[j] = (wh[j - 16] + s0h + wh[j - 7] + s1h + math.floor(l / DOIS32)) % DOIS32
        end
        local ah, bh, ch, dh, eh, fh, gh, hhh = hh[1], hh[2], hh[3], hh[4], hh[5], hh[6], hh[7], hh[8]
        local al, bl, cl, dl, el, fl, gl, hhl = hl[1], hl[2], hl[3], hl[4], hl[5], hl[6], hl[7], hl[8]
        for j = 1, 80 do
            local r1h, r1l = rotr64(eh, el, 14)
            local r2h, r2l = rotr64(eh, el, 18)
            local r3h, r3l = rotr64(eh, el, 41)
            local chh = bxor(band(eh, fh), band(bnot(eh), gh))
            local chl = bxor(band(el, fl), band(bnot(el), gl))
            local t1l = hhl + bxor(r1l, r2l, r3l) + chl + K2L[j] + wl[j]
            local t1h = hhh + bxor(r1h, r2h, r3h) + chh + K2H[j] + wh[j] + math.floor(t1l / DOIS32)
            t1l = t1l % DOIS32
            r1h, r1l = rotr64(ah, al, 28)
            r2h, r2l = rotr64(ah, al, 34)
            r3h, r3l = rotr64(ah, al, 39)
            local t2l = bxor(r1l, r2l, r3l) + bxor(band(al, bl), band(al, cl), band(bl, cl))
            local t2h = bxor(r1h, r2h, r3h) + bxor(band(ah, bh), band(ah, ch), band(bh, ch))
            hhh, hhl, gh, gl, fh, fl = gh, gl, fh, fl, eh, el
            local el2 = dl + t1l
            eh, el = (dh + t1h + math.floor(el2 / DOIS32)) % DOIS32, el2 % DOIS32
            dh, dl, ch, cl, bh, bl = ch, cl, bh, bl, ah, al
            local al2 = t1l + t2l
            ah, al = (t1h + t2h + math.floor(al2 / DOIS32)) % DOIS32, al2 % DOIS32
        end
        local novos = { ah, al, bh, bl, ch, cl, dh, dl, eh, el, fh, fl, gh, gl, hhh, hhl }
        for k = 1, 8 do
            local l = hl[k] + novos[2 * k]
            hh[k] = (hh[k] + novos[2 * k - 1] + math.floor(l / DOIS32)) % DOIS32
            hl[k] = l % DOIS32
        end
    end
    local partes = {}
    for k = 1, saida do
        partes[k] = string.pack(">I4I4", hh[k], hl[k])
    end
    return hex(table.concat(partes))
end

-- Keccak-f[1600]: 25 lanes de 64 bits em (alto, baixo), índice x + 5y + 1.
local ROT_KECCAK = { 0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14 }
local RCH, RCL

local function carregar_keccak()
    if RCH then
        return
    end
    RCH, RCL = {}, {}
    local r = 1
    for i = 1, 24 do
        local h, l = 0, 0
        for j = 0, 6 do
            local bit = r % 2
            r *= 2
            if r >= 256 then
                r = bxor(r, 0x171)
            end
            if bit == 1 then
                local pos = 2 ^ j - 1
                if pos >= 32 then
                    h = bor(h, lsh(1, pos - 32))
                else
                    l = bor(l, lsh(1, pos))
                end
            end
        end
        RCH[i], RCL[i] = h, l
    end
end

local function keccak(ah, al)
    local ch, cl, bh, bl = {}, {}, {}, {}
    for r = 1, 24 do
        for x = 1, 5 do
            ch[x] = bxor(ah[x], ah[x + 5], ah[x + 10], ah[x + 15], ah[x + 20])
            cl[x] = bxor(al[x], al[x + 5], al[x + 10], al[x + 15], al[x + 20])
        end
        for x = 0, 4 do
            local nh, nl = ch[(x + 1) % 5 + 1], cl[(x + 1) % 5 + 1]
            local dh = bxor(ch[(x + 4) % 5 + 1], bor(lsh(nh, 1), rsh(nl, 31)))
            local dl = bxor(cl[(x + 4) % 5 + 1], bor(lsh(nl, 1), rsh(nh, 31)))
            for y = 0, 20, 5 do
                ah[x + y + 1] = bxor(ah[x + y + 1], dh)
                al[x + y + 1] = bxor(al[x + y + 1], dl)
            end
        end
        for x = 0, 4 do
            for y = 0, 4 do
                local i = x + 5 * y + 1
                local n = ROT_KECCAK[i]
                local h, l = ah[i], al[i]
                if n ~= 0 then
                    h, l = rotr64(h, l, 64 - n)
                end
                local destino = y + 5 * ((2 * x + 3 * y) % 5) + 1
                bh[destino], bl[destino] = h, l
            end
        end
        for y = 0, 20, 5 do
            for x = 0, 4 do
                local i, i1, i2 = y + x + 1, y + (x + 1) % 5 + 1, y + (x + 2) % 5 + 1
                ah[i] = bxor(bh[i], band(bnot(bh[i1]), bh[i2]))
                al[i] = bxor(bl[i], band(bnot(bl[i1]), bl[i2]))
            end
        end
        ah[1], al[1] = bxor(ah[1], RCH[r]), bxor(al[1], RCL[r])
    end
end

local function sha3(msg, bits)
    carregar_keccak()
    local taxa = 200 - bits / 4
    local falta = taxa - #msg % taxa
    local m = falta == 1 and msg .. "\134" or msg .. "\6" .. string.rep("\0", falta - 2) .. "\128"
    local ah, al = table.create(25, 0), table.create(25, 0)
    for i = 1, #m, taxa do
        for k = 0, taxa / 8 - 1 do
            local l, h = string.unpack("<I4I4", m, i + k * 8)
            ah[k + 1], al[k + 1] = bxor(ah[k + 1], h), bxor(al[k + 1], l)
        end
        keccak(ah, al)
    end
    local partes = {}
    for k = 1, math.ceil(bits / 64) do
        partes[k] = string.pack("<I4I4", al[k], ah[k])
    end
    return hex(table.concat(partes):sub(1, bits / 8))
end

local HASHES = {
    md5 = md5,
    sha1 = sha1,
    sha224 = function(m) return sha256(m, { table.unpack(K2L, 89, 96) }, 7) end,
    sha256 = function(m) return sha256(m, { table.unpack(K2H, 81, 88) }, 8) end,
    sha384 = function(m) return sha512(m, { table.unpack(K2H, 89, 96) }, { table.unpack(K2L, 89, 96) }, 6) end,
    sha512 = function(m) return sha512(m, { table.unpack(K2H, 81, 88) }, { table.unpack(K2L, 81, 88) }, 8) end,
}
for _, n in { 224, 256, 384, 512 } do
    HASHES["sha3-" .. n] = function(m) return sha3(m, n) end
end

local function hash(dados, algoritmo)
    checar(dados, "string", 1, "crypt.hash")
    checar(algoritmo, "string", 2, "crypt.hash")
    local f = HASHES[algoritmo:lower():gsub("_", "-"):gsub("^sha%-(%d)", "sha%1")]
    if not f then
        error("invalid argument #2 to 'crypt.hash' (unsupported algorithm '" .. algoritmo .. "')", 2)
    end
    carregar_hash()
    return f(dados)
end

-- AES -----------------------------------------------------------------------

local SBOX, INV, M2, M3, M9, M11, M13, M14

local function mul(a, b)
    local p = 0
    while b > 0 do
        if b % 2 == 1 then
            p = bxor(p, a)
        end
        a = band(bxor(lsh(a, 1), band(a, 0x80) ~= 0 and 0x1b or 0), 0xff)
        b = rsh(b, 1)
    end
    return p
end

local function carregar_aes()
    if SBOX then
        return
    end
    SBOX, INV, M2, M3, M9, M11, M13, M14 = {}, {}, {}, {}, {}, {}, {}, {}
    local function rot8(x, s)
        return band(bor(lsh(x, s), rsh(x, 8 - s)), 0xff)
    end
    -- gera a S-box pelo inverso multiplicativo em GF(2^8) (p anda por 3, q por 1/3)
    local p, q = 1, 1
    repeat
        p = band(bxor(p, lsh(p, 1), band(p, 0x80) ~= 0 and 0x1b or 0), 0xff)
        q = band(bxor(q, lsh(q, 1)), 0xff)
        q = band(bxor(q, lsh(q, 2)), 0xff)
        q = band(bxor(q, lsh(q, 4)), 0xff)
        if band(q, 0x80) ~= 0 then
            q = bxor(q, 0x09)
        end
        local x = bxor(q, rot8(q, 1), rot8(q, 2), rot8(q, 3), rot8(q, 4), 0x63)
        SBOX[p], INV[x] = x, p
    until p == 1
    SBOX[0], INV[0x63] = 0x63, 0
    for i = 0, 255 do
        M2[i], M3[i], M9[i] = mul(i, 2), mul(i, 3), mul(i, 9)
        M11[i], M13[i], M14[i] = mul(i, 11), mul(i, 13), mul(i, 14)
    end
end

local function expandir(chave)
    local nk = #chave / 4
    local nr = nk + 6
    local w = table.clone(chave)
    local rcon = 1
    for i = nk, 4 * (nr + 1) - 1 do
        local p = (i - 1) * 4
        local t0, t1, t2, t3 = w[p + 1], w[p + 2], w[p + 3], w[p + 4]
        if i % nk == 0 then
            t0, t1, t2, t3 = bxor(SBOX[t1], rcon), SBOX[t2], SBOX[t3], SBOX[t0]
            rcon = M2[rcon]
        elseif nk > 6 and i % nk == 4 then
            t0, t1, t2, t3 = SBOX[t0], SBOX[t1], SBOX[t2], SBOX[t3]
        end
        local b = (i - nk) * 4
        w[i * 4 + 1], w[i * 4 + 2] = bxor(w[b + 1], t0), bxor(w[b + 2], t1)
        w[i * 4 + 3], w[i * 4 + 4] = bxor(w[b + 3], t2), bxor(w[b + 4], t3)
    end
    return w, nr
end

local function cifrar_bloco(s, w, nr)
    for i = 1, 16 do
        s[i] = bxor(s[i], w[i])
    end
    local t = {}
    for r = 1, nr do
        for c = 0, 3 do
            for l = 0, 3 do
                t[l + 4 * c + 1] = SBOX[s[l + 4 * ((c + l) % 4) + 1]]
            end
        end
        if r < nr then
            for c = 1, 13, 4 do
                local a0, a1, a2, a3 = t[c], t[c + 1], t[c + 2], t[c + 3]
                t[c] = bxor(M2[a0], M3[a1], a2, a3)
                t[c + 1] = bxor(a0, M2[a1], M3[a2], a3)
                t[c + 2] = bxor(a0, a1, M2[a2], M3[a3])
                t[c + 3] = bxor(M3[a0], a1, a2, M2[a3])
            end
        end
        for i = 1, 16 do
            s[i] = bxor(t[i], w[r * 16 + i])
        end
    end
    return s
end

local function decifrar_bloco(s, w, nr)
    for i = 1, 16 do
        s[i] = bxor(s[i], w[nr * 16 + i])
    end
    local t = {}
    for r = nr - 1, 0, -1 do
        for c = 0, 3 do
            for l = 0, 3 do
                t[l + 4 * c + 1] = INV[s[l + 4 * ((c - l) % 4) + 1]]
            end
        end
        for i = 1, 16 do
            t[i] = bxor(t[i], w[r * 16 + i])
        end
        if r > 0 then
            for c = 1, 13, 4 do
                local a0, a1, a2, a3 = t[c], t[c + 1], t[c + 2], t[c + 3]
                t[c] = bxor(M14[a0], M11[a1], M13[a2], M9[a3])
                t[c + 1] = bxor(M9[a0], M14[a1], M11[a2], M13[a3])
                t[c + 2] = bxor(M13[a0], M9[a1], M14[a2], M11[a3])
                t[c + 3] = bxor(M11[a0], M13[a1], M9[a2], M14[a3])
            end
        end
        table.move(t, 1, 16, 1, s)
    end
    return s
end

local function eh_base64(s)
    return #s % 4 == 0 and s:match("^[A-Za-z0-9+/]*=?=?$") ~= nil
end

-- Chave e IV chegam em base64 (o que generatekey/encrypt devolvem); texto cru
-- do tamanho certo também passa, como em outros executores.
-- O IV aceita mais de 16 bytes e usa os 16 primeiros: o teste da UNC passa
-- generatekey() (32 bytes) como IV.
local function bytes_de(valor, aceita, n, nome, oque)
    checar(valor, "string", n, nome)
    local cru = eh_base64(valor) and base64_decode(valor) or valor
    if not aceita(#cru) then
        cru = valor
    end
    if not aceita(#cru) then
        error(string.format("invalid argument #%d to '%s' (invalid %s size)", n, nome, oque), 3)
    end
    return { cru:byte(1, math.min(#cru, 32)) }
end

local MODOS = { CBC = true, ECB = true, CTR = true, CFB = true, OFB = true }
local function TAM_CHAVE(n)
    return n == 16 or n == 24 or n == 32
end
local function TAM_IV(n)
    return n >= 16
end

local function aes(cifrar, dados, chave, iv, modo, nome)
    modo = string.upper(modo == nil and "CBC" or checar(modo, "string", 4, nome))
    if not MODOS[modo] then
        error("invalid argument #4 to '" .. nome .. "' (unsupported mode '" .. modo .. "')", 3)
    end
    carregar_aes()
    local w, nr = expandir(bytes_de(chave, TAM_CHAVE, 2, nome, "key"))
    local vetor = table.move(bytes_de(iv, TAM_IV, 3, nome, "iv"), 1, 16, 1, {})
    local entrada = { dados:byte(1, -1) }
    local blocos = modo == "CBC" or modo == "ECB"
    if blocos and cifrar then
        local pad = 16 - #entrada % 16
        for _ = 1, pad do
            entrada[#entrada + 1] = pad
        end
    elseif blocos and #entrada % 16 ~= 0 then
        error("invalid argument #1 to '" .. nome .. "' (data is not a multiple of 16 bytes)", 3)
    end
    local saida = {}
    local anterior = table.clone(vetor)
    for i = 1, #entrada, 16 do
        local bloco = table.move(entrada, i, i + 15, 1, {})
        local n = math.min(16, #entrada - i + 1)
        if modo == "ECB" then
            bloco = cifrar and cifrar_bloco(bloco, w, nr) or decifrar_bloco(bloco, w, nr)
        elseif modo == "CBC" then
            if cifrar then
                for k = 1, 16 do
                    bloco[k] = bxor(bloco[k], anterior[k])
                end
                bloco = cifrar_bloco(bloco, w, nr)
                anterior = table.clone(bloco)
            else
                local cifrado = table.clone(bloco)
                decifrar_bloco(bloco, w, nr)
                for k = 1, 16 do
                    bloco[k] = bxor(bloco[k], anterior[k])
                end
                anterior = cifrado
            end
        else
            -- modos de fluxo: só a cifra direta do bloco, XOR com os dados
            local fluxo = cifrar_bloco(table.clone(anterior), w, nr)
            for k = 1, n do
                bloco[k] = bxor(bloco[k], fluxo[k])
            end
            if modo == "OFB" then
                anterior = fluxo
            elseif modo == "CFB" then
                anterior = cifrar and table.move(bloco, 1, 16, 1, {}) or table.move(entrada, i, i + 15, 1, {})
            else
                for k = 16, 1, -1 do
                    anterior[k] = (anterior[k] + 1) % 256
                    if anterior[k] ~= 0 then
                        break
                    end
                end
            end
        end
        table.move(bloco, 1, n, #saida + 1, saida)
    end
    if blocos and not cifrar then
        local pad = saida[#saida]
        if not pad or pad < 1 or pad > 16 then
            error("'" .. nome .. "' failed: bad padding (wrong key, iv or mode?)", 3)
        end
        for _ = 1, pad do
            saida[#saida] = nil
        end
    end
    local partes = {}
    for k = 1, #saida, 4096 do
        partes[#partes + 1] = string.char(table.unpack(saida, k, math.min(k + 4095, #saida)))
    end
    return table.concat(partes), string.char(table.unpack(vetor))
end

local function encrypt(dados, chave, iv, modo)
    checar(dados, "string", 1, "crypt.encrypt")
    if iv == nil then
        iv = base64_encode(bytes_aleatorios(16))
    end
    local cifrado, vetor = aes(true, dados, chave, iv, modo, "crypt.encrypt")
    return base64_encode(cifrado), base64_encode(vetor)
end

local function decrypt(dados, chave, iv, modo)
    checar(dados, "string", 1, "crypt.decrypt")
    if iv == nil then
        error("invalid argument #3 to 'crypt.decrypt' (string expected, got nil)", 2)
    end
    local cru = eh_base64(dados) and base64_decode(dados) or dados
    return (aes(false, cru, chave, iv, modo, "crypt.decrypt"))
end

local base64 = { encode = marcar(base64_encode), decode = marcar(base64_decode) }

publicar("crypt", {
    base64encode = base64.encode,
    base64decode = base64.decode,
    base64_encode = base64.encode,
    base64_decode = base64.decode,
    base64 = base64,
    hash = marcar(hash),
    encrypt = marcar(encrypt),
    decrypt = marcar(decrypt),
    generatebytes = marcar(function(n)
        checar(n, "number", 1, "crypt.generatebytes")
        return base64_encode(bytes_aleatorios(math.max(0, math.floor(n))))
    end),
    generatekey = marcar(function()
        return base64_encode(bytes_aleatorios(32))
    end),
})
publicar("base64", base64)
publicar("base64_encode", base64.encode)
publicar("base64_decode", base64.decode)

-- LZ4 (formato de bloco) --------------------------------------------------------

-- Compressor guloso: tabela das sequências de 4 bytes já vistas. Segue as regras
-- do formato: os últimos 5 bytes são literais e o último match começa pelo
-- menos 12 bytes antes do fim.
local function lz4_comprimir(s)
    checar(s, "string", 1, "lz4compress")
    local n = #s
    local out = {}
    local function tamanho_extra(v)
        while v >= 255 do
            out[#out + 1] = "\255"
            v -= 255
        end
        out[#out + 1] = string.char(v)
    end
    local function sequencia(literais, desloc, comp)
        local lit = #literais
        local m = comp and comp - 4 or 0
        out[#out + 1] = string.char(math.min(lit, 15) * 16 + math.min(m, 15))
        if lit >= 15 then
            tamanho_extra(lit - 15)
        end
        out[#out + 1] = literais
        if comp then
            out[#out + 1] = string.char(desloc % 256, math.floor(desloc / 256))
            if m >= 15 then
                tamanho_extra(m - 15)
            end
        end
    end
    local vistos = {}
    local ancora, i = 1, 1
    while i <= n - 11 do
        local chave = string.sub(s, i, i + 3)
        local cand = vistos[chave]
        vistos[chave] = i
        if cand and i - cand <= 65535 then
            local comp = 4
            while i + comp <= n - 5 and string.byte(s, cand + comp) == string.byte(s, i + comp) do
                comp += 1
            end
            sequencia(string.sub(s, ancora, i - 1), i - cand, comp)
            i += comp
            ancora = i
        else
            i += 1
        end
    end
    sequencia(string.sub(s, ancora, n))
    return table.concat(out)
end

local function lz4_descomprimir(s, tamanho)
    checar(s, "string", 1, "lz4decompress")
    local n, i, t = #s, 1, 0
    local out = {}
    local function invalido()
        error("lz4decompress: invalid or corrupted data", 3)
    end
    local function ler_extra(v)
        local b
        repeat
            b = string.byte(s, i)
            if not b then
                invalido()
            end
            i += 1
            v += b
        until b ~= 255
        return v
    end
    while i <= n do
        local token = string.byte(s, i)
        i += 1
        local lit = rsh(token, 4)
        if lit == 15 then
            lit = ler_extra(lit)
        end
        if i + lit - 1 > n then
            invalido()
        end
        for k = 0, lit - 1 do
            out[t + k + 1] = string.byte(s, i + k)
        end
        t += lit
        i += lit
        if i > n then
            break
        end
        if i + 1 > n then
            invalido()
        end
        local desloc = string.byte(s, i) + string.byte(s, i + 1) * 256
        i += 2
        if desloc == 0 or desloc > t then
            invalido()
        end
        local comp = band(token, 15)
        if comp == 15 then
            comp = ler_extra(comp)
        end
        -- cópia byte a byte: o match pode sobrepor o que está sendo escrito
        local origem = t - desloc
        for k = 1, comp + 4 do
            out[t + k] = out[origem + k]
        end
        t += comp + 4
    end
    if type(tamanho) == "number" and tamanho >= 0 and tamanho < t then
        t = math.floor(tamanho)
    end
    local partes = {}
    for k = 1, t, 4096 do
        partes[#partes + 1] = string.char(table.unpack(out, k, math.min(k + 4095, t)))
    end
    return table.concat(partes)
end

publicar("lz4compress", marcar(lz4_comprimir))
publicar("lz4decompress", marcar(lz4_descomprimir))

-- Arquivos ------------------------------------------------------------------

-- Sem `io` no jogo: o espelho abaixo nasce do instantâneo que o host prefixa ao
-- chunk (`__luaria_arquivos`) e cada escrita sai por `print("LUARIA_FS ...")`, que
-- a UI aplica na pasta ~/.config/luaria/Workspace/. Só vale com a UI aberta.
do
    local inicial = __luaria_arquivos
    local arqs = {}
    local pastas = {}
    local MAX_NOME = 200
    local MAX_ESCRITA = 4 * 1024 * 1024
    local TAM_PARTE = 3000
    local contador = 0
    -- token da sessão do host: linha LUARIA_* sem ele (print de outro script do
    -- jogo) é ignorada pela UI
    local token = type(inicial.token) == "string" and inicial.token or ""

    for caminho, b64 in pairs(inicial.arquivos or {}) do
        arqs[caminho] = { b64 = b64 }
    end
    for caminho in pairs(inicial.pastas or {}) do
        pastas[caminho] = true
    end

    -- mesma regra do host (arquivos.rs::resolver)
    local function validar(caminho)
        if type(caminho) ~= "string" or caminho == "" or #caminho > MAX_NOME
            or caminho:find("\\", 1, true) or caminho:find("\0", 1, true) then
            error("invalid path", 3)
        end
        for parte in (caminho .. "/"):gmatch("([^/]*)/") do
            if parte == "" or parte == "." or parte == ".." then
                error("invalid path", 3)
            end
        end
        return caminho
    end

    local function pai(caminho)
        return caminho:match("^(.*)/[^/]*$")
    end

    local function garantir_pastas(caminho)
        local p = pai(caminho)
        while p do
            pastas[p] = true
            p = pai(p)
        end
    end

    local function emitir(op, caminho, dados)
        contador += 1
        local id = string.format("%s%x%x%x", token, os.time(), rng:NextInteger(0, 0xFFFFFF), contador)
        local c64 = base64_encode(caminho)
        local d64 = base64_encode(dados)
        local n = math.max(1, math.ceil(#d64 / TAM_PARTE))
        for i = 1, n do
            local parte = d64:sub((i - 1) * TAM_PARTE + 1, i * TAM_PARTE)
            print("LUARIA_FS " .. op .. " " .. id .. " " .. i .. "/" .. n .. " " .. c64 .. " " .. parte)
        end
    end

    local function ler(caminho)
        local e = arqs[caminho]
        if e == nil then
            return nil
        end
        if e.dado == nil then
            e.dado = base64_decode(e.b64)
        end
        return e.dado
    end

    local function dados_validos(dados)
        if type(dados) ~= "string" then
            error("invalid argument #2 (string expected)", 3)
        end
        if #dados > MAX_ESCRITA then
            error("data too large (max 4 MB)", 3)
        end
        return dados
    end

    publicar("writefile", marcar(function(caminho, dados)
        validar(caminho)
        dados_validos(dados)
        arqs[caminho] = { dado = dados }
        garantir_pastas(caminho)
        emitir("w", caminho, dados)
    end))

    publicar("appendfile", marcar(function(caminho, dados)
        validar(caminho)
        dados_validos(dados)
        local atual = ler(caminho) or ""
        arqs[caminho] = { dado = atual .. dados }
        garantir_pastas(caminho)
        emitir("a", caminho, dados)
    end))

    publicar("readfile", marcar(function(caminho)
        validar(caminho)
        local dado = ler(caminho)
        if dado == nil then
            error(caminho .. " does not exist", 2)
        end
        return dado
    end))

    publicar("isfile", marcar(function(caminho)
        validar(caminho)
        return arqs[caminho] ~= nil
    end))

    publicar("isfolder", marcar(function(caminho)
        validar(caminho)
        return pastas[caminho] == true
    end))

    publicar("makefolder", marcar(function(caminho)
        validar(caminho)
        pastas[caminho] = true
        garantir_pastas(caminho)
        emitir("m", caminho, "")
    end))

    publicar("delfile", marcar(function(caminho)
        validar(caminho)
        arqs[caminho] = nil
        emitir("d", caminho, "")
    end))

    publicar("delfolder", marcar(function(caminho)
        validar(caminho)
        local prefixo = caminho .. "/"
        for k in pairs(pastas) do
            if k == caminho or k:sub(1, #prefixo) == prefixo then
                pastas[k] = nil
            end
        end
        for k in pairs(arqs) do
            if k:sub(1, #prefixo) == prefixo then
                arqs[k] = nil
            end
        end
        emitir("D", caminho, "")
    end))

    publicar("listfiles", marcar(function(caminho)
        local raiz = caminho == "" or caminho == "."
        if not raiz then
            validar(caminho)
            if not pastas[caminho] then
                error(caminho .. " does not exist", 2)
            end
        end
        local prefixo = raiz and "" or (caminho .. "/")
        local achados = {}
        local function filho(k)
            return k:sub(1, #prefixo) == prefixo and k ~= caminho and not k:sub(#prefixo + 1):find("/", 1, true)
        end
        for k in pairs(arqs) do
            if filho(k) then
                achados[#achados + 1] = k
            end
        end
        for k in pairs(pastas) do
            if filho(k) then
                achados[#achados + 1] = k
            end
        end
        table.sort(achados)
        return achados
    end))

    publicar("loadfile", marcar(function(caminho, nome)
        validar(caminho)
        local dado = ler(caminho)
        if dado == nil then
            return nil, caminho .. ": file does not exist"
        end
        return env.loadstring(dado, nome or ("=" .. caminho))
    end))

    publicar("dofile", marcar(function(caminho)
        local f, e = env.loadfile(caminho)
        if not f then
            error(e, 2)
        end
        return f()
    end))

    local MAX_CLIP = 192 * 1024

    -- só ida: a UI joga o texto no clipboard do sistema (wl-copy)
    publicar("setclipboard", marcar(function(texto)
        if type(texto) == "number" then
            texto = tostring(texto)
        end
        checar(texto, "string", 1, "setclipboard")
        if #texto > MAX_CLIP then
            error("invalid argument #1 to 'setclipboard' (text too large, max 192 KB)", 2)
        end
        print("LUARIA_CLIP " .. token .. " " .. base64_encode(texto))
    end))
    publicar("toclipboard", env.setclipboard)
end

-- request via canal do host (ponte_codec.rs): o host faz o curl e devolve status + corpo
-- codificado. Sem os dois nativos da ponte, request nem existe (nada fingido).
do
    local METODOS = { GET = true, POST = true, PUT = true, DELETE = true, PATCH = true, HEAD = true, OPTIONS = true }
    local TIPO_HTTP = 1
    local MAX_HEADERS = 64
    local PRAZO_TOTAL = 30
    -- shortcut: o pulso só anda no início de cada passada e ponte.rs esperar_voo segura a passada
    -- até os pedidos em voo acabarem; pedido HTTP > 1.5 s dá falso "host não atende". Pulsar dentro do esperar_voo.
    local PRAZO_PULSO = 1.5
    local PRAZO_SEM_CORO = 3
    local ERROS_PE = {
        [-1] = "channel full",
        [-2] = "request too large",
        [-3] = "bridge unavailable",
        [-4] = "invalid argument",
    }

    local function codificar(campos)
        local partes = {}
        for i, c in ipairs(campos) do
            partes[i] = #c .. ":" .. c
        end
        return table.concat(partes)
    end

    -- Mesmo contrato do Rust: len decimal sem sinal nem zeros à esquerda, até 10 dígitos.
    local function decodificar(s, max)
        local campos, i, fim_buf = {}, 1, #s
        while i <= fim_buf do
            if #campos == max then
                return nil, "campos demais"
            end
            local ini = i
            while i <= fim_buf and s:byte(i) >= 48 and s:byte(i) <= 57 do
                i += 1
            end
            if i > fim_buf then
                return nil, "truncated or missing field"
            end
            local digitos = s:sub(ini, i - 1)
            if s:byte(i) ~= 58 or digitos == "" or #digitos > 10 or (#digitos > 1 and digitos:sub(1, 1) == "0") then
                return nil, "invalid field size"
            end
            i += 1
            local fim = i + tonumber(digitos) - 1
            if fim > fim_buf then
                return nil, "truncated or missing field"
            end
            campos[#campos + 1] = s:sub(i, fim)
            i = fim + 1
        end
        return campos
    end

    -- Envia e espera a resposta. O pe roda dentro do jogo e não pode bloquear, então o pr
    -- é chamado em polling; sem coroutina (não dá pra ceder) a espera é ocupada e mais curta.
    local function pedir(tipo, payload)
        local h = n_pe(tipo, payload)
        if h < 0 then
            error("request failed: " .. (ERROS_PE[h] or "code " .. h), 3)
        end
        local sem_coro = not coroutine.isyieldable()
        local limite = sem_coro and PRAZO_SEM_CORO or PRAZO_TOTAL
        local inicio = os.clock()
        local pulso, visto = nil, inicio
        while true do
            local est, a, b = n_pr(h)
            if est == 1 then
                return a, b
            end
            if est == 2 then
                error("request failed: request lost/cancelled", 3)
            end
            local agora = os.clock()
            if a ~= pulso then
                pulso, visto = a, agora
            end
            if agora - visto > PRAZO_PULSO then
                n_pr(h, true)
                error("request failed: host not responding (open the UI)", 3)
            end
            if agora - inicio > limite then
                n_pr(h, true)
                error("request failed: timed out", 3)
            end
            if not sem_coro then
                task.wait()
            end
        end
    end

    local function invalido(detalhe)
        error("invalid argument #1 to 'request' (" .. detalhe .. ")", 3)
    end

    local function request(opts)
        checar(opts, "table", 1, "request")
        local url = opts.Url
        if type(url) ~= "string" or not url:lower():match("^https?://") then
            invalido("Url must be an http or https string")
        end
        local metodo = opts.Method or "GET"
        if type(metodo) ~= "string" or not METODOS[metodo:upper()] then
            invalido("Method must be one of GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS")
        end
        local cabecalhos = opts.Headers or {}
        if type(cabecalhos) ~= "table" then
            invalido("Headers must be a table of strings")
        end
        local campos = { metodo:upper(), url, "" }
        local n = 0
        for nome, valor in pairs(cabecalhos) do
            if type(nome) ~= "string" or type(valor) ~= "string" then
                invalido("Headers must be a table of strings")
            end
            n += 1
            if n > MAX_HEADERS then
                invalido("too many headers (max 64)")
            end
            campos[#campos + 1] = nome
            campos[#campos + 1] = valor
        end
        campos[3] = tostring(n)
        local corpo = opts.Body or ""
        if type(corpo) ~= "string" then
            invalido("Body must be a string")
        end
        campos[#campos + 1] = corpo

        local status, resposta = pedir(TIPO_HTTP, codificar(campos))
        -- status 0 é erro local do host (texto puro no corpo); fora de 100..599 é código de falha da ponte
        if status == 0 then
            error("request failed: " .. resposta, 2)
        end
        if status < 100 or status > 599 then
            error("request failed: code " .. status, 2)
        end

        -- mensagem, n, (nome, valor)*, corpo
        local partes, motivo = decodificar(resposta, 3 + 2 * MAX_HEADERS)
        local n_resp = partes and tonumber(partes[2])
        if not partes or not n_resp or #partes ~= 3 + 2 * n_resp then
            error("request failed: invalid response" .. (motivo and (" (" .. motivo .. ")") or ""), 2)
        end
        local headers = {}
        for i = 3, #partes - 1, 2 do
            headers[partes[i]] = partes[i + 1]
        end
        return {
            Success = status >= 200 and status < 300,
            StatusCode = status,
            StatusMessage = partes[1],
            Headers = headers,
            Body = partes[#partes],
        }
    end

    if type(n_pe) == "function" and type(n_pr) == "function" then
        publicar("request", marcar(request))
        publicar("http_request", env.request)
        publicar("http", { request = env.request })
    end
end
