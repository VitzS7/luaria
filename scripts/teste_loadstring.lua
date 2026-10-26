-- Teste do interpretador de assets/loadstring.lua FORA do jogo, no REPL do Luau
-- real (tools/luau.sh; binário em ~/.cache/luaria/luau-src/build/release/luau).
-- Compara cada trecho rodando nativo (loadstring do REPL) e interpretado.
--
-- Uso (a linha do marcador recebe o interpretador, sem tocar no repo):
--   sed '/^--@@LOADSTRING@@/r assets/loadstring.lua' scripts/teste_loadstring.lua > /tmp/x.lua
--   luau /tmp/x.lua

local env = {}
local function publicar(nome, valor) env[nome] = valor end
local function marcar(f) return f end
local nativo = loadstring
--@@LOADSTRING@@
local interp = env.loadstring

local function ser(...)
    local n = select("#", ...)
    local t = table.create(n)
    for i = 1, n do
        local v = select(i, ...)
        if type(v) == "table" then
            local ks = {}
            for k in v do ks[#ks + 1] = k end
            table.sort(ks, function(a, b) return tostring(a) < tostring(b) end)
            local p = {}
            for _, k in ks do p[#p + 1] = tostring(k) .. "=" .. ser(v[k]) end
            t[i] = "{" .. table.concat(p, ",") .. "}"
        elseif type(v) == "function" then
            t[i] = "fn"
        else
            t[i] = typeof(v) .. ":" .. tostring(v)
        end
    end
    return n .. "|" .. table.concat(t, ";")
end

local casos = {
{"fatorial", [[local function f(n) if n <= 1 then return 1 end return n * f(n - 1) end return f(10), f(20)]]},
{"fib", [[local function fib(n) if n < 2 then return n end return fib(n-1) + fib(n-2) end return fib(15)]]},
{"closures em loop", [[local fs = {} for i = 1, 5 do fs[i] = function() return i end end local r = {} for i, f in fs do r[i] = f() end return r]]},
{"closure contador", [[local function mk() local c = 0 return function() c += 1 return c end end local a, b = mk(), mk() a() a() return a(), b()]]},
{"upvalue compartilhado", [[local x = 0 local function inc() x += 1 end local function get() return x end inc() inc() return get(), x]]},
{"upvalue 2 niveis", [[local x = 1 local function a() local function b() x = x + 10 return x end return b end return a()(), x]]},
{"while closure", [[local fs, i = {}, 0 while i < 3 do i += 1 local j = i fs[#fs+1] = function() j += 1 return j end end return fs[1](), fs[1](), fs[2](), fs[3]()]]},
{"varargs", [[local function f(...) return select('#', ...), ... end return f(1, nil, 3, nil)]]},
{"varargs tabela", [[local function f(...) local t = {...} return #t, select('#', ...) end return f(1, 2, 3)]]},
{"varargs com fixos", [[local function f(a, b, ...) return a, b, select('#', ...), ... end return f(1), f(1, 2, 3, 4)]]},
{"multiplos retornos", [[local function f() return 1, 2, 3 end local a, b, c, d = f() local t = {f(), f()} return a, b, c, d, #t, (f())]]},
{"retorno vazio", [[local function f() return end return select('#', f()), select('#', (f()))]]},
{"tail call", [[local function f(n, acc) if n == 0 then return acc end return f(n - 1, acc + n) end return f(5000, 0)]]},
{"metatable index", [[local t = setmetatable({}, {__index = function(_, k) return k .. "!" end}) return t.a, t.b, rawget(t, "a")]]},
{"metatable call", [[local t = setmetatable({}, {__call = function(self, a, b) return a + b end}) return t(2, 3)]]},
{"metatable concat/eq/lt", [[local mt = {} mt.__concat = function(a, b) return "C" end mt.__eq = function() return true end mt.__lt = function() return true end mt.__le = function() return false end mt.__len = function() return 42 end
local a, b = setmetatable({}, mt), setmetatable({}, mt) return a .. b, a == b, a < b, a <= b, #a, a .. "x", "x" .. a]]},
{"metatable aritmetica", [[local mt = {__add = function(a, b) return "add" end, __unm = function() return "unm" end, __idiv = function() return "idiv" end, __mod = function() return "mod" end}
local a = setmetatable({}, mt) return a + 1, 1 + a, -a, a // 2, a % 3]]},
{"newindex", [[local log = {} local t = setmetatable({}, {__newindex = function(t, k, v) log[#log+1] = k rawset(t, k, v) end}) t.a = 1 t.b = 2 t.a = 3 return log, t.a]]},
{"heranca", [[local A = {} A.__index = A function A.new(x) return setmetatable({x = x}, A) end function A:get() return self.x end
local B = setmetatable({}, {__index = A}) B.__index = B function B.new(x) local o = A.new(x) return setmetatable(o, B) end function B:dobro() return self:get() * 2 end
local b = B.new(21) return b:get(), b:dobro()]]},
{"metodo em campo", [[local o = {n = {v = 5}} function o.n.f(x) return x + 1 end function o.n:g(x) return self.v + x end return o.n.f(1), o.n:g(10)]]},
{"gsub/format", [[return ("hello world"):gsub("o", "0"), string.format("%5.2f|%d|%s|%q", 3.14159, 42, "x", "a\nb"), ("abc"):upper():rep(2)]]},
{"string metodos", [[local s = "a,b,c" local r = {} for p in s:gmatch("[^,]+") do r[#r+1] = p end return r, s:find(","), s:sub(2, 3), #s]]},
{"coroutine basica", [[local co = coroutine.create(function(a) local b = coroutine.yield(a + 1) local c = coroutine.yield(b * 2) return "fim", c end)
local r = {} r[1] = {coroutine.resume(co, 1)} r[2] = {coroutine.resume(co, 10)} r[3] = {coroutine.resume(co, "z")} r[4] = coroutine.status(co) return r]]},
{"coroutine wrap loop", [[local gen = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end) local r = {} for i = 1, 3 do r[i] = gen() end return r]]},
{"yield em funcao aninhada", [[local function f(x) coroutine.yield(x) return x * 2 end local co = coroutine.wrap(function() local a = f(1) local b = f(a) return a + b end) return co(), co(), co()]]},
{"iteracao generalizada", [[local t = {10, 20, 30, a = 1} local s, n = 0, 0 for k, v in t do s += v n += 1 end return s, n]]},
{"pairs ipairs next", [[local t = {5, 6, x = 7} local a, b, c = 0, 0, 0 for _, v in ipairs(t) do a += v end for _, v in pairs(t) do b += v end for k, v in next, t do c += v end return a, b, c]]},
{"__iter", [[local t = setmetatable({}, {__iter = function() local i = 0 return function() i += 1 if i <= 3 then return i, i * i end end end}) local r = {} for k, v in t do r[#r+1] = k + v end return r]]},
{"for numerico", [[local r = {} for i = 10, 1, -3 do r[#r+1] = i end for i = 1, 3 do r[#r+1] = i end for i = 1, 0 do r[#r+1] = 99 end for i = 1.5, 3.5 do r[#r+1] = i end return r]]},
{"break/continue", [[local r = {} for i = 1, 10 do if i % 2 == 0 then continue end if i > 7 then break end r[#r+1] = i end
local j = 0 while true do j += 1 if j == 3 then continue end if j > 5 then break end r[#r+1] = -j end return r]]},
{"continue aninhado", [[local r = 0 for i = 1, 3 do for j = 1, 3 do if j == 2 then continue end r += i * 10 + j end end return r]]},
{"interpolacao", [[local n, s = 5, "ab" return `x{n}y{s}z`, `{n + 1}`, `{({1,2})[2]}`, `a\{b`, `{string.format("%d", 7)}`, ``]]},
{"interpolacao aninhada", [[local t = {a = 1} return `v={t.a} {`in{t.a}`}`]]},
{"if expression", [[local function f(x) return if x < 0 then "neg" elseif x == 0 then "zero" else "pos" end return f(-1), f(0), f(3), (if false then 1 else 2) + 1]]},
{"compound assign", [=[local a = 10 a += 5 a -= 3 a *= 2 a /= 4 a //= 2 a %= 4 a ^= 3 local s = "x" s ..= "y" local t = {n = 1, [2] = 5} t.n += 1 t[2] *= 3 return a, s, t.n, t[2]]=]},
{"compound global", [[g1 = 1 g1 += 4 g1 ..= "!" return g1]]},
{"repeat until escopo", [[local i = 0 repeat local x = i * 2 i += 1 until x >= 6 return i]]},
{"repeat continue", [[local i, r = 0, 0 repeat i += 1 if i == 2 then continue end r += i until i >= 4 return r]]},
{"strings escapes", [[return "a\tb\\c\"d\'e\65\066\x41\u{48}\u{20AC}z\z
      w", 'q\'', "l1\
l2", #"\0a"]]},
{"strings longas", "return [[a\nb]], [==[x]]y]==], [[\nfoo]], #[=[\n\n]=]"},
{"comentarios", "local a = 1 --[[ c\nmulti ]] + 2 --[==[ x\n]==] + 3 -- fim\nreturn a"},
{"numeros", [[return 0xFF, 0b1010, 1_000_000, 0x_1F, 1e3, 1.5e-2, .5, 3., 0xff_ff, 0b1_1, 1E2, 2^10]]},
{"floor div e mod", [[return 7 // 2, -7 // 2, 7 // -2, 7.5 // 2, 7 % 3, -7 % 3, 7 % -3, 5.5 % 2, 2^0.5 > 1.41]]},
{"precedencia", [[return 2 + 3 * 4, (2 + 3) * 4, 2 ^ 3 ^ 2, -2 ^ 2, not nil == true, 1 .. 2 .. 3, "a" .. 1 + 2, 1 < 2 == true, #"abc" + 1, - - 3, not not nil]]},
{"and/or", [[local a, b = nil, false return a or "d", b or "e", 1 and 2, nil and 1, false and nil, a and a.x, 0 or 5, "" and "s", nil or false, false or nil]]},
{"curto circuito", [[local n = 0 local function f() n += 1 return true end local _ = false and f() _ = true or f() _ = nil and f() _ = 1 and f() return n]]},
{"comparacoes", [[return 1 == 1.0, "a" < "b", "a" <= "a", 2 >= 3, "10" == 10, 1 ~= 2, nil == false, {} == {}, "abc" < "abd"]]},
{"atribuicao multipla", [=[local a, b, c = 1, 2 a, b = b, a local t = {} t.x, t.y = 1, 2, 3 local i = 1 i, t[i] = i + 1, 20 return a, b, c, t.x, t.y, i, t[1]]=]},
{"swap em tabela", [[local t = {1, 2, 3} t[1], t[3] = t[3], t[1] return t]]},
{"construtor tabela", [[local function f() return 7, 8 end local t = {1, 2, x = 3, [10] = 4, f()} local u = {f(), f()} local v = {(f())} return t, u, v, #{n = 1}, #{nil, nil, 3}]]},
{"construtor vararg", [[local function f(...) return {...}, {n = select('#', ...), ...}, {..., 0} end return f(1, 2)]]},
{"chamada string/tabela", [=[local function f(x) return type(x) == "table" and #x or x end return f"txt", f{1, 2, 3}, f[[long]], ("x"):rep(3)]=]},
{"upvalue em for-in", [[local fs = {} for k, v in {a = 1, b = 2} do fs[#fs+1] = function() return k .. v end end table.sort(fs, function(x, y) return x() < y() end) return fs[1](), fs[2]()]]},
{"recursao mutua", [[local isodd local function iseven(n) if n == 0 then return true end return isodd(n - 1) end function isodd(n) if n == 0 then return false end return iseven(n - 1) end return iseven(10), isodd(7)]]},
{"funcao global", [[function gf(x) return x * 2 end local r = gf(4) gv = 9 return r, gv, gf == nil]]},
{"global entre chunks", [[return sharedval]]},
{"tipos ignorados", [[type Pt = {x: number, y: number?, [string]: any} type Fn<T, U...> = (a: T, U...) -> (T, number) export type Un = "a" | "b" | nil
local function f<T>(a: T, b: number?, ...: string): (T, number) return a, b or 0 end
local x: number, y: string = 1, "s" local z = (x :: number) + 1 local w = f(5) :: any
local t: {number} = {1, 2} local g: (number) -> number = function(n: number): number return n + 1 end
for i: number = 1, 2 do x += i end local u: typeof(x) | string = x return x, y, z, w, #t, g(1), u]]},
{"tipo generico em campo", [[type A<T> = {v: T} type B = A<number> & {k: string} local o: B = {v = 1, k = "x"} return o.v, o.k]]},
{"atributo native", [[@native local function f(x: number): number return x * 2 end return f(4)]]},
{"continue como identificador", [[local continue = 5 continue = continue + 1 return continue]]},
{"type como identificador", [[local type = type return type(1), typeof(nil)]]},
{"upvalue param", [[local function mk(n) return function() n += 1 return n end end local f = mk(5) f() return f(), mk(0)()]]},
{"param capturado em vararg", [[local function mk(a, ...) local n = select('#', ...) return function() a += 1 return a, n end end local f = mk(1, 2, 3) f() return f()]]},
{"error com nivel", [[local ok, e = pcall(function() error("boom") end) local ok2, e2 = pcall(function() error("sem pos", 0) end) local ok3, e3 = pcall(function() error({1}) end) return ok, e, ok2, e2, ok3, type(e3)]]},
{"erro de chamada nil", [[local ok, e = pcall(function() local t = {} t.foo() end) return ok, e:find("attempt to call a nil value", 1, true) ~= nil]]},
{"erro aritmetica", [[local ok, e = pcall(function() return nil + 1 end) return ok, type(e)]]},
{"pcall/error propaga valores", [[return pcall(error, "x", 0), select('#', pcall(function() return 1, 2, 3 end)), pcall(function(...) return ... end, 7, 8)]]},
{"assert", [[local ok, e = pcall(function() assert(false, "msg") end) return ok, e, assert(1, "x")]]},
{"ambiente genv", [[x_novo = 1 local a = x_novo local b = getgenv and 1 return a, math.floor(2.5), string.len("abc"), type(print), type(nonexistent)]]},
{"loadstring aninhado", [[local f = loadstring("return ... + 1") return f(41), loadstring("x = ") == nil]]},
{"sort com comparador interpretado", [[local t = {5, 2, 8, 1} table.sort(t, function(a, b) return a > b end) return t, table.concat(t, ",")]]},
{"select negativo e unpack", [[return select(-1, 1, 2, 3), select(2, "a", "b", "c"), unpack({1, 2, 3}), table.pack(1, nil, 3).n]]},
{"shadowing", [[local x = 1 local function f() local x = x + 1 return x end local y = x do local x = 100 y += x end return f(), x, y]]},
{"local nao inicializado em loop", [[local r = {} for i = 1, 3 do local v if i == 1 then v = "a" end r[i] = v end return r, #r]]},
{"cadeia de else", [[local function f(x) if x == 1 then return "a" elseif x == 2 then return "b" elseif x == 3 then return "c" else return "d" end end return f(1), f(2), f(3), f(4)]]},
{"return em loop aninhado", [[local function f() for i = 1, 3 do for j = 1, 3 do if i * j == 4 then return i, j end end end return 0 end return f()]]},
{"table.insert/remove", [[local t = {} for i = 1, 5 do table.insert(t, i * i) end table.remove(t, 1) return t, #t]]},
{"profundidade", [[local function d(n) if n == 0 then return 0 end return 1 + d(n - 1) end return d(1500)]]},
{"string index em var", [[local s = "abc" return s:len(), s.len, ("x").y]]},
}

-- erros de sintaxe: ambos devem devolver nil + mensagem com a mesma linha
local invalidos = {
    "x = ", "local = 1", "if x then", "for i = 1 do end", "return return", "x = = 1", "local function () end",
    "a.b:c = 1", "f(", "x = {1, 2", "\n\nlocal x = 'abc", "x = [[abc", "x = 1 +", "while true do", "local x <const> = 1",
    "x = `abc{1", "return ...", "function f() return ... end", "local a = 0x", "x = 3 4", "end",
}

local ok, falhas, total = 0, {}, 0
local function registrar(nome, bom, detalhe)
    total += 1
    if bom then ok += 1 else falhas[#falhas + 1] = nome .. "\n    " .. tostring(detalhe) end
end

-- sharedval é global do código interpretado; o nativo enxerga o mesmo valor por _G
interp("sharedval = 'compartilhado'")()
local genv = env.getgenv()
casos[#casos + 1] = nil

for _, c in casos do
    local nome, src = c[1], c[2]
    local fn = nativo(src, "t")
    local fi, msg = interp(src, "t")
    if not fi then
        registrar(nome, false, "interp nao compilou: " .. tostring(msg))
    else
        -- globais do nativo vão em _G; igualamos os do interpretado antes de comparar
        local rn = table.pack(pcall(fn))
        local ri = table.pack(pcall(fi))
        local sn, si = ser(table.unpack(rn, 1, rn.n)), ser(table.unpack(ri, 1, ri.n))
        if nome == "global entre chunks" then
            registrar(nome, si == "2|boolean:true;string:compartilhado", si)
        else
            registrar(nome, sn == si, "nativo   " .. sn .. "\n    interp   " .. si)
        end
    end
end

for _, src in invalidos do
    local fn, msgN = nativo(src, "t")
    local fi, msgI = interp(src, "t")
    local linhaN = fn == nil and tonumber(string.match(msgN, '^%[string "t"%]:(%d+):'))
    local linhaI = fi == nil and type(msgI) == "string" and tonumber(string.match(msgI, '^%[string "t"%]:(%d+):'))
    local bom = (fn == nil) == (fi == nil) and (fn ~= nil or linhaN == linhaI)
    registrar("sintaxe: " .. src:gsub("\n", "\\n"), bom, "nativo " .. tostring(msgN) .. "\n    interp " .. tostring(msgI))
end

-- chunkname: "=nome" cru, sem nome usa a primeira linha
do
    local _, m1 = interp("x = =", "=meu")
    local _, m2 = interp("x = =")
    registrar("chunkname =", string.match(m1, "^meu:1:") ~= nil, m1)
    registrar("chunkname default", string.match(m2, '^%[string "x = ="%]:1:') ~= nil, m2)
end

-- genv único: global criada num chunk aparece em getgenv() e noutro chunk
do
    interp("zz_global = 7")()
    registrar("getgenv", genv.zz_global == 7 and interp("return zz_global")() == 7, tostring(genv.zz_global))
end

for _, f in falhas do print("FALHA " .. f) end
print(("OK %d/%d"):format(ok, total))

-- Benchmark: nativo x interpretado ------------------------------------------
local function medir(nome, fonte)
    local fn, fi = nativo(fonte, "b"), interp(fonte, "b")
    local t0 = os.clock()
    local rn = fn()
    local tn = os.clock() - t0
    t0 = os.clock()
    local ri = fi()
    local ti = os.clock() - t0
    print(("%-14s nativo %.3fs  interp %.3fs  fator %.0fx  %s"):format(nome, tn, ti, ti / math.max(tn, 1e-9), tostring(rn == ri)))
end
medir("fib(27)", [[local function fib(n) if n < 2 then return n end return fib(n-1) + fib(n-2) end return fib(27)]])
medir("loop 1e6", [[local s = 0 for i = 1, 1000000 do s = s + i % 7 end return s]])
medir("concat 1e4", [[local s = "" for i = 1, 10000 do s = s .. "x" end return #s]])
medir("tabela/metodo", [[local C = {} C.__index = C function C:inc() self.n += 1 end local o = setmetatable({n = 0}, C) for i = 1, 300000 do o:inc() end return o.n]])
