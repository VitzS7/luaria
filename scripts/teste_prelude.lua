-- Teste do prelude (assets/prelude.lua + loadstring.lua) FORA do jogo, no REPL
-- do Luau. Roblox não existe aqui: Random/game/task são dublês mínimos, então só
-- vale para o que é Lua puro (crypt, lz4, closures, arquivos, ponte de globais).
--
-- Uso (o marcador recebe o prelude, sem tocar no repo):
--   sed -e '/^--@@PRELUDE@@/r assets/prelude.lua' -e '/^--@@PRELUDE@@/r assets/loadstring.lua' \
--       scripts/teste_prelude.lua > /tmp/p.lua && luau /tmp/p.lua

Random = { new = function()
    return { NextInteger = function(_, a, b) return math.random(a, b) end }
end }
-- relógio falso: o request mede prazo com os.clock e cede com task.wait
local relogio = 0
os = setmetatable({ clock = function() return relogio end }, { __index = os })
task = { wait = function() relogio += 0.1 end }
game = {
    __inst = true,
    FindFirstChildOfClass = function() return nil end,
    GetService = function() return {} end,
    GetChildren = function() return {} end,
}
-- typeof de Roblox: tabela com __inst é Instance (o resto cai no typeof do Luau)
local typeof_luau = typeof
typeof = function(v)
    if type(v) == "table" and rawget(v, "__inst") then
        return "Instance"
    end
    return typeof_luau(v)
end
UserSettings = function()
    return { GetService = function() return ugs end }
