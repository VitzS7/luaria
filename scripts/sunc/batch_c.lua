-- Lote C: Instances, Scripts, Signals. Injetado em sunc_own.lua via --@@BATCH_C@@.

local Players = game:GetService('Players')

local function exigirExiste(nome)
	assert(exists(nome), 'does not exist')
end

local function contem(lista, valor)
	return table.find(lista, valor) ~= nil
end

-- Script de jogo ativo para os testes positivos; sem ele os positivos viram SKIP.
local function acharScriptDoJogo()
	local jogador = Players.LocalPlayer
	if not jogador then return nil end
	for _, pasta in ipairs({ jogador:FindFirstChild('PlayerScripts'), jogador:FindFirstChild('PlayerGui') }) do
		for _, item in ipairs(pasta and pasta:GetDescendants() or {}) do
			if item:IsA('LocalScript') then return item end
		end
	end
	return nil
end

local scriptJogo = acharScriptDoJogo()

local function exigirScriptJogo(nome)
	if scriptJogo then return true end
	SKIP('needs a game script')
	return false
end

-- Instances

T('cloneref', function()
	exigirExiste('cloneref')
	local clone = cloneref(game)
	assert(clone ~= game, 'clone is the same reference')
	eq(clone.Name, game.Name, 'Name')
end)

T('compareinstances', function()
	exigirExiste('compareinstances')
	eq(compareinstances(game, game), true, 'game vs game')
	eq(compareinstances(game, workspace), false, 'game vs workspace')
	eq(compareinstances(game, cloneref(game)), true, 'game vs cloneref(game)')
end)

T('fireclickdetector', function()
	exigirExiste('fireclickdetector')
	local parte = Instance.new('Part')
	parte.Parent = workspace
	local detector = Instance.new('ClickDetector', parte)
	local eventos = { 'MouseClick', 'RightMouseClick', 'MouseHoverEnter', 'MouseHoverLeave' }
	local contagem = {}
	local conexoes = {}
	for _, evento in ipairs(eventos) do
		contagem[evento] = 0
		table.insert(conexoes, detector[evento]:Connect(function()
			contagem[evento] += 1
		end))
	end
	for _, evento in ipairs(eventos) do
		fireclickdetector(detector, 0, evento)
	end
	task.wait()
	for _, evento in ipairs(eventos) do
		assert(contagem[evento] >= 1, evento .. ' did not fire')
	end
	for _, conexao in ipairs(conexoes) do conexao:Disconnect() end
	parte:Destroy()
end)

T('fireproximityprompt', function()
	exigirExiste('fireproximityprompt')
	local parte = Instance.new('Part')
	parte.Parent = workspace
	local prompt = Instance.new('ProximityPrompt', parte)
	local disparou = false
	local conexao = prompt.Triggered:Connect(function() disparou = true end)
	fireproximityprompt(prompt)
	-- sem task.wait: o contrato é disparar Triggered na hora da chamada
	assert(disparou, 'Triggered did not fire synchronously')
	conexao:Disconnect()
	parte:Destroy()
end)

T('firetouchinterest', function()
	exigirExiste('firetouchinterest')
	local p1, p2 = Instance.new('Part'), Instance.new('Part')
	p1.Parent, p2.Parent = workspace, workspace
	local tocou, saiu = false, false
	local c1 = p1.Touched:Connect(function() tocou = true end)
	local c2 = p1.TouchEnded:Connect(function() saiu = true end)
	-- toggle: true/0 = começou o toque, false/1 = terminou
	firetouchinterest(p1, p2, true)
	task.wait()
	assert(tocou, 'Touched did not fire (true)')
	firetouchinterest(p1, p2, false)
	task.wait()
	assert(saiu, 'TouchEnded did not fire (false)')
	tocou, saiu = false, false
	firetouchinterest(p1, p2, 0)
	task.wait()
	assert(tocou, 'Touched did not fire (0)')
	firetouchinterest(p1, p2, 1)
	task.wait()
	assert(saiu, 'TouchEnded did not fire (1)')
	c1:Disconnect()
	c2:Disconnect()
	p1:Destroy()
	p2:Destroy()
end)

T('getcallbackvalue', function()
	exigirExiste('getcallbackvalue')
	local bindable = Instance.new('BindableFunction')
	local callback = function() end
	bindable.OnInvoke = callback
	eq(getcallbackvalue(bindable, 'OnInvoke'), callback, 'OnInvoke')
	local remota = Instance.new('RemoteFunction')
	eq(getcallbackvalue(remota, 'OnClientInvoke'), nil, 'OnClientInvoke unset')
	eq(getcallbackvalue(bindable, 'PropriedadeInexistente'), nil, 'missing property')
end)

T('gethui', function()
	exigirExiste('gethui')
	local container = gethui()
	assert(typeof(container) == 'Instance', 'gethui did not return an Instance')
	local tela = Instance.new('ScreenGui')
	tela.Name = 'SuncGethuiTeste'
	tela.Parent = container
	assert(container:FindFirstChild(tela.Name), 'ScreenGui not found in gethui container')
	tela:Destroy()
end)

T('getinstances', function()
	exigirExiste('getinstances')
	local solta = Instance.new('Part')
	assert(contem(getinstances(), solta), 'Part without Parent missing from getinstances')
	solta:Destroy()
end)

