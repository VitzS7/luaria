-- loadstring dinâmico: interpretador de Luau escrito em Luau.
--
-- O cliente não tem compilador, então fonte só vira função aqui,
-- em tempo de execução: lexer -> parser (resolve escopo e upvalues) -> closures.
-- Cada nó do AST vira uma closure já resolvida (variável local = slot fixo no
-- frame), sem tree-walk com `if tipo == ...` a cada execução.
--
-- Roda dentro do chunk do prelude: `env`, `publicar` e `marcar` vêm de lá.
-- Sem pcall (ver prelude.lua): erro de sintaxe é capturado com coroutine.

local ponte_globais
publicar("loadstring", marcar((function()
local unpack, pack, move, tcreate = table.unpack, table.pack, table.move, table.create
local sbyte, ssub, sfind, smatch, schar, sgsub = string.byte, string.sub, string.find, string.match, string.char, string.gsub

-- Ambiente do código interpretado ------------------------------------------

-- getfenv é bloqueado, então a lista de globais padrão é literal. Nome que não
-- existir neste cliente vira nil, sem erro.
local base = setmetatable({
    game = game, workspace = workspace, script = script, shared = shared, _G = _G, _VERSION = _VERSION,
    Instance = Instance, Vector2 = Vector2, Vector3 = Vector3, Vector2int16 = Vector2int16,
    Vector3int16 = Vector3int16, CFrame = CFrame, Color3 = Color3, UDim = UDim, UDim2 = UDim2,
    Rect = Rect, Enum = Enum, Ray = Ray, Region3 = Region3, Region3int16 = Region3int16,
    TweenInfo = TweenInfo, NumberRange = NumberRange, NumberSequence = NumberSequence,
    NumberSequenceKeypoint = NumberSequenceKeypoint, ColorSequence = ColorSequence,
    ColorSequenceKeypoint = ColorSequenceKeypoint, BrickColor = BrickColor, Random = Random,
    Axes = Axes, Faces = Faces, PhysicalProperties = PhysicalProperties, Font = Font,
    DateTime = DateTime, OverlapParams = OverlapParams, RaycastParams = RaycastParams,
    PathWaypoint = PathWaypoint, SharedTable = SharedTable, CatalogSearchParams = CatalogSearchParams,
    FloatCurveKey = FloatCurveKey, RotationCurveKey = RotationCurveKey,
    task = task, string = string, table = table, math = math, os = os, utf8 = utf8, bit32 = bit32,
    coroutine = coroutine, debug = debug, buffer = buffer,
    typeof = typeof, type = type, tostring = tostring, tonumber = tonumber,
    pairs = pairs, ipairs = ipairs, next = next, select = select, unpack = unpack,
    rawget = rawget, rawset = rawset, rawequal = rawequal, rawlen = rawlen,
    setmetatable = setmetatable, getmetatable = getmetatable, assert = assert,
    print = print, warn = warn, pcall = pcall, xpcall = xpcall, require = require,
    tick = tick, time = time, wait = wait, delay = delay, spawn = spawn, elapsedTime = elapsedTime,
    collectgarbage = collectgarbage, newproxy = newproxy, gcinfo = gcinfo, settings = settings,
    UserSettings = UserSettings, stats = stats, version = version, plugin = plugin,
    PluginManager = PluginManager, printidentity = printidentity,
}, { __index = env })
-- Leitor/gravador de globais de verdade, gerados pelo host para os nomes que o
-- script usa (prelude.rs::ponte_globais). Sem eles getgenv() seria uma tabela
-- solta: `getgenv().X = 1` não viraria o global `X` do script.
local ler_global = function() return nil end
local gravar_global = function() return false end
ponte_globais = function(ler, gravar)
    ler_global, gravar_global = ler, gravar
end
local genv = setmetatable({}, {
    __index = function(_, k)
        local v = base[k]
        if v == nil then
            v = ler_global(k)
        end
        return v
    end,
    __newindex = function(t, k, v)
        if not gravar_global(k, v) then
            rawset(t, k, v)
        end
    end,
})

-- Posição da última chamada executada: o error() do ambiente interpretado
-- precisa dela para prefixar "nome:linha:" como o error() nativo faria.
local pos = ""
local erroNativo = error
base.error = function(msg, nivel)
    if type(msg) == "string" and nivel ~= 0 then
        msg = pos .. " " .. msg
    end
    return erroNativo(msg, 0)
end

-- assert nativo prefixa a posição de quem o chamou, que aqui seria o interpretador
base.assert = function(v, msg, ...)
    if v then return v, msg, ... end
    erroNativo(pos .. " " .. (type(msg) == "string" and msg or "assertion failed!"), 0)
end

publicar("getgenv", marcar(function()
    return genv
end))

-- Léxico ---------------------------------------------------------------------

local CN = "?"

local function falha(linha, msg)
    erroNativo(CN .. ":" .. linha .. ": " .. msg, 0)
end

local KW = {}
for w in ("and break do else elseif end false for function if in local nil not or repeat return then true until while"):gmatch("%a+") do
    KW[w] = true
end
local P3 = { ["..."] = true, ["..="] = true, ["//="] = true }
local P2 = {}
for w in ("== ~= <= >= .. :: -> += -= *= /= %= ^= //"):gmatch("%S+") do
    P2[w] = true
end
local P1 = {}
for c in ("+-*/%^#&<>=(){}[];:,.@|?"):gmatch(".") do
    P1[c] = true
end

local ESC = { n = "\n", t = "\t", r = "\r", a = "\a", b = "\b", f = "\f", v = "\v",
    ["\\"] = "\\", ['"'] = '"', ["'"] = "'", ["`"] = "`", ["{"] = "{" }

-- src[a] == "\\"; devolve texto, próximo índice e linha
local function escapar(src, a, linha)
    local c = ssub(src, a + 1, a + 1)
    local r = ESC[c]
    if r then return r, a + 2, linha end
    if c == "\n" then return "\n", a + 2, linha + 1 end
    if c == "\r" then
        return "\n", a + (ssub(src, a + 2, a + 2) == "\n" and 3 or 2), linha + 1
    end
    if c == "z" then
        local _, e = sfind(src, "^%s*", a + 2)
        local _, k = sgsub(ssub(src, a + 2, e), "\n", "")
        return "", e + 1, linha + k
    end
    if c == "x" then
        local h = smatch(src, "^%x%x", a + 2)
        if not h then falha(linha, "Malformed escape sequence '\\x'") end
        return schar(tonumber(h, 16)), a + 4, linha
    end
    if c == "u" then
        local h = smatch(src, "^{(%x+)}", a + 2)
        if not h then falha(linha, "Malformed escape sequence '\\u'") end
        return utf8.char(tonumber(h, 16)), a + 4 + #h, linha
    end
    local d = smatch(src, "^%d%d?%d?", a + 1)
    if d then
        local v = tonumber(d)
        if v > 255 then falha(linha, "Invalid escape sequence '\\" .. d .. "'") end
        return schar(v), a + 1 + #d, linha
    end
    falha(linha, "Invalid escape sequence '\\" .. c .. "'")
end

local function lerString(src, i, linha)
    local padrao = "[\\\n" .. ssub(src, i, i) .. "]"
    local buf, nb, ini = {}, 0, i + 1
    while true do
        local a = sfind(src, padrao, ini)
        if not a then falha(linha, "Malformed string; did you forget to finish it?") end
        if a > ini then nb += 1; buf[nb] = ssub(src, ini, a - 1) end
        local c = sbyte(src, a)
        if c == 10 then falha(linha, "Malformed string; did you forget to finish it?") end
        if c ~= 92 then return table.concat(buf), a + 1, linha end
        local r
        r, ini, linha = escapar(src, a, linha)
        nb += 1; buf[nb] = r
    end
end

local lex

-- partes: textos puros e {tipos, valores, linhas} para cada `{expr}`
local function lerInterp(src, i, linha)
    local partes, np, buf, nb, ini = {}, 0, {}, 0, i + 1
    while true do
        local a = sfind(src, "[\\{`\n]", ini)
        if not a then falha(linha, "Malformed interpolated string; did you forget to add a '`'?") end
        if a > ini then nb += 1; buf[nb] = ssub(src, ini, a - 1) end
        local c = sbyte(src, a)
        if c == 10 then
            nb += 1; buf[nb] = "\n"; linha += 1; ini = a + 1
        elseif c == 92 then
            local r
            r, ini, linha = escapar(src, a, linha)
            nb += 1; buf[nb] = r
        else
            if nb > 0 then np += 1; partes[np] = table.concat(buf); buf, nb = {}, 0 end
            if c == 96 then return partes, a + 1, linha end
            local tt, tv, tl
            tt, tv, tl, ini, linha = lex(src, a + 1, linha, true)
            np += 1; partes[np] = { tt, tv, tl }
        end
    end
end

-- interp: para no `}` que fecha a expressão de uma string interpolada
lex = function(src, i, linha, interp)
    local tt, tv, tl, n, prof = {}, {}, {}, 0, 0
    local tam = #src
    while i <= tam do
        local c = sbyte(src, i)
        if c == 10 then
            linha += 1; i += 1
        elseif c == 32 or c == 9 or c == 13 then
            i += 1
        elseif c == 45 and sbyte(src, i + 1) == 45 then
            local eq = smatch(src, "^%-%-%[(=*)%[", i)
            if eq then
                local a, b = sfind(src, "]" .. eq .. "]", i, true)
                if not a then falha(linha, "Unfinished long comment") end
                local _, k = sgsub(ssub(src, i, b), "\n", "")
                linha += k; i = b + 1
            else
                i = sfind(src, "\n", i, true) or tam + 1
            end
        else
            local tipo, valor, prox = nil, nil, i
            if (c >= 97 and c <= 122) or (c >= 65 and c <= 90) or c == 95 then
                local _, e = sfind(src, "^[%w_]+", i)
                local w = ssub(src, i, e)
                prox = e + 1
                if KW[w] then tipo = w else tipo, valor = "name", w end
            elseif (c >= 48 and c <= 57) or (c == 46 and smatch(src, "^%.%d", i)) then
                local txt
                if smatch(src, "^0[xX]", i) then
                    txt = smatch(src, "^0[xX][%x_]*", i)
                elseif smatch(src, "^0[bB]", i) then
                    txt = smatch(src, "^0[bB][01_]*", i)
                else
                    txt = smatch(src, "^%d[%d_]*%.?[%d_]*", i) or smatch(src, "^%.%d[%d_]*", i)
                    local ex = smatch(src, "^[eE][+-]?%d[%d_]*", i + #txt)
                    if ex then txt = txt .. ex end
                end
                prox = i + #txt
                if smatch(src, "^[%w_]", prox) then falha(linha, "Malformed number") end
                local limpo = sgsub(txt, "_", "")
                local v
                if smatch(limpo, "^0[bB]") then
                    local bits = ssub(limpo, 3)
                    v = bits == "" and 0 or tonumber(bits, 2)
                else
                    v = tonumber(limpo)
                end
                if not v then falha(linha, "Malformed number") end
                tipo, valor = "number", v
            elseif c == 34 or c == 39 then
                tipo = "string"
                valor, prox, linha = lerString(src, i, linha)
            elseif c == 96 then
                tipo = "interp"
                valor, prox, linha = lerInterp(src, i, linha)
            elseif c == 91 and smatch(src, "^%[=*%[", i) then
                local eq = smatch(src, "^%[(=*)%[", i)
                local ini = i + #eq + 2
                local a, b = sfind(src, "]" .. eq .. "]", ini, true)
                if not a then falha(linha, "Malformed string; did you forget to finish it?") end
                local txt = ssub(src, ini, a - 1)
                local _, k = sgsub(txt, "\n", "")
                if ssub(txt, 1, 1) == "\n" then txt = ssub(txt, 2)
                elseif ssub(txt, 1, 2) == "\r\n" then txt = ssub(txt, 3) end
                tipo, valor, prox = "string", txt, b + 1
                n += 1; tt[n] = tipo; tv[n] = valor; tl[n] = linha
                linha += k; i = prox
                tipo = nil
            else
                local s3, s2, s1 = ssub(src, i, i + 2), ssub(src, i, i + 1), ssub(src, i, i)
                if P3[s3] then tipo, prox = s3, i + 3
                elseif P2[s2] then tipo, prox = s2, i + 2
                elseif P1[s1] then
                    tipo, prox = s1, i + 1
                    if interp then
                        if s1 == "{" then prof += 1
                        elseif s1 == "}" then
                            if prof == 0 then
                                n += 1; tt[n] = "eof"; tv[n] = false; tl[n] = linha
                                return tt, tv, tl, i + 1, linha
                            end
                            prof -= 1
                        end
                    end
                else
                    falha(linha, "Unexpected symbol '" .. s1 .. "'")
                end
            end
            if tipo then
                n += 1; tt[n] = tipo; tv[n] = valor; tl[n] = linha
                i = prox
            end
        end
    end
    if interp then falha(linha, "Malformed interpolated string; expected '}'") end
    n += 1; tt[n] = "eof"; tv[n] = false; tl[n] = linha
    return tt, tv, tl, i, linha
end

-- Parser ---------------------------------------------------------------------

local T, V, L, P
local fs

local function descTok()
    local t = T[P]
    if t == "eof" then return "<eof>" end
    if t == "name" or t == "number" then return tostring(V[P]) end
    if t == "string" then return "string" end
    return t
end

local function erro(msg)
    falha(L[P], msg)
end

local function esperar(t, ctx)
    if T[P] ~= t then erro("Expected '" .. t .. "'" .. (ctx or "") .. ", got '" .. descTok() .. "'") end
    P += 1
end

local function aceitar(t)
    if T[P] == t then P += 1; return true end
    return false
end

local function esperarNome()
    if T[P] ~= "name" then erro("Expected identifier, got '" .. descTok() .. "'") end
    P += 1
    return V[P - 1]
end

local function novaFuncao()
    return { parent = fs, act = {}, vars = {}, ups = {}, upidx = {} }
end

local function declarar(nome)
    local v = { name = nome, fs = fs, captured = false }
    fs.vars[#fs.vars + 1] = v
    fs.act[#fs.act + 1] = v
    return v
end

local function fecharEscopo(m)
    local a = fs.act
    for i = #a, m + 1, -1 do a[i] = nil end
end

local function registrarUp(g, v)
    if g.upidx[v] then return end
    if g.parent ~= v.fs then registrarUp(g.parent, v) end
    g.ups[#g.ups + 1] = v
    g.upidx[v] = #g.ups
end

local function resolver(nome)
    local v
    local g = fs
    while g and not v do
        local a = g.act
        for i = #a, 1, -1 do
            if a[i].name == nome then v = a[i]; break end
        end
        g = g.parent
    end
    if not v then return { t = "global", name = nome } end
    if v.fs == fs then return { t = "local", var = v } end
    v.captured = true
    registrarUp(fs, v)
    return { t = "upv", var = v, idx = fs.upidx[v] }
end

local expr, bloco, skipTipo

-- Tipos: só parseia e descarta.
local function skipListaTipos(fecha)
    while T[P] ~= fecha do
        if T[P] == "..." then P += 1 end
        if T[P] == "name" and T[P + 1] == ":" then P += 2 end
        skipTipo()
        if T[P] == "..." then P += 1 end
        if T[P] == "=" then P += 1; skipTipo() end
        if not aceitar(",") then break end
    end
    esperar(fecha)
end

local function skipTipoSimples()
    local t = T[P]
    if t == "(" then
        P += 1
        skipListaTipos(")")
        if aceitar("->") then skipTipo() end
    elseif t == "<" then
        P += 1
        skipListaTipos(">")
        skipTipoSimples()
    elseif t == "{" then
        P += 1
        while T[P] ~= "}" do
            if (V[P] == "read" or V[P] == "write") and T[P] == "name" and (T[P + 1] == "[" or (T[P + 1] == "name" and T[P + 2] == ":")) then
                P += 1
            end
            if T[P] == "[" then
                P += 1; skipTipo(); esperar("]"); esperar(":"); skipTipo()
            elseif T[P] == "name" and T[P + 1] == ":" then
                P += 2; skipTipo()
            else
                skipTipo()
            end
            if not (aceitar(",") or aceitar(";")) then break end
        end
        esperar("}")
    elseif t == "name" then
        if V[P] == "typeof" and T[P + 1] == "(" then
            P += 2; expr(); esperar(")")
        else
            P += 1
            if T[P] == "." then P += 2 end
            if T[P] == "<" then P += 1; skipListaTipos(">") end
        end
    elseif t == "string" or t == "nil" or t == "true" or t == "false" then
        P += 1
    else
        erro("Expected type, got '" .. descTok() .. "'")
    end
end

skipTipo = function()
    if T[P] == "|" or T[P] == "&" then P += 1 end
    if T[P] == "..." then P += 1 end
    skipTipoSimples()
    while true do
        local t = T[P]
        if t == "?" then P += 1
        elseif t == "|" or t == "&" then P += 1; skipTipoSimples()
        else break end
    end
end

local function skipGenericos()
    if T[P] == "<" then
        P += 1
        skipListaTipos(">")
    end
end

local function skipAtributos()
    while T[P] == "@" do
        P += 1
        if T[P] == "[" then
            local prof = 0
            repeat
                if T[P] == "[" then prof += 1 elseif T[P] == "]" then prof -= 1 end
                P += 1
            until prof == 0 or T[P] == "eof"
        else
            esperarNome()
        end
    end
end

local function constNo(v)
    return { t = "const", v = v }
end

local function corpoFuncao(metodo, linha)
    local nf = novaFuncao()
    fs = nf
    if metodo then declarar("self") end
    skipGenericos()
    esperar("(")
    local np = metodo and 1 or 0
    while T[P] ~= ")" do
        if T[P] == "..." then
            P += 1
            nf.vararg = true
            if aceitar(":") then skipTipo() end
            break
        end
        declarar(esperarNome())
        np += 1
        if aceitar(":") then skipTipo() end
        if not aceitar(",") then break end
    end
    esperar(")")
    if nf.vararg then nf.vavar = declarar("...") end
    if aceitar(":") then skipTipo() end
    local corpo = bloco()
    esperar("end", " (to close 'function' at line " .. linha .. ")")
    fs = nf.parent
    return { t = "function", fs = nf, np = np, body = corpo }
end

local function listaExpr()
    local l = { expr() }
    while aceitar(",") do l[#l + 1] = expr() end
    return l
end

local function tabela()
    local linha = L[P]
    esperar("{")
    local es = {}
    while T[P] ~= "}" do
        if T[P] == "[" then
            P += 1
            local k = expr()
            esperar("]"); esperar("=")
            es[#es + 1] = { k = k, v = expr() }
        elseif T[P] == "name" and T[P + 1] == "=" then
            local k = constNo(V[P])
            P += 2
            es[#es + 1] = { k = k, v = expr() }
        else
            es[#es + 1] = { v = expr() }
        end
        if not (aceitar(",") or aceitar(";")) then break end
    end
    esperar("}", " (to close '{' at line " .. linha .. ")")
    return { t = "table", entries = es }
end

local function argsChamada()
    local t = T[P]
    if t == "string" then
        P += 1
        return { constNo(V[P - 1]) }
    elseif t == "{" then
        return { tabela() }
    elseif t == "(" then
        local linha = L[P]
        P += 1
        local args = {}
        if T[P] ~= ")" then args = listaExpr() end
        esperar(")", " (to close '(' at line " .. linha .. ")")
        return args
    end
    erro("Expected '(', '{' or string, got '" .. descTok() .. "'")
end

local function interpolada(partes)
    local ps = {}
    for i = 1, #partes do
        local p = partes[i]
        if type(p) == "string" then
            ps[i] = p
        else
            local sT, sV, sL, sP = T, V, L, P
            T, V, L, P = p[1], p[2], p[3], 1
            ps[i] = expr()
            if T[P] ~= "eof" then erro("Expected '}' after interpolated expression, got '" .. descTok() .. "'") end
            T, V, L, P = sT, sV, sL, sP
        end
    end
    return { t = "interp", parts = ps }
end

local function primario()
    local t = T[P]
    local e
    if t == "name" then
        P += 1
        e = resolver(V[P - 1])
    elseif t == "(" then
        local linha = L[P]
        P += 1
        local interno = expr()
        esperar(")", " (to close '(' at line " .. linha .. ")")
        e = (interno.t == "call" or interno.t == "method" or interno.t == "vararg") and { t = "paren", e = interno } or interno
    else
        erro("Unexpected symbol '" .. descTok() .. "' when parsing expression")
    end
    while true do
        t = T[P]
        if t == "." then
            P += 1
            e = { t = "index", obj = e, key = constNo(esperarNome()) }
        elseif t == "[" then
            P += 1
            local k = expr()
            esperar("]")
            e = { t = "index", obj = e, key = k }
        elseif t == ":" then
            P += 1
            local nome = esperarNome()
            local linha = L[P]
            e = { t = "method", obj = e, name = nome, args = argsChamada(), line = linha }
        elseif t == "(" or t == "string" or t == "{" then
            local linha = L[P]
            e = { t = "call", fn = e, args = argsChamada(), line = linha }
        else
            return e
        end
    end
end

local function simples()
    local t = T[P]
    local e
    if t == "number" or t == "string" then
        P += 1
        e = constNo(V[P - 1])
    elseif t == "nil" then P += 1; e = constNo(nil)
    elseif t == "true" then P += 1; e = constNo(true)
    elseif t == "false" then P += 1; e = constNo(false)
    elseif t == "..." then
        if not fs.vavar then erro("Cannot use '...' outside of a vararg function") end
        P += 1
        e = { t = "vararg", var = fs.vavar }
    elseif t == "{" then
        e = tabela()
    elseif t == "interp" then
        P += 1
        e = interpolada(V[P - 1])
    elseif t == "@" then
        skipAtributos()
        local linha = L[P]
        esperar("function")
        e = corpoFuncao(false, linha)
    elseif t == "function" then
        local linha = L[P]
        P += 1
        e = corpoFuncao(false, linha)
    elseif t == "if" then
        P += 1
        local c = expr()
        esperar("then")
        local a = expr()
        local nos = { { c, a } }
        while aceitar("elseif") do
            local c2 = expr()
            esperar("then")
            nos[#nos + 1] = { c2, expr() }
        end
        esperar("else", " in if-else expression")
        e = expr()
        for i = #nos, 1, -1 do
            e = { t = "ifexp", c = nos[i][1], a = nos[i][2], b = e }
        end
    else
        e = primario()
    end
    while T[P] == "::" do
        P += 1
        skipTipo()
    end
    return e
end

local LP = { ["+"] = 6, ["-"] = 6, ["*"] = 7, ["/"] = 7, ["//"] = 7, ["%"] = 7, ["^"] = 10, [".."] = 5,
    ["=="] = 3, ["~="] = 3, ["<"] = 3, ["<="] = 3, [">"] = 3, [">="] = 3, ["and"] = 2, ["or"] = 1 }
local RP = { ["+"] = 6, ["-"] = 6, ["*"] = 7, ["/"] = 7, ["//"] = 7, ["%"] = 7, ["^"] = 9, [".."] = 4,
    ["=="] = 3, ["~="] = 3, ["<"] = 3, ["<="] = 3, [">"] = 3, [">="] = 3, ["and"] = 2, ["or"] = 1 }

local function subexpr(limite)
    local e
    local t = T[P]
    if t == "not" or t == "-" or t == "#" then
        P += 1
        local o = subexpr(8)
        if t == "-" and o.t == "const" and type(o.v) == "number" then
            e = constNo(-o.v)
        else
            e = { t = "un", op = t, e = o }
        end
    else
        e = simples()
    end
    while true do
        local op = T[P]
        local lp = LP[op]
        if not lp or lp <= limite then break end
        P += 1
        local d = subexpr(RP[op])
        if op == "and" or op == "or" then
            e = { t = op, a = e, b = d }
        else
            e = { t = "bin", op = op, a = e, b = d }
        end
    end
    return e
end

expr = function()
    return subexpr(0)
end

local function fimBloco(t)
    return t == "eof" or t == "end" or t == "else" or t == "elseif" or t == "until"
end

local function contexto(nome, linha)
    return " (to close '" .. nome .. "' at line " .. linha .. ")"
end

local function declaracaoTipo()
    esperarNome()
    skipGenericos()
    esperar("=")
    skipTipo()
end

local OPCOMP = { ["+="] = "+", ["-="] = "-", ["*="] = "*", ["/="] = "/", ["//="] = "//", ["%="] = "%", ["^="] = "^", ["..="] = ".." }

local function paraStat(linha)
    local mark = #fs.act
    local nome1 = esperarNome()
    if aceitar(":") then skipTipo() end
    local no
    if T[P] == "=" then
        P += 1
        local a = expr()
        esperar(",")
        local b = expr()
        local c = aceitar(",") and expr() or nil
        esperar("do")
        local v = declarar(nome1)
        local corpo = bloco()
        no = { t = "fornum", var = v, a = a, b = b, c = c, body = corpo }
    else
        local nomes = { nome1 }
        while aceitar(",") do
            nomes[#nomes + 1] = esperarNome()
            if aceitar(":") then skipTipo() end
        end
        esperar("in")
        local es = listaExpr()
        esperar("do")
        local vars = {}
        for i = 1, #nomes do vars[i] = declarar(nomes[i]) end
        local corpo = bloco()
        no = { t = "forin", vars = vars, exprs = es, body = corpo }
    end
    esperar("end", contexto("for", linha))
    fecharEscopo(mark)
    return no
end

local function statement()
    local t = T[P]
    local linha = L[P]
    if t == "if" then
        P += 1
        local conds, blocos, senao = {}, {}, nil
        conds[1] = expr()
        esperar("then")
        blocos[1] = bloco()
        while true do
            if aceitar("elseif") then
                conds[#conds + 1] = expr()
                esperar("then")
                blocos[#blocos + 1] = bloco()
            elseif aceitar("else") then
                senao = bloco()
                esperar("end", contexto("if", linha))
                break
            else
                esperar("end", contexto("if", linha))
                break
            end
        end
        return { t = "if", conds = conds, blocos = blocos, senao = senao }
    elseif t == "while" then
        P += 1
        local c = expr()
        esperar("do")
        local corpo = bloco()
        esperar("end", contexto("while", linha))
        return { t = "while", cond = c, body = corpo }
    elseif t == "do" then
        P += 1
        local corpo = bloco()
        esperar("end", contexto("do", linha))
        return { t = "do", body = corpo }
    elseif t == "for" then
        P += 1
        return paraStat(linha)
    elseif t == "repeat" then
        P += 1
        local mark = #fs.act
        local corpo = bloco(true)
        esperar("until", contexto("repeat", linha))
        local c = expr()
        fecharEscopo(mark)
        return { t = "repeat", body = corpo, cond = c }
    elseif t == "function" or t == "@" then
        skipAtributos()
        if T[P] == "local" then
            P += 1
            t = "local"
        else
            P += 1
            local alvo = resolver(esperarNome())
            local metodo = false
            while T[P] == "." or T[P] == ":" do
                local dois = T[P] == ":"
                P += 1
                alvo = { t = "index", obj = alvo, key = constNo(esperarNome()) }
                if dois then metodo = true; break end
            end
            local fn = corpoFuncao(metodo, linha)
            return { t = "assign", targets = { alvo }, exprs = { fn } }
        end
    elseif t == "return" then
        P += 1
        local es = {}
        local n = T[P]
        if not (fimBloco(n) or n == ";") then es = listaExpr() end
        return { t = "return", exprs = es }
    elseif t == "break" then
        P += 1
        return { t = "break" }
    end
    if t == "local" then
        if T[P] == "local" then P += 1 end
        if T[P] == "function" then
            P += 1
            local v = declarar(esperarNome())
            local fn = corpoFuncao(false, linha)
            return { t = "localfn", var = v, fn = fn }
        end
        local nomes = {}
        repeat
            nomes[#nomes + 1] = esperarNome()
            if aceitar(":") then skipTipo() end
        until not aceitar(",")
        local es = {}
        if aceitar("=") then es = listaExpr() end
        local vars = {}
        for i = 1, #nomes do vars[i] = declarar(nomes[i]) end
        return { t = "local", vars = vars, exprs = es }
    end
    if t == "name" then
        local nome, prox = V[P], T[P + 1]
        if nome == "continue" and (fimBloco(prox) or prox == ";" or prox == "name" or prox == "local"
            or prox == "if" or prox == "for" or prox == "while" or prox == "return" or prox == "break"
            or prox == "do" or prox == "repeat" or prox == "function") then
            P += 1
            return { t = "continue" }
        elseif nome == "type" and prox == "name" then
            P += 1
            declaracaoTipo()
            return nil
        elseif nome == "export" and prox == "name" and V[P + 1] == "type" then
            P += 2
            declaracaoTipo()
            return nil
        end
    end
    local e = primario()
    local o = T[P]
    if o == "=" or o == "," then
        local alvos = { e }
        while aceitar(",") do alvos[#alvos + 1] = primario() end
        esperar("=")
        for i = 1, #alvos do
            local k = alvos[i].t
            if k ~= "local" and k ~= "upv" and k ~= "global" and k ~= "index" then erro("Assigned expression must be a variable or a field") end
        end
        return { t = "assign", targets = alvos, exprs = listaExpr() }
    elseif OPCOMP[o] then
        P += 1
        return { t = "compound", op = OPCOMP[o], target = e, e = expr() }
    end
    if e.t ~= "call" and e.t ~= "method" then erro("Expected identifier when parsing expression statement, got '" .. descTok() .. "'") end
    return { t = "callstat", e = e }
end

-- manter: não fecha o escopo (o `until` do repeat enxerga os locais do corpo)
bloco = function(manter)
    local lista = {}
    local mark = #fs.act
    while not fimBloco(T[P]) do
        local s = statement()
        if s then lista[#lista + 1] = s end
        aceitar(";")
        if s and s.t == "return" then break end
    end
    if not manter then fecharEscopo(mark) end
    return lista
end

-- Compilador (AST -> closures) -------------------------------------------------

local CE, CS = {}, {}
local cexp, cmulti, cblock, compilarFuncao
local CF

local function ehMulti(n)
    local t = n.t
    return t == "call" or t == "method" or t == "vararg"
end

-- 1 = constante, 2 = local simples (sem caixa), 0 = qualquer outra
local function classif(n)
    if n.t == "const" then return 1, n.v end
    if n.t == "local" and not n.var.captured then return 2, n.var.slot end
    return 0
end

local function slotDe(n)
    if n.t == "upv" then return CF.np + n.idx, true end
    return n.var.slot, n.var.captured
end

cexp = function(n)
    return CE[n.t](n)
end

cmulti = function(n)
    local t = n.t
    if t == "call" then return CE.callm(n) end
    if t == "method" then return CE.methodm(n) end
    local s = n.var.slot
    return function(f)
        local va = f[s]
        return unpack(va, 1, va.n)
    end
end

CE.const = function(n)
    local v = n.v
    return function() return v end
end

CE["local"] = function(n)
    local s, caixa = slotDe(n)
    if caixa then return function(f) return f[s][1] end end
    return function(f) return f[s] end
end
CE.upv = CE["local"]

CE.global = function(n)
    local nome = n.name
    return function() return genv[nome] end
end

CE.vararg = function(n)
    local s = n.var.slot
    return function(f) return f[s][1] end
end

CE.paren = function(n)
    return cexp(n.e)
end

CE.index = function(n)
    local ob, chave = n.obj, n.key
    if chave.t == "const" then
        local k = chave.v
        local ko, so = classif(ob)
        if ko == 2 then return function(f) return f[so][k] end end
        local o = cexp(ob)
        return function(f) return o(f)[k] end
    end
    local o, k = cexp(ob), cexp(chave)
    return function(f) return o(f)[k(f)] end
end

-- Operadores binários: gg = geral/geral, gk = direita constante,
-- lk = local/constante, ll = local/local. Cada variante poupa chamadas.
local BIN = {}
BIN["+"] = {
    gg = function(a, b) return function(f) return a(f) + b(f) end end,
    gk = function(a, k) return function(f) return a(f) + k end end,
    lk = function(s, k) return function(f) return f[s] + k end end,
    ll = function(s, t) return function(f) return f[s] + f[t] end end }
BIN["-"] = {
    gg = function(a, b) return function(f) return a(f) - b(f) end end,
    gk = function(a, k) return function(f) return a(f) - k end end,
    lk = function(s, k) return function(f) return f[s] - k end end,
    ll = function(s, t) return function(f) return f[s] - f[t] end end }
BIN["*"] = {
    gg = function(a, b) return function(f) return a(f) * b(f) end end,
    gk = function(a, k) return function(f) return a(f) * k end end,
    lk = function(s, k) return function(f) return f[s] * k end end,
    ll = function(s, t) return function(f) return f[s] * f[t] end end }
BIN["%"] = {
    gg = function(a, b) return function(f) return a(f) % b(f) end end,
    gk = function(a, k) return function(f) return a(f) % k end end,
    lk = function(s, k) return function(f) return f[s] % k end end,
    ll = function(s, t) return function(f) return f[s] % f[t] end end }
BIN["<"] = {
    gg = function(a, b) return function(f) return a(f) < b(f) end end,
    gk = function(a, k) return function(f) return a(f) < k end end,
    lk = function(s, k) return function(f) return f[s] < k end end,
    ll = function(s, t) return function(f) return f[s] < f[t] end end }
BIN["<="] = {
    gg = function(a, b) return function(f) return a(f) <= b(f) end end,
    gk = function(a, k) return function(f) return a(f) <= k end end,
    lk = function(s, k) return function(f) return f[s] <= k end end,
    ll = function(s, t) return function(f) return f[s] <= f[t] end end }
BIN[">"] = {
    gg = function(a, b) return function(f) return a(f) > b(f) end end,
    gk = function(a, k) return function(f) return a(f) > k end end,
    lk = function(s, k) return function(f) return f[s] > k end end,
    ll = function(s, t) return function(f) return f[s] > f[t] end end }
BIN[">="] = {
    gg = function(a, b) return function(f) return a(f) >= b(f) end end,
    gk = function(a, k) return function(f) return a(f) >= k end end,
    lk = function(s, k) return function(f) return f[s] >= k end end,
    ll = function(s, t) return function(f) return f[s] >= f[t] end end }
BIN["=="] = {
    gg = function(a, b) return function(f) return a(f) == b(f) end end,
    gk = function(a, k) return function(f) return a(f) == k end end,
    lk = function(s, k) return function(f) return f[s] == k end end,
    ll = function(s, t) return function(f) return f[s] == f[t] end end }
BIN["~="] = {
    gg = function(a, b) return function(f) return a(f) ~= b(f) end end,
    gk = function(a, k) return function(f) return a(f) ~= k end end,
    lk = function(s, k) return function(f) return f[s] ~= k end end,
    ll = function(s, t) return function(f) return f[s] ~= f[t] end end }

-- Operadores sem variante especializada, e as mesmas contas como função pura
-- (compostos em campo precisam avaliar o alvo uma vez só).
local OPF = {
    ["+"] = function(a, b) return a + b end, ["-"] = function(a, b) return a - b end,
    ["*"] = function(a, b) return a * b end, ["/"] = function(a, b) return a / b end,
    ["//"] = function(a, b) return a // b end, ["%"] = function(a, b) return a % b end,
    ["^"] = function(a, b) return a ^ b end, [".."] = function(a, b) return a .. b end,
}
local GEN = {
    ["/"] = function(a, b) return function(f) return a(f) / b(f) end end,
    ["//"] = function(a, b) return function(f) return a(f) // b(f) end end,
    ["^"] = function(a, b) return function(f) return a(f) ^ b(f) end end,
    [".."] = function(a, b) return function(f) return a(f) .. b(f) end end,
}

CE.bin = function(n)
    local op = n.op
    local B = BIN[op]
    if not B then return GEN[op](cexp(n.a), cexp(n.b)) end
    local ka, va = classif(n.a)
    local kb, vb = classif(n.b)
    if ka == 2 and kb == 1 then return B.lk(va, vb) end
    if ka == 2 and kb == 2 then return B.ll(va, vb) end
    if kb == 1 then return B.gk(cexp(n.a), vb) end
    return B.gg(cexp(n.a), cexp(n.b))
end

CE["and"] = function(n)
    local a, b = cexp(n.a), cexp(n.b)
    return function(f)
        local v = a(f)
        if v then return b(f) end
        return v
    end
end

CE["or"] = function(n)
    local a, b = cexp(n.a), cexp(n.b)
    return function(f)
        local v = a(f)
        if v then return v end
        return b(f)
    end
end

CE.un = function(n)
    local e, op = cexp(n.e), n.op
    if op == "not" then return function(f) return not e(f) end end
    if op == "-" then return function(f) return -e(f) end end
    return function(f) return #e(f) end
end

CE.ifexp = function(n)
    local c, a, b = cexp(n.c), cexp(n.a), cexp(n.b)
    return function(f)
        if c(f) then return a(f) end
        return b(f)
    end
end

CE.interp = function(n)
    local lit, ex, np = {}, {}, #n.parts
    for i = 1, np do
        local p = n.parts[i]
        if type(p) == "string" then lit[i] = p else lit[i] = false; ex[i] = cexp(p) end
    end
    return function(f)
        local s = ""
        for i = 1, np do
            local l = lit[i]
            if l then s = s .. l else s = s .. tostring(ex[i](f)) end
        end
        return s
    end
end

CE.table = function(n)
    local es, ne = n.entries, #n.entries
    if ne == 0 then return function() return {} end end
    local multi = ehMulti(es[ne].v) and not es[ne].k
    local nf = multi and ne - 1 or ne
    local ks, vs, soPos = {}, {}, true
    for i = 1, nf do
        ks[i] = es[i].k and cexp(es[i].k) or false
        vs[i] = cexp(es[i].v)
        if ks[i] then soPos = false end
    end
    local ultimo = multi and cmulti(es[ne].v)
    if soPos and not ultimo then
        local a, b, c = vs[1], vs[2], vs[3]
        if nf == 1 then return function(f) return { a(f) } end end
        if nf == 2 then return function(f) return { a(f), b(f) } end end
        if nf == 3 then return function(f) return { a(f), b(f), c(f) } end end
    end
    return function(f)
        local t, c = {}, 0
        for i = 1, nf do
            local k = ks[i]
            if k then
                t[k(f)] = vs[i](f)
            else
                c += 1
                t[c] = vs[i](f)
            end
        end
        if ultimo then
            local p = pack(ultimo(f))
            move(p, 1, p.n, c + 1, t)
        end
        return t
    end
end

-- Lista de argumentos/valores em geral: devolve tabela e quantidade.
local function cLista(es)
    local ne = #es
    local multi = ne > 0 and ehMulti(es[ne])
    local nf = multi and ne - 1 or ne
    local cs = {}
    for i = 1, nf do cs[i] = cexp(es[i]) end
    local ultimo = multi and cmulti(es[ne])
    return function(f)
        local t = {}
        for i = 1, nf do t[i] = cs[i](f) end
        if ultimo then
            local p = pack(ultimo(f))
            move(p, 1, p.n, nf + 1, t)
            return t, nf + p.n
        end
        return t, nf
    end
end

local function semFuncao(p, desc)
    erroNativo(p .. " attempt to call a nil value" .. desc, 0)
end

local function descChamada(n)
    local f = n.fn
    if f.t == "global" then return " (global '" .. f.name .. "')" end
    if f.t == "local" or f.t == "upv" then return " (local '" .. f.var.name .. "')" end
    if f.t == "index" and f.key.t == "const" and type(f.key.v) == "string" then return " (field '" .. f.key.v .. "')" end
    return ""
end

-- modo 1 devolve só o primeiro valor; modo 2 devolve todos (e vira tail call)
local function montarChamada(n, modo)
    local fn, args = cexp(n.fn), n.args
    local PP, DD = CN .. ":" .. n.line .. ":", descChamada(n)
    local na = #args
    if na > 0 and ehMulti(args[na]) then na = -1 end
    local a, b, c = args[1] and cexp(args[1]), args[2] and cexp(args[2]), args[3] and cexp(args[3])
    if na == 0 then
        if modo == 1 then
            return function(f)
                local g = fn(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g())
            end
        end
        return function(f)
            local g = fn(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g()
        end
    elseif na == 1 then
        if modo == 1 then
            return function(f)
                local g = fn(f)
                local x = a(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(x))
            end
        end
        return function(f)
            local g = fn(f)
            local x = a(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(x)
        end
    elseif na == 2 then
        if modo == 1 then
            return function(f)
                local g = fn(f)
                local x, y = a(f), b(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(x, y))
            end
        end
        return function(f)
            local g = fn(f)
            local x, y = a(f), b(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(x, y)
        end
    elseif na == 3 then
        if modo == 1 then
            return function(f)
                local g = fn(f)
                local x, y, z = a(f), b(f), c(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(x, y, z))
            end
        end
        return function(f)
            local g = fn(f)
            local x, y, z = a(f), b(f), c(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(x, y, z)
        end
    end
    local ca = cLista(args)
    if modo == 1 then
        return function(f)
            local g = fn(f)
            local t, k = ca(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return (g(unpack(t, 1, k)))
        end
    end
    return function(f)
        local g = fn(f)
        local t, k = ca(f)
        if g == nil then semFuncao(PP, DD) end
        pos = PP
        return g(unpack(t, 1, k))
    end
end

local function montarMetodo(n, modo)
    local ob, nome, args = cexp(n.obj), n.name, n.args
    local PP, DD = CN .. ":" .. n.line .. ":", " (method '" .. n.name .. "')"
    local na = #args
    if na > 0 and ehMulti(args[na]) then na = -1 end
    local a, b = args[1] and cexp(args[1]), args[2] and cexp(args[2])
    if na == 0 then
        if modo == 1 then
            return function(f)
                local o = ob(f)
                local g = o[nome]
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(o))
            end
        end
        return function(f)
            local o = ob(f)
            local g = o[nome]
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(o)
        end
    elseif na == 1 then
        if modo == 1 then
            return function(f)
                local o = ob(f)
                local g = o[nome]
                local x = a(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(o, x))
            end
        end
        return function(f)
            local o = ob(f)
            local g = o[nome]
            local x = a(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(o, x)
        end
    elseif na == 2 then
        if modo == 1 then
            return function(f)
                local o = ob(f)
                local g = o[nome]
                local x, y = a(f), b(f)
                if g == nil then semFuncao(PP, DD) end
                pos = PP
                return (g(o, x, y))
            end
        end
        return function(f)
            local o = ob(f)
            local g = o[nome]
            local x, y = a(f), b(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return g(o, x, y)
        end
    end
    local ca = cLista(args)
    if modo == 1 then
        return function(f)
            local o = ob(f)
            local g = o[nome]
            local t, k = ca(f)
            if g == nil then semFuncao(PP, DD) end
            pos = PP
            return (g(o, unpack(t, 1, k)))
        end
    end
    return function(f)
        local o = ob(f)
        local g = o[nome]
        local t, k = ca(f)
        if g == nil then semFuncao(PP, DD) end
        pos = PP
        return g(o, unpack(t, 1, k))
    end
end

CE.call = function(n) return montarChamada(n, 1) end
CE.callm = function(n) return montarChamada(n, 2) end
CE.method = function(n) return montarMetodo(n, 1) end
CE.methodm = function(n) return montarMetodo(n, 2) end

-- Funções ----------------------------------------------------------------------
--
-- Frame = tabela: [1..np] parâmetros, [np+1..np+nu] caixas dos upvalues, resto
-- locais. Variável capturada vive numa caixa {valor}; a closure copia a caixa na
-- criação, então cada iteração de loop tem a sua.
--
-- Corpo devolve (sinal, valor): 1 break, 2 continue, 3 um valor, 4 pacote,
-- 5 return vazio, 6 tail call (valor = closure que faz a chamada).

local function fabricaFuncao(np, nu, bp, vaslot, ns, corpo)
    if not vaslot and not bp and np <= 3 then
        if np == 0 then
            return function(u)
                return function()
                    local g = tcreate(ns)
                    for i = 1, nu do g[i] = u[i] end
                    local s, v = corpo(g)
                    if s == 3 then return v elseif s == 4 then return unpack(v, 1, v.n) elseif s == 6 then return v(g) end
                end
            end
        elseif np == 1 then
            return function(u)
                return function(a)
                    local g = tcreate(ns)
                    g[1] = a
                    for i = 1, nu do g[1 + i] = u[i] end
                    local s, v = corpo(g)
                    if s == 3 then return v elseif s == 4 then return unpack(v, 1, v.n) elseif s == 6 then return v(g) end
                end
            end
        elseif np == 2 then
            return function(u)
                return function(a, b)
                    local g = tcreate(ns)
                    g[1] = a; g[2] = b
                    for i = 1, nu do g[2 + i] = u[i] end
                    local s, v = corpo(g)
                    if s == 3 then return v elseif s == 4 then return unpack(v, 1, v.n) elseif s == 6 then return v(g) end
                end
            end
        end
        return function(u)
            return function(a, b, c)
                local g = tcreate(ns)
                g[1] = a; g[2] = b; g[3] = c
                for i = 1, nu do g[3 + i] = u[i] end
                local s, v = corpo(g)
                if s == 3 then return v elseif s == 4 then return unpack(v, 1, v.n) elseif s == 6 then return v(g) end
            end
        end
    end
    return function(u)
        return function(...)
            local args = pack(...)
            local g = tcreate(ns)
            for i = 1, np do g[i] = args[i] end
            if vaslot then
                local va = { n = args.n > np and args.n - np or 0 }
                if va.n > 0 then move(args, np + 1, args.n, 1, va) end
                g[vaslot] = va
            end
            for i = 1, nu do g[np + i] = u[i] end
            if bp then
                for i = 1, #bp do
                    local s = bp[i]
                    g[s] = { g[s] }
                end
            end
            local s, v = corpo(g)
            if s == 3 then return v elseif s == 4 then return unpack(v, 1, v.n) elseif s == 6 then return v(g) end
        end
    end
end

-- devolve a fábrica e de onde copiar cada caixa de upvalue no frame do pai
compilarFuncao = function(n)
    local nfs, np = n.fs, n.np
    local nu = #nfs.ups
    local k = np + nu
    for i, v in nfs.vars do
        if i <= np then
            v.slot = i
        else
            k += 1
            v.slot = k
        end
    end
    local pai = CF
    CF = { np = np }
    local corpo = cblock(n.body)
    CF = pai
    local bp
    for i = 1, np do
        if nfs.vars[i].captured then
            bp = bp or {}
            bp[#bp + 1] = i
        end
    end
    local origem = {}
    for i = 1, nu do
        local v = nfs.ups[i]
        origem[i] = v.fs == nfs.parent and v.slot or (pai.np + nfs.parent.upidx[v])
    end
    return fabricaFuncao(np, nu, bp, nfs.vavar and nfs.vavar.slot, k, corpo), origem, nu
end

CE["function"] = function(n)
    local mk, origem, nu = compilarFuncao(n)
    if nu == 0 then return function() return mk(nil) end end
    if nu == 1 then
        local s1 = origem[1]
        return function(f) return mk({ f[s1] }) end
    end
    return function(f)
        local u = {}
        for i = 1, nu do u[i] = f[origem[i]] end
        return mk(u)
    end
end

-- Instruções -------------------------------------------------------------------

local function sequencia(cs)
    local n = #cs
    if n == 0 then return function() end end
    if n == 1 then return cs[1] end
    local a, b, c, d = cs[1], cs[2], cs[3], cs[4]
    if n == 2 then
        return function(f)
            local s, v = a(f)
            if s then return s, v end
            return b(f)
        end
    elseif n == 3 then
        return function(f)
            local s, v = a(f)
            if s then return s, v end
            s, v = b(f)
            if s then return s, v end
            return c(f)
        end
    elseif n == 4 then
        return function(f)
            local s, v = a(f)
            if s then return s, v end
            s, v = b(f)
            if s then return s, v end
            s, v = c(f)
            if s then return s, v end
            return d(f)
        end
    end
    return function(f)
        for i = 1, n do
            local s, v = cs[i](f)
            if s then return s, v end
        end
    end
end

cblock = function(lista)
    local cs = {}
    for i = 1, #lista do
        local s = lista[i]
        cs[i] = CS[s.t](s)
    end
    return sequencia(cs)
end

CS.callstat = function(n)
    local c = cmulti(n.e)
    return function(f) c(f) end
end

CS["do"] = function(n)
    return cblock(n.body)
end

CS["break"] = function() return function() return 1 end end
CS.continue = function() return function() return 2 end end

CS["return"] = function(n)
    local es = n.exprs
    local ne = #es
    if ne == 0 then return function() return 5 end end
    if ne == 1 then
        if ehMulti(es[1]) then
            local m = cmulti(es[1])
            return function() return 6, m end
        end
        local e = cexp(es[1])
        return function(f) return 3, e(f) end
    end
    local l = cLista(es)
    return function(f)
        local t, k = l(f)
        t.n = k
        return 4, t
    end
end

CS["local"] = function(n)
    local vars, es = n.vars, n.exprs
    local nv = #vars
    if nv == 1 and #es == 1 then
        local s, e = vars[1].slot, cexp(es[1])
        if vars[1].captured then
            return function(f) local v = e(f); f[s] = { v } end
        end
        return function(f) f[s] = e(f) end
    end
    if #es == 0 then
        local slots, caix = {}, {}
        for i = 1, nv do slots[i] = vars[i].slot; caix[i] = vars[i].captured end
        return function(f)
            for i = 1, nv do
                f[slots[i]] = caix[i] and {} or nil
            end
        end
    end
    local l = cLista(es)
    local slots, caix = {}, {}
    for i = 1, nv do slots[i] = vars[i].slot; caix[i] = vars[i].captured end
    return function(f)
        local t = l(f)
        for i = 1, nv do
            if caix[i] then f[slots[i]] = { t[i] } else f[slots[i]] = t[i] end
        end
    end
end

CS.localfn = function(n)
    local s, mk = n.var.slot, cexp(n.fn)
    if n.var.captured then
        return function(f)
            local caixa = {}
            f[s] = caixa
            caixa[1] = mk(f)
        end
    end
    return function(f) f[s] = mk(f) end
end

-- alvo de atribuição: devolve (preparo, gravador). `preparo` (só em campo)
-- avalia objeto e chave; `gravador(f, obj, chave, valor)` grava.
local function cAlvo(a)
    local t = a.t
    if t == "global" then
        local nome = a.name
        return false, function(f, o, k, v) genv[nome] = v end
    elseif t == "index" then
        local o, k = cexp(a.obj), cexp(a.key)
        return function(f) return o(f), k(f) end, function(f, ob, ch, v) ob[ch] = v end
    end
    local s, caixa = slotDe(a)
    if caixa then return false, function(f, o, k, v) f[s][1] = v end end
    return false, function(f, o, k, v) f[s] = v end
end

local function atribuicaoSimples(a, e)
    local t = a.t
    if t == "global" then
        local nome = a.name
        return function(f) genv[nome] = e(f) end
    elseif t == "index" then
        local ko, so
        if a.key.t == "const" then
            local k = a.key.v
            ko, so = classif(a.obj)
            if ko == 2 then return function(f) f[so][k] = e(f) end end
            local o = cexp(a.obj)
            return function(f) o(f)[k] = e(f) end
        end
        local o, k = cexp(a.obj), cexp(a.key)
        return function(f) o(f)[k(f)] = e(f) end
    end
    local s, caixa = slotDe(a)
    if caixa then return function(f) f[s][1] = e(f) end end
    return function(f) f[s] = e(f) end
end

CS.assign = function(n)
    local alvos, es = n.targets, n.exprs
    local na = #alvos
    if na == 1 and #es == 1 then
        return atribuicaoSimples(alvos[1], cexp(es[1]))
    end
    local prep, grav = {}, {}
    for i = 1, na do prep[i], grav[i] = cAlvo(alvos[i]) end
    local l = cLista(es)
    return function(f)
        local objs, chaves = {}, {}
        for i = 1, na do
            local p = prep[i]
            if p then objs[i], chaves[i] = p(f) end
        end
        local t = l(f)
        for i = na, 1, -1 do grav[i](f, objs[i], chaves[i], t[i]) end
    end
end

CS.compound = function(n)
    local a = n.target
    if a.t ~= "index" then
        return atribuicaoSimples(a, cexp({ t = "bin", op = n.op, a = a, b = n.e }))
    end
    local prep, grav = cAlvo(a)
    local e, op = cexp(n.e), OPF[n.op]
    return function(f)
        local o, k = prep(f)
        o[k] = op(o[k], e(f))
    end
end

local function encadearIf(conds, blocos, i, senao)
    if i > #conds then return senao end
    local c, t = cexp(conds[i]), cblock(blocos[i])
    local e = encadearIf(conds, blocos, i + 1, senao)
    if not e then
        return function(f)
            if c(f) then return t(f) end
        end
    end
    return function(f)
        if c(f) then return t(f) end
        return e(f)
    end
end

CS["if"] = function(n)
    return encadearIf(n.conds, n.blocos, 1, n.senao and cblock(n.senao))
end

CS["while"] = function(n)
    local c, corpo = cexp(n.cond), cblock(n.body)
    return function(f)
        while c(f) do
            local s, v = corpo(f)
            if s then
                if s == 1 then break elseif s ~= 2 then return s, v end
            end
        end
    end
end

CS["repeat"] = function(n)
    local corpo, c = cblock(n.body), cexp(n.cond)
    return function(f)
        repeat
            local s, v = corpo(f)
            if s then
                if s == 1 then break elseif s ~= 2 then return s, v end
            end
        until c(f)
    end
end

CS.fornum = function(n)
    local s, a, b, corpo = n.var.slot, cexp(n.a), cexp(n.b), cblock(n.body)
    local c = n.c and cexp(n.c)
    local caixa = n.var.captured
    if not c and not caixa then
        return function(f)
            for i = a(f), b(f) do
                f[s] = i
                local sg, v = corpo(f)
                if sg then
                    if sg == 1 then break elseif sg ~= 2 then return sg, v end
                end
            end
        end
    end
    c = c or function() return 1 end
    return function(f)
        for i = a(f), b(f), c(f) do
            if caixa then f[s] = { i } else f[s] = i end
            local sg, v = corpo(f)
            if sg then
                if sg == 1 then break elseif sg ~= 2 then return sg, v end
            end
        end
    end
end

-- iteração generalizada: tabela direta itera como pairs; __iter assume o controle
local function prepararIter(fn, st, ct)
    if type(fn) == "table" then
        local mt = getmetatable(fn)
        local it = type(mt) == "table" and rawget(mt, "__iter")
        if it then return it(fn) end
        return next, fn, nil
    end
    return fn, st, ct
end

CS.forin = function(n)
    local vars, corpo = n.vars, cblock(n.body)
    local l = cLista(n.exprs)
    local nv = #vars
    local slots, caix, algumaCaixa = {}, {}, false
    for i = 1, nv do
        slots[i] = vars[i].slot
        caix[i] = vars[i].captured
        algumaCaixa = algumaCaixa or caix[i]
    end
    local s1, s2 = slots[1], slots[2]
    if nv <= 2 and not algumaCaixa then
        return function(f)
            local t = l(f)
            local fn, st, ct = prepararIter(t[1], t[2], t[3])
            while true do
                local x, y = fn(st, ct)
                if x == nil then break end
                ct = x
                f[s1] = x
                if s2 then f[s2] = y end
                local sg, v = corpo(f)
                if sg then
                    if sg == 1 then break elseif sg ~= 2 then return sg, v end
                end
            end
        end
    end
    return function(f)
        local t = l(f)
        local fn, st, ct = prepararIter(t[1], t[2], t[3])
        while true do
            local r = pack(fn(st, ct))
            if r[1] == nil then break end
            ct = r[1]
            for i = 1, nv do
                if caix[i] then f[slots[i]] = { r[i] } else f[slots[i]] = r[i] end
            end
            local sg, v = corpo(f)
            if sg then
                if sg == 1 then break elseif sg ~= 2 then return sg, v end
            end
        end
    end
end

-- Entrada ----------------------------------------------------------------------

local function nomeExibicao(nome, src)
    if type(nome) == "string" then
        local c = ssub(nome, 1, 1)
        if c == "=" or c == "@" then return ssub(nome, 2) end
        return '[string "' .. nome .. '"]'
    end
    local linha = smatch(src, "^[^\n]*")
    local cortou = #linha < #src
    if #linha > 40 then linha = ssub(linha, 1, 37); cortou = true end
    return '[string "' .. linha .. (cortou and '..."]' or '"]')
end

local function compilarChunk(src, cn)
    CN = cn
    local tt, tv, tl = lex(src, 1, 1, false)
    T, V, L, P = tt, tv, tl, 1
    fs = novaFuncao()
    fs.vavar = declarar("...")
    local corpo = bloco()
    if T[P] ~= "eof" then erro("Expected <eof>, got '" .. descTok() .. "'") end
    local raiz = { t = "function", fs = fs, np = 0, body = corpo }
    fs = nil
    local mk = compilarFuncao(raiz)
    return mk(nil)
end

local function loadstring(src, nome)
    if type(src) ~= "string" then
        erroNativo("invalid argument #1 to 'loadstring' (string expected, got " .. typeof(src) .. ")", 2)
    end
    local cn = nomeExibicao(nome, src)
    local ok, res = coroutine.resume(coroutine.create(compilarChunk), src, cn)
    if not ok then return nil, res end
    return res
end

base.loadstring = loadstring
return loadstring
end)()))