end
ugs = { FramerateCap = 144 }
-- dublês de GUI só para o Drawing andar
local criadas = {}
Instance = { new = function(classe)
    local i = { ClassName = classe, __inst = true }
    function i.Destroy() i.destruida = true end
    function i.IsA(_, c) return c == "Instance" or c == classe end
    criadas[#criadas + 1] = i
    return i
end }
local function v2(x, y)
    return setmetatable({ X = x, Y = y, Magnitude = math.sqrt(x * x + y * y) }, {
        __sub = function(a, b) return v2(a.X - b.X, a.Y - b.Y) end,
    })
end
Vector2 = { new = v2, zero = v2(0, 0) }
UDim2 = { fromOffset = function(x, y) return { x = x, y = y } end }
UDim = { new = function(a, b) return { a, b } end }
Color3 = { new = function(r, g, b) return { r, g, b } end }
Enum = setmetatable({}, { __index = function(_, k)
    return setmetatable({}, { __index = function(_, v) return k .. "." .. v end })
end })
local impressos = {}
local print_real = print
print = function(s) impressos[#impressos + 1] = s end

local __luaria_arquivos = { token = "tk" }
-- dublês dos nativos (nativas.rs); o caminho sem eles é testado à parte, tirando estas linhas
local mts, congeladas = setmetatable({}, { __mode = "k" }), setmetatable({}, { __mode = "k" })
__n_grm = function(o) return mts[o] end
__n_srm = function(...) assert(select("#", ...) == 2, "srm exige 2 args"); local o, mt = ...; mts[o] = mt end
__n_sro = function(t, v) congeladas[t] = v end
-- gc_extra: objetos que o teste põe no GC (instâncias sem pai, envs de script)
local gc_extra = {}
__n_gg = function(incluir)
    assert(type(incluir) == "boolean", "gg exige boolean")
    local lista = incluir and { {}, print } or { print }
    for _, x in gc_extra do lista[#lista + 1] = x end
    return lista
end
-- constantes por função: consts[f] = { n = qtd, valores... }; fora da faixa não devolve nada, como o nativo
local consts = setmetatable({}, { __mode = "k" })
__n_gk = function(f, i)
    local k = consts[f]
    if not k or i < 1 or i > k.n then return end
    return k[i]
end
-- escrita de constante/upvalue: mesmo contrato do nativo (tipo igual ou nil; índice 1..n)
__n_sk = function(f, i, v)
    local k = consts[f]
    if not k or i < 1 or i > k.n then return end
    local atual = k[i]
    if atual ~= nil and type(atual) ~= type(v) then return end
    k[i] = v
    return true
end
local upv = setmetatable({}, { __mode = "k" })
__n_su = function(f, i, v)
    if i ~= 1 or not upv[f] then return end
    upv[f] = v
    return true
end
-- canal do request (ponte_nativas.rs): cada teste troca pe/pr pelo roteiro dele
local canal = { enviados = {}, cancelados = 0, pe = function() return 8 end, pr = function() return 1, 200, "" end }
__n_pe = function(tipo, payload)
    canal.enviados[#canal.enviados + 1] = { tipo = tipo, payload = payload }
    return canal.pe(tipo, payload)
end
__n_pr = function(h, cancelar)
    if cancelar then
        canal.cancelados += 1
        return 2
    end
    relogio += 0.01
    return canal.pr(h)
end
local table = setmetatable({ isfrozen = function(t) return congeladas[t] == true end }, { __index = table })
local __luaria_env, __luaria_ponte = (function()
--@@PRELUDE@@
return env, ponte_globais
end)()
local E = __luaria_env
print = print_real

local total, falhas = 0, 0
local function caso(nome, f)
    total += 1
    local ok, e = coroutine.resume(coroutine.create(f))
    if not ok then
        falhas += 1
        print_real("FALHOU " .. nome .. ": " .. tostring(e))
    end
end
local function erro_de(f, ...)
    local ok, e = coroutine.resume(coroutine.create(f), ...)
    return not ok and tostring(e) or nil
end

-- crypt.hash -------------------------------------------------------------------
local HASH = {
    test = {
        md5 = "098f6bcd4621d373cade4e832627b4f6",
        sha1 = "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3",
        sha224 = "90a3ed9e32b2aaf4c61c410eb925426119e1a9dc53d4286ade99a809",
        sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        sha384 = "768412320f7b0aa5812fce428dc4706b3cae50e02a64caa16a782249bfe8efc4b7ef1ccb126255d196047dfedf17a0a9",
        sha512 = "ee26b0dd4af7e749aa1a8ee3c10ae9923f618980772e473f8819a5d4940e0db27ac185f8a0e1d5f84f88bc887fd67b143732c304cc5fa9ad8e6f57f50028a8ff",
        ["sha3-224"] = "3797bf0afbbfca4a7bbba7602a2b552746876517a7f9b7ce2db0ae7b",
        ["sha3-256"] = "36f028580bb02cc8272a9a020f4200e346e276ae664e45ee80745574e2f5ab80",
        ["sha3-384"] = "e516dabb23b6e30026863543282780a3ae0dccf05551cf0295178d7ff0f1b41eecb9db3ff219007c4e097260d58621bd",
        ["sha3-512"] = "9ece086e9bac491fac5c1d1046ca11d737b92a2b2ebd93f005d7b710110c0a678288166e7fbe796883a4f2e9b3ca9f484f521d0ce464345cc1aec96779149c14",
    },
    [""] = {
        md5 = "d41d8cd98f00b204e9800998ecf8427e",
        sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        sha512 = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
        ["sha3-256"] = "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a",
    },
}
local longo = {}
for i = 0, 255 do longo[#longo + 1] = string.char(i) end
longo = string.rep(table.concat(longo), 3)
HASH[longo] = {
    md5 = "e6899eaaf06fd702f3ed3f988eb19362",
    sha1 = "ac2a264c8ec1f4232a40854e8239bc3a697ab1d2",
    sha224 = "591b51555cc8c83f6951d9ec73d834d6d17865fecdb543605471a562",
    sha256 = "f3a25aa93aa2fbba28d79260535bbd6a5eb0fc1c24a8b0f04e12b484c1dfe363",
    sha384 = "6bee04118003cf5630ccb22cdb53f78dc079231c7d09209955dfc34234cbab03d41da198e30bf1e77ef8b289ddd794ec",
    sha512 = "f1721f49518ee462a3d81def26d81037cd474b4254b85ad7c8f1509594d0177bbb996ee9625813852bacac108c2a72c83a8587050fec1dcda64730d6470953e6",
    ["sha3-224"] = "1e79576532395de20d67911b1f6b482bbcaaed8e6863d1e58082916e",
    ["sha3-384"] = "1719287be5dd0ce14ac51bfdc22f8e1137a6951666b2d946cd0d031bb68bc3173ba0e202bc0e6202f5d337061c6ec21f",
    ["sha3-512"] = "ad3a11a3430f0fac234a6c15bff0cb609b3d6fde0eac5873893e2775d15cf47d6791b4db907361c53719da248a7662f759b3f6c7ffb4fe69492a9449728cdad7",
}
for msg, tabela in HASH do
    for alg, esperado in tabela do
        caso("hash " .. alg .. " #" .. #msg, function()
            local h = E.crypt.hash(msg, alg)
            assert(h == esperado, h)
        end)
    end
end
caso("hash aceita SHA256 e sha3_256", function()
    assert(E.crypt.hash("test", "SHA256") == HASH.test.sha256)
    assert(E.crypt.hash("test", "sha3_256") == HASH.test["sha3-256"])
end)
caso("hash algoritmo inválido", function()
    assert(erro_de(E.crypt.hash, "x", "sha7"):find("unsupported algorithm"))
    assert(erro_de(E.crypt.hash, nil, "md5"):find("invalid argument #1"))
end)

-- crypt.encrypt/decrypt (openssl enc como referência) ----------------------------
local chave = E.base64.encode(string.char(table.unpack((function()
    local t = {}
    for i = 0, 31 do t[#t + 1] = i end
    return t
end)())))
local iv = E.base64.encode(string.char(table.unpack((function()
    local t = {}
    for i = 100, 115 do t[#t + 1] = i end
    return t
end)())))
local claro = "The quick brown fox jumps over the lazy dog!!"
local AES = {
    CBC = { "RgUcJojjtoiIyO6SdNV+vdNh44YKl4kbcDGayVQKi1eCYPEMg01/Jw1kL16Iok6V", "V45wWtkaABmQqVIjbZb0anmVgFFrn0/0/cy5rnQn74cF+k4UrGifvXNF0MdWzn1j", "+GWiaJ4f+c++Orl/f10+hROx05bidtqEp8v7lDbcjMxbOE26m7irnD/YzteK8bOJ" },
    ECB = { "9wIcAd5DyBR80kd6frpVs2mNwp9tsNXtpO7GgrM5Orsfmap7DVpaSU2GDOr30zAt", "G1UrCInENYVlGgRXr6kH8CHDpKlehQQtIwZxMRx0iy+Us9COJbPZlLE2hny01uOB", "vORkaeL3q2t+p2e9MlJSmvyEebjZbMF2ZOXwt2uVUOo6/Xj37hYJqpylmuHGis6O" },
    CTR = { "ytcLvMxphxtmJ3f6/pLquwrgGOc0VY2pWJFbfgWLny5rP0+9sYwSRS+5QEZ9", "5//3qU2OxZC23ZtMQjzpsk5Z6MjpC7jAiMyEAm/LoROvBFKY3slaSqBKtZtE", "x7B0AMdV0i4Vrd3Zicj+/NOG8yVqpnioAVb7LP/+weaopLo9t9PUeMYx365L" },
    CFB = { "ytcLvMxphxtmJ3f6/pLqu9NIux2izeqv1BShiS/h9bpQUwdLDlzKiBwFhTAb", "5//3qU2OxZC23ZtMQjzpsjZyT9OKBdS1tOFSZ+T063eBwKlQ80mZjWueyvuz", "x7B0AMdV0i4Vrd3Zicj+/O8FidtagzM/xhQX5ap9e2Z3ZwkxC4TjGdiJIt3+" },
    OFB = { "ytcLvMxphxtmJ3f6/pLqu8If3Hc4CJiS84o8VE1haPzESHU/0vd7XRWF2GHm", "5//3qU2OxZC23ZtMQjzpsoJBaPlUzCBbg6EGUAkTTAtgHyCLsAMptnOGeUTF", "x7B0AMdV0i4Vrd3Zicj+/IpInuP5Vh3Jva9BBnNIIvxdyn+1gLvoUf7qeXng" },
}
for modo, lista in AES do
    for n, esperado in lista do
        local bits = ({ 128, 192, 256 })[n]
        caso("aes " .. modo .. " " .. bits, function()
            local k = E.base64.encode(E.base64.decode(chave):sub(1, bits / 8))
            local c, v = E.crypt.encrypt(claro, k, iv, modo)
            assert(c == esperado, c)
            assert(v == iv)
            assert(E.crypt.decrypt(c, k, iv, modo) == claro)
        end)
    end
end
caso("aes do teste da UNC (iv gerado)", function()
    local k = E.crypt.generatekey()
    local c, v = E.crypt.encrypt("test", k, nil, "CBC")
    assert(v and #E.base64.decode(v) == 16)
    assert(E.crypt.decrypt(c, k, v, "CBC") == "test")
end)
caso("aes do teste da UNC (iv de 32 bytes)", function()
    local k, v = E.crypt.generatekey(), E.crypt.generatekey()
    local c = E.crypt.encrypt("test", k, v, "CBC")
    assert(E.crypt.decrypt(c, k, v, "CBC") == "test")
end)
caso("aes erros", function()
    assert(erro_de(E.crypt.encrypt, "x", "curta", iv, "CBC"):find("invalid key size"))
    assert(erro_de(E.crypt.encrypt, "x", chave, iv, "GCM"):find("unsupported mode"))
    assert(erro_de(E.crypt.decrypt, "AAAAAAAAAAAAAAAAAAAAAA==", chave, iv, "CBC"):find("bad padding"))
end)
caso("generatebytes/base64", function()
    assert(#E.crypt.base64decode(E.crypt.generatebytes(37)) == 37)
    assert(E.crypt.base64encode("test") == "dGVzdA==" and E.base64_decode("dGVzdA==") == "test")
    assert(E.crypt.base64.encode == E.base64.encode)
end)

-- lz4 ------------------------------------------------------------------------------
caso("lz4 ida e volta", function()
    for _, s in { "", "a", "Hello, world!", string.rep("abc", 1000), longo, string.rep("x", 70000) .. "fim" } do
        local c = E.lz4compress(s)
        assert(E.lz4decompress(c, #s) == s, "#" .. #s)
    end
    assert(#E.lz4compress(string.rep("abc", 1000)) < 100, "não comprimiu")
end)
caso("lz4 blocos do lz4 de referência (lz4 -9)", function()
    assert(E.lz4decompress("\31\97\1\0\44\80\97\98\99\100\101") == string.rep("a", 64) .. "abcde")
    local hw = string.rep("Hello, world! Hello, world! Hello, world! 1234567890", 3)
    local bloco = "\239\72\101\108\108\111\44\32\119\111\114\108\100\33\32\14\0\9\175\49\50\51\52\53\54\55\56\57\48\52\0\80\80\54\55\56\57\48"
    assert(E.lz4decompress(bloco) == hw)
end)
caso("lz4 corrompido", function()
    assert(erro_de(E.lz4decompress, "\31a\9\0"):find("invalid"))
end)

-- closures ---------------------------------------------------------------------------
caso("newcclosure/iscclosure/islclosure", function()
    local function f(a, b) return a + b, "ok" end
    local c = E.newcclosure(f)
    assert(c ~= f and E.iscclosure(c) and not E.islclosure(c))
    local x, y = c(2, 3)
    assert(x == 5 and y == "ok")
    assert(c(10, 1) == 11, "segunda chamada")
    assert(E.iscclosure(print) and E.islclosure(f))
    assert(E.isexecutorclosure(c) and E.isexecutorclosure(f) and not E.isexecutorclosure(print))
    assert(erro_de(E.iscclosure, 1):find("function expected"))
end)
caso("clonefunction", function()
    local function f() return "s" end
    local g = E.clonefunction(f)
    assert(g ~= f and g() == "s")
    assert(E.iscclosure(E.clonefunction(print)))
end)

-- arquivos -------------------------------------------------------------------------
caso("arquivos com token e loadfile", function()
    impressos = {}
    print = function(s) impressos[#impressos + 1] = s end
    E.writefile("a/x.lua", "return ... + 1")
    E.setclipboard("oi")
    assert(impressos[1]:match("^LUARIA_FS w tk%x+ 1/1 "), impressos[1])
    assert(impressos[2] == "LUARIA_CLIP tk " .. E.base64.encode("oi"), impressos[2])
    assert(E.isfolder("a") and E.readfile("a/x.lua") == "return ... + 1")
    assert(E.loadfile("a/x.lua")(1) == 2)
    E.writefile("cinco.lua", "return 5")
    assert(E.dofile("cinco.lua") == 5)
    print = print_real
    local f, e = E.loadfile("nao/existe.lua")
    assert(f == nil and e:find("does not exist"))
    assert(erro_de(E.writefile, "../fora", "x"):find("invalid path"))
end)

-- Drawing (só a lógica; a GUI é dublê) -------------------------------------------
caso("Drawing", function()
    local q = E.Drawing.new("Square")
    assert(E.isrenderobj(q) and not E.isrenderobj(newproxy()) and not E.isrenderobj({}))
    q.Visible = true
    q.Size = Vector2.new(10, 20)
    assert(E.getrenderproperty(q, "Visible") == true)
    E.setrenderproperty(q, "Visible", false)
    assert(q.Visible == false and q.Size.Y == 20)
    assert(erro_de(function() q.Nada = 1 end):find("not a valid member"))
    assert(erro_de(function() q.Visible = 1 end):find("boolean expected"))
    local l = E.Drawing.new("Line")
    l.From, l.To, l.Thickness = Vector2.new(0, 0), Vector2.new(3, 4), 2
    assert(E.Drawing.Fonts.Monospace == 3)
    local t = E.Drawing.new("Triangle")
    t.PointB = Vector2.new(5, 5)
    q:Destroy()
    assert(q.__OBJECT_EXISTS == false)
    E.cleardrawcache()
    assert(l.__OBJECT_EXISTS == false and t.__OBJECT_EXISTS == false)
    assert(erro_de(E.Drawing.new, "Hexagon"):find("invalid drawing type"))
    for _, i in criadas do
        if i.ClassName == "Frame" then assert(i.destruida, "frame vazou") end
    end
end)

-- ponte de globais (a linha que o host gera em prelude.rs::ponte_globais) -------
caso("getgenv liga com global de verdade", function()
    __luaria_ponte(function(k) if k == "__TESTE_G" then return __TESTE_G end end,
        function(k, v) if k == "__TESTE_G" then __TESTE_G = v return true end return false end)
    E.getgenv().__TESTE_G = 42
    assert(__TESTE_G == 42, "global não mudou")
    __TESTE_G = 7
    assert(E.getgenv().__TESTE_G == 7, "getgenv não viu o global")
    E.getgenv().solta = 1
    assert(rawget(E.getgenv(), "solta") == 1)
    assert(E.loadstring("return __TESTE_G")() == 7, "loadstring não viu o global")
end)

-- metatable crua e readonly (nativos) ------------------------------------------------
caso("getrawmetatable", function()
    local mt = {}
    local o = {}; mts[o] = mt
    assert(E.getrawmetatable(o) == mt)
    assert(E.getrawmetatable({}) == nil)
    assert(erro_de(E.getrawmetatable, 5):find("table or userdata expected"))
end)
caso("setrawmetatable devolve o objeto e aceita nil", function()
    local o, mt = {}, {}
    assert(E.setrawmetatable(o, mt) == o and mts[o] == mt)
    E.setrawmetatable(o, nil)
    assert(mts[o] == nil)
    assert(erro_de(E.setrawmetatable, 1, {}):find("table or userdata expected"))
    assert(erro_de(E.setrawmetatable, {}, 1):find("invalid argument #2"))
end)
caso("setrawmetatable em congelada re-trava", function()
    local o = {}; congeladas[o] = true
    E.setrawmetatable(o, {})
    assert(congeladas[o] == true)
end)
caso("setreadonly valida tipos", function()
    local t = {}
    E.setreadonly(t, true); assert(E.isreadonly(t))
    E.setreadonly(t, false); assert(not E.isreadonly(t))
    assert(erro_de(E.setreadonly, t, 1):find("boolean expected"))
    assert(erro_de(E.setreadonly, 1, true):find("table expected"))
end)

caso("getgc entrega lista e respeita o boolean", function()
    assert(type(E.getgc) == "function")
    assert(#E.getgc() == 1)
    assert(#E.getgc(true) == 2)
    assert(#E.getgc(1) == 1)
end)

-- constantes: o que não é nil/boolean/number/string/vector vira nil (UNC espera isso no import)
caso("getconstant e getconstants escondem tabela e função", function()
    local f = function() end
    consts[f] = { n = 5, "print", {}, nil, "x", print }
    assert(E.debug.getconstant(f, 1) == "print")
    assert(E.debug.getconstant(f, 2) == nil)
    assert(E.debug.getconstant(f, 3) == nil)
    assert(E.debug.getconstant(f, 4) == "x")
    assert(E.debug.getconstant(f, 5) == nil)
    assert(erro_de(E.debug.getconstant, f, 6):find("index out of range"))
    local lista = E.debug.getconstants(f)
    assert(lista[1] == "print" and lista[2] == nil and lista[3] == nil and lista[4] == "x" and lista[5] == nil)
end)

caso("setconstant troca do mesmo tipo e recusa tipo diferente", function()
    local f = function() end
    consts[f] = { n = 2, "antes", 1 }
    E.debug.setconstant(f, 1, "depois")
    assert(consts[f][1] == "depois")
    local e = erro_de(E.debug.setconstant, f, 2, "x")
    assert(e and e:find("type incompatible with the constant"), e)
    assert(consts[f][2] == 1)
    assert(erro_de(E.debug.setconstant, f, 3, 1):find("index out of range"))
    assert(erro_de(E.debug.setconstant, f, 0, "x"):find("index out of range"))
    assert(erro_de(E.debug.setconstant, 1, 1, "x"):find("function expected"))
end)

caso("setupvalue passa pelo nativo e erra no indice fora da faixa", function()
    local g = function() end
    upv[g] = 1
    E.debug.setupvalue(g, 1, 7)
    assert(upv[g] == 7)
    assert(erro_de(E.debug.setupvalue, g, 2, 7):find("index out of range"))
end)

-- instâncias sem pai e envs de script saem do GC --------------------------------------
caso("getnilinstances pega só Parent nil e não pega game", function()
    local solta = Instance.new("Part")
    local presa = Instance.new("Part")
    presa.Parent = game
    gc_extra = { solta, presa, game, 5 }
    local lista = E.getnilinstances()
    gc_extra = {}
    assert(#lista == 1 and lista[1] == solta, "achou " .. #lista)
    assert(lista[1].Parent == nil)
end)
caso("getinstances inclui instância sem pai", function()
    local solta = Instance.new("Folder")
    gc_extra = { solta }
    local achou = false
    for _, i in E.getinstances() do
        if i == solta then achou = true end
    end
    gc_extra = {}
    assert(achou, "getinstances não trouxe a instância sem pai")
end)
caso("getsenv, getrunningscripts e getloadedmodules leem env do GC", function()
    local ls, ms, cs = Instance.new("LocalScript"), Instance.new("ModuleScript"), Instance.new("CoreScript")
    local envLs = { script = ls, x = 1 }
    gc_extra = { envLs, { script = ms }, { script = cs }, { script = ls }, { outro = 1 } }
    assert(E.getsenv(ls) == envLs, "getsenv não achou o env do LocalScript")
    assert(E.getsenv(cs).script == cs, "getsenv não achou o CoreScript")
    assert(erro_de(E.getsenv, Instance.new("Script")):find("no environment"))
    assert(erro_de(E.getsenv, 1):find("Instance expected"))
    local rodando = E.getrunningscripts()
    assert(#rodando == 2 and rodando[1] == ls and rodando[2] == ms, "getrunningscripts: " .. #rodando)
    local modulos = E.getloadedmodules()
    assert(#modulos == 1 and modulos[1] == ms, "getloadedmodules: " .. #modulos)
    gc_extra = {}
end)

-- hookmetamethod: metatable real, com readonly restaurado ------------------------------
caso("hookmetamethod troca na metatable e devolve o antigo", function()
    local antigo = function() return false end
    local o = {}
    local mt = { __index = antigo }
    mts[o] = mt
    local novo = function() return true end
    assert(E.hookmetamethod(o, "__index", novo) == antigo)
    assert(mt.__index == novo and congeladas[mt] ~= true)
end)
caso("hookmetamethod em metatable congelada re-trava", function()
    local o, mt = {}, { __index = 1 }
    mts[o] = mt
    congeladas[mt] = true
    E.hookmetamethod(o, "__index", function() end)
    assert(congeladas[mt] == true and mt.__index ~= 1)
end)
caso("hookmetamethod recusa userdata e objeto sem metatable", function()
    assert(erro_de(E.hookmetamethod, newproxy(), "__namecall", function() end):find("__namecall needs the native"))
    assert(erro_de(E.hookmetamethod, {}, "__index", function() end):find("has no metatable"))
    assert(erro_de(E.hookmetamethod, {}, 1, function() end):find("invalid argument #2"))
end)

-- setfpscap: escreve e lê de volta no FramerateCap
caso("setfpscap escreve FramerateCap", function()
    E.setfpscap(60)
    assert(ugs.FramerateCap == 60)
    E.setfpscap(0)
    assert(ugs.FramerateCap == 0)
    assert(erro_de(E.setfpscap, "60"):find("number expected"))
end)

-- request (canal do host) ------------------------------------------------------------
-- decodificador independente do codec do prelude: o teste não reusa o que testa
local function campos_de(s)
    local campos, i = {}, 1
    while i <= #s do
        local dois = s:find(":", i, true)
        local len = tonumber(s:sub(i, dois - 1))
        campos[#campos + 1] = s:sub(dois + 1, dois + len)
        i = dois + len + 1
    end
    return campos
end
local function codifica(campos)
    local s = {}
    for _, c in campos do
        s[#s + 1] = #c .. ":" .. c
    end
    return table.concat(s)
end
-- resposta do host: mensagem, n, (nome, valor)*, corpo
local function resposta(mensagem, pares, corpo)
    local campos = { mensagem, tostring(#pares // 2) }
    for _, c in pares do
        campos[#campos + 1] = c
    end
    campos[#campos + 1] = corpo
    return codifica(campos)
end
local function ultimo_envio()
    return canal.enviados[#canal.enviados]
end

caso("request: pedido ok e formato enviado", function()
    canal.pe = function() return 8 end
    canal.pr = function() return 1, 200, resposta("OK", { "Content-Type", "text/plain" }, "corpo:com:dois\n") end
    local r = E.request({ Url = "https://a.teste/x", Method = "post", Headers = { ["X-A"] = "1" }, Body = "c:orpo" })
    assert(r.Success == true and r.StatusCode == 200 and r.StatusMessage == "OK")
    assert(r.Headers["Content-Type"] == "text/plain")
    assert(r.Body == "corpo:com:dois\n", r.Body)
    local envio = ultimo_envio()
    assert(envio.tipo == 1, "tipo")
    local enviado = campos_de(envio.payload)
    assert(#enviado == 6, "campos: " .. #enviado)
    assert(enviado[1] == "POST" and enviado[2] == "https://a.teste/x" and enviado[3] == "1")
    assert(enviado[4] == "X-A" and enviado[5] == "1" and enviado[6] == "c:orpo")
end)
caso("request: codec ida e volta com dígitos e ':' no corpo", function()
    canal.pe = function() return 8 end
    canal.pr = function() return 1, 201, resposta("Created", {}, "3:abc") end
    local r = E.request({ Url = "http://a.teste/", Body = "3:abc" })
    assert(r.StatusCode == 201 and r.Body == "3:abc", r.Body)
    assert(campos_de(ultimo_envio().payload)[4] == "3:abc")
end)
caso("request: pendente depois pronto", function()
    local voltas = 0
    canal.pe = function() return 16 end
    canal.pr = function()
        voltas += 1
        if voltas < 4 then
            return 0, voltas
        end
        return 1, 404, resposta("Not Found", {}, "")
    end
    local r = E.request({ Url = "https://a.teste/" })
    assert(voltas == 4 and r.StatusCode == 404 and not r.Success)
    assert(r.Body == "" and next(r.Headers) == nil)
end)
caso("request: pulso parado aborta como host não atende", function()
    canal.pe = function() return 24 end
    canal.pr = function() return 0, 5 end
    canal.cancelados = 0
    local e = erro_de(E.request, { Url = "https://a.teste/" })
    assert(e and e:find("host not responding"), e)
    assert(canal.cancelados == 1, "não cancelou")
end)
caso("request: pulso andando estoura o prazo total", function()
    local pulso = 0
    canal.pr = function()
        pulso += 1
        return 0, pulso
    end
    canal.cancelados = 0
    local e = erro_de(E.request, { Url = "https://a.teste/" })
    assert(e and e:find("timed out"), e)
    assert(canal.cancelados == 1, "não cancelou")
end)
caso("request: pedido perdido", function()
    canal.pr = function() return 2 end
    local e = erro_de(E.request, { Url = "https://a.teste/" })
    assert(e and e:find("request lost"), e)
end)
caso("request: erros de pe (-1 a -4)", function()
    local esperado = {
        [-1] = "channel full",
        [-2] = "request too large",
        [-3] = "bridge unavailable",
        [-4] = "invalid argument",
    }
    for h, msg in esperado do
        canal.pe = function() return h end
        local e = erro_de(E.request, { Url = "https://a.teste/" })
        assert(e and e:find(msg, 1, true), msg .. " / " .. tostring(e))
    end
end)
caso("request: status 0 vira erro com a mensagem do host", function()
    canal.pe = function() return 8 end
    canal.pr = function() return 1, 0, "destination refused" end
    local e = erro_de(E.request, { Url = "http://127.0.0.1:1/" })
    assert(e and e:find("request failed: destination refused", 1, true), e)
end)
caso("request: contagem de headers que não bate", function()
    canal.pr = function() return 1, 200, codifica({ "OK", "2", "A", "1", "corpo" }) end
    local e = erro_de(E.request, { Url = "https://a.teste/" })
    assert(e and e:find("invalid response"), e)
end)
caso("request: opts inválidos recusados antes do canal", function()
    local antes = #canal.enviados
    local ruins = {
        {},
        { Url = "file:///etc/passwd" },
        { Url = 5 },
        { Url = "https://a.teste/", Method = "TRACE" },
        { Url = "https://a.teste/", Method = 9 },
        { Url = "https://a.teste/", Headers = { X = 1 } },
        { Url = "https://a.teste/", Headers = { X = "1", [1] = "y" } },
        { Url = "https://a.teste/", Body = 9 },
    }
    for _, opts in ruins do
        local e = erro_de(E.request, opts)
        assert(e and e:find("invalid argument #1 to 'request'", 1, true), tostring(e))
    end
    assert(erro_de(E.request, nil):find("invalid argument #1 to 'request'"))
    assert(#canal.enviados == antes, "mandou pedido inválido")
end)
caso("request: 65 headers recusados", function()
    local h = {}
    for i = 1, 65 do
        h["X-" .. i] = "1"
    end
    local e = erro_de(E.request, { Url = "https://a.teste/", Headers = h })
    assert(e and e:find("too many headers"), e)
end)
caso("http_request e http.request apontam pro mesmo", function()
    assert(E.http_request == E.request and E.http.request == E.request)
    assert(E.isexecutorclosure(E.request))
end)
-- fora de coroutine não dá pra ceder: espera ocupada com teto de 3 s
total += 1
canal.pe = function() return 8 end
canal.pr = function() return 0, relogio end
local ok_sem_coro, e_sem_coro = pcall(E.request, { Url = "https://a.teste/" })
if ok_sem_coro or not tostring(e_sem_coro):find("timed out", 1, true) then
    falhas += 1
    print_real("FALHOU request fora de coroutine: " .. tostring(e_sem_coro))
end

print_real(string.format("OK %d/%d", total - falhas, total))