T('getnilinstances', function()
	exigirExiste('getnilinstances')
	local solta = Instance.new('Part')
	local parentada = Instance.new('Part')
	parentada.Parent = workspace
	local lista = getnilinstances()
	assert(contem(lista, solta), 'Part without Parent missing from getnilinstances')
	assert(not contem(lista, parentada), 'Part parented to workspace listed in getnilinstances')
	solta:Destroy()
	parentada:Destroy()
end)

-- Scripts

T('getcallingscript', function()
	exigirExiste('getcallingscript')
	-- thread do executor não tem script chamador
	eq(getcallingscript(), nil, 'executor thread')
	SKIP('needs a game script to call from')
end)

T('getloadedmodules', function()
	exigirExiste('getloadedmodules')
	local usado = Instance.new('ModuleScript')
	usado.Source = 'return 42'
	usado.Parent = workspace
	local naoUsado = Instance.new('ModuleScript')
	naoUsado.Source = 'return 0'
	naoUsado.Parent = workspace
	eq(require(usado), 42, 'require result')
	local lista = getloadedmodules()
	assert(contem(lista, usado), 'required ModuleScript missing')
	assert(not contem(lista, naoUsado), 'never-required ModuleScript listed')
	usado:Destroy()
	naoUsado:Destroy()
end)

T('getrunningscripts', function()
	exigirExiste('getrunningscripts')
	if not exigirScriptJogo('getrunningscripts') then return end
	assert(contem(getrunningscripts(), scriptJogo), 'game LocalScript not listed')
end)

T('getscriptbytecode', function()
	exigirExiste('getscriptbytecode')
	eq(getscriptbytecode(Instance.new('LocalScript')), nil, 'fake script')
	if not exigirScriptJogo('getscriptbytecode') then return end
	local bytecode = getscriptbytecode(scriptJogo)
	assert(type(bytecode) == 'string' and #bytecode > 0, 'real script bytecode empty')
end)

T('getscriptclosure', function()
	exigirExiste('getscriptclosure')
	eq(getscriptclosure(Instance.new('LocalScript')), nil, 'fake script')
	if not exigirScriptJogo('getscriptclosure') then return end
	assert(type(getscriptclosure(scriptJogo)) == 'function', 'real script closure is not a function')
end)

T('getscriptfromthread', function()
	exigirExiste('getscriptfromthread')
	eq(getscriptfromthread(coroutine.running()), nil, 'executor thread')
end)

T('getscripthash', function()
	exigirExiste('getscripthash')
	eq(getscripthash(Instance.new('LocalScript')), nil, 'fake script without bytecode')
	if not exigirScriptJogo('getscripthash') then return end
	local hash = getscripthash(scriptJogo)
	assert(type(hash) == 'string' and #hash == 96 and hash:match('^%x+$'), 'hash is not 96 hex chars')
	eq(getscripthash(scriptJogo), hash, 'hash unstable between calls')
end)

T('getscripts', function()
	exigirExiste('getscripts')
	local ls = Instance.new('LocalScript')
	ls.Parent = workspace
	local lista = getscripts()
	assert(contem(lista, ls), 'created LocalScript missing from getscripts')
	for _, item in ipairs(lista) do
		assert(not item:IsA('CoreScript'), 'CoreScript listed in getscripts')
	end
	ls:Destroy()
end)

T('getsenv', function()
	exigirExiste('getsenv')
	expectError(getsenv, Instance.new('LocalScript'))
	if not exigirScriptJogo('getsenv') then return end
	assert(type(getsenv(scriptJogo)) == 'table', 'env is not a table')
end)

-- Signals

T('firesignal', function()
	exigirExiste('firesignal')
	local parte = Instance.new('Part')
	local chamadas = 0
	local recebido = 'unset'
	local conexao = parte.ChildAdded:Connect(function(argumento)
		chamadas += 1
		recebido = argumento
	end)
	firesignal(parte.ChildAdded)
	assert(chamadas == 1, 'connection not called')
	eq(recebido, nil, 'argument without explicit value')
	conexao:Disconnect()
	local alvo = Instance.new('Part')
	local recebidoWorkspace
	local conexaoWorkspace = workspace.ChildAdded:Connect(function(argumento)
		recebidoWorkspace = argumento
	end)
	firesignal(workspace.ChildAdded, alvo)
	eq(recebidoWorkspace, alvo, 'workspace.ChildAdded argument')
	conexaoWorkspace:Disconnect()
	parte:Destroy()
	alvo:Destroy()
end)

T('getconnections', function()
	exigirExiste('getconnections')
	local evento = Instance.new('BindableEvent')
	local chamadas = 0
	local funcao = function() chamadas += 1 end
	local conexao = evento.Event:Connect(funcao)
	local lista = getconnections(evento.Event)
	eq(lista[1].Function, funcao, 'Function')
	-- Thread pode ser nil quando a conexão não está rodando numa thread própria
	assert(lista[1].Thread == nil or type(lista[1].Thread) == 'thread', 'Thread has wrong type')
	lista[1]:Fire()
	eq(chamadas, 1, 'Fire call count')
	lista[1]:Disable()
	eq(lista[1].Enabled, false, 'Enabled after Disable')
	lista[1]:Enable()
	eq(lista[1].Enabled, true, 'Enabled after Enable')
	lista[1]:Disconnect()
	eq(lista[1].Connected, false, 'Connected after Disconnect')
	conexao:Disconnect()
	evento:Destroy()
end)

T('replicatesignal', function()
	exigirExiste('replicatesignal')
	-- aridade errada tem que lançar erro
	expectError(replicatesignal)
	SKIP('needs the official server')
end)
