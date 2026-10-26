// Execução de scripts e conexão com o cliente Roblox. O estado da conexão é
// sondado a cada 2 s; `executar` manda o código ao motor e lê o log de volta.

import { $, api, escapar, icone, postar, diaLocal } from "./util.js";
import { estado } from "./estado.js";
import { toast } from "./dialogo.js";
import { abaAtiva } from "./abas.js";
import { aplicarMarcas } from "./editor.js";
import { pollLog, renderProblemas, trocarPainel } from "./console.js";
import { carregarAtividade } from "./home.js";


const INTERVALO_STATUS_MS = 2000;
const INTERVALO_LOG_MS = 900;

const hexOuTraco = (valor) => (valor ? `0x${valor.toString(16)}` : "—");

/** Com vários alvos o chip só conta; com um, mostra o cliente ativo (lista do popover de clientes). */
function rotuloConexao(s) {
  const clientes = estado.clientes || [];
  const alvos = clientes.filter((c) => c.alvo).length;
  if (alvos > 1) return `${alvos} alvos`;
  const ativo = clientes.find((c) => c.ativo);
  if (!ativo) return `pid ${s.pid}`;
  const nome = ativo.display || ativo.nome;
  return nome ? `pid ${ativo.pid} · ${nome}` : `pid ${ativo.pid}`;
}

/** Atualiza o chip da barra, o texto da aba Cliente e os diagnósticos. */
export function renderStatus() {
  const s = estado.status || {};
  const conectado = Boolean(s.conectado);
  const anexando = Boolean(s.anexando);
  const chip = $("#conexao");
  chip.classList.toggle("on", conectado);
  chip.classList.toggle("anexando", anexando && !conectado);
  chip.classList.toggle("erro", Boolean(s.erro) && !conectado && !anexando);
  const texto = anexando && !conectado ? "attaching…" : conectado ? rotuloConexao(s) : "disconnected";
  $("#conexao-texto").textContent = texto;
  chip.title = s.erro && !conectado ? s.erro : conectado ? `live client · pid ${s.pid}` : "client not found";

  $("#cliente-texto").textContent = conectado
    ? `attached to process ${s.pid}`
    : anexando
      ? "attaching to client…"
      : s.erro || "waiting for client";
  $("#diag-pid").textContent = s.pid ?? "—";
  $("#diag-load").textContent = hexOuTraco(s.luau_load);
  $("#diag-calld").textContent = hexOuTraco(s.lua_d_call);
  $("#diag-prelude").textContent = s.linhas_prelude ? `${s.linhas_prelude} lines` : "—";

  const botao = $("#btn-anexar");
  botao.disabled = anexando;
  botao.classList.toggle("girando", anexando);
  $("#btn-anexar-texto").textContent = conectado ? "Reattach" : anexando ? "Attaching…" : "Attach client";
  if (s.versao) $("#versao").textContent = `v${s.versao}`;
  estado.versao = s.versao || estado.versao;
}

let statusEmVoo = null;

/** Com o servidor ocupado (motor anexando) as sondagens não se empilham. */
export function pollStatus() {
  statusEmVoo ??= (async () => {
    try {
      estado.status = await api("/api/status");
    } catch {
      estado.status = { conectado: false, cliente_vivo: false };
    } finally {
      statusEmVoo = null;
    }
    renderStatus();
    verificarBordaConexao(estado.status);
  })();
  return statusEmVoo;
}

/** Só dispara na troca: sondar com o mesmo cliente vivo não pode rodar o AutoExec de novo. */
function verificarBordaConexao(s) {
  const pid = s.conectado ? (s.pid ?? 0) : null;
  if (pid === estado.ultimoPidConectado) return;
  estado.ultimoPidConectado = pid;
  if (pid !== null) autoexecutar();
}

const dormir = (ms) => new Promise((resolver) => setTimeout(resolver, ms));

// O console é best-effort: falha ao logar não pode derrubar o AutoExec.
const logarConsole = (texto, tipo) => postar("/api/console", { texto, tipo }).catch(() => null);

let avisoAutoexec = null;

function mostrarAvisoAutoexec(texto) {
  if (!avisoAutoexec) {
    const el = document.createElement("div");
    el.className = "toast info";
    el.innerHTML = `${icone("i-terminal")}<span></span><button type="button" class="btn" data-pular>Skip</button>`;
    el.querySelector("[data-pular]").addEventListener("click", pularAutoexec);
    $("#toasts").appendChild(el);
    avisoAutoexec = el;
  }
  avisoAutoexec.querySelector("span").textContent = texto;
}

function fecharAvisoAutoexec() {
  avisoAutoexec?.remove();
  avisoAutoexec = null;
}

/** Não aborta o script em curso (abortar no meio não é seguro): o corte vale entre scripts. */
function pularAutoexec() {
  estado.autoexecPular = true;
  fecharAvisoAutoexec();
}

async function executarScriptAutoexec(script) {
  try {
    const { conteudo } = await api(`/api/fs/ler?caminho=${encodeURIComponent(script.caminho)}`);
    while (estado.executando) await dormir(300);
    const resultado = await executarCodigo({ nome: script.nome, conteudo });
    if (!resultado?.ok) {
      await logarConsole(`autoexec: ${script.nome} failed: ${resultado?.erro ?? "no response from the engine"}`, "erro");
    }
  } catch (erro) {
    await logarConsole(`autoexec: ${script.nome} failed: ${erro.message}`, "erro");
  }
}

async function autoexecutar() {
  if (estado.autoexecEmCurso) return;
  estado.autoexecEmCurso = true;
  estado.autoexecPular = false;
  try {
    const { ligado, scripts } = await api("/api/autoexec");
    if (!ligado || !scripts.length) return;
    for (const [i, script] of scripts.entries()) {
      if (estado.autoexecPular) {
        await logarConsole("autoexec: skipped by user");
        return;
      }
      if (!estado.status?.conectado) {
        await logarConsole("autoexec: client disconnected, remaining cancelled", "erro");
        return;
      }
      if (i > 0) await dormir(500);
      mostrarAvisoAutoexec(`AutoExec: ${script.nome} (${i + 1}/${scripts.length})`);
      await logarConsole(`autoexec: ${script.nome}`);
      await executarScriptAutoexec(script);
    }
  } catch (erro) {
    await logarConsole(`autoexec: could not read the list: ${erro.message}`, "erro");
  } finally {
    estado.autoexecEmCurso = false;
    fecharAvisoAutoexec();
  }
}

export async function anexar() {
  const botao = $("#btn-anexar");
  botao.disabled = true;
  try {
    const s = await postar("/api/anexar", {});
    estado.status = s;
    renderStatus();
    toast(s.conectado ? `attached to pid ${s.pid}` : s.erro || "client not found", s.conectado ? "ok" : "erro");
  } catch (erro) {
    toast(`failed to attach: ${erro.message}`, "erro");
  } finally {
    botao.disabled = false;
  }
}

function iniciarExecucao(aba) {
  estado.executando = true;
  estado.problemas = [];
  estado.arquivoExecucao = aba.nome;
  renderProblemas();
  aplicarMarcas([]);
  $("#barra-progresso").classList.add("ativo");
  $("#btn-executar").disabled = true;
  trocarPainel("output");
}

function encerrarExecucao() {
  estado.executando = false;
  $("#barra-progresso").classList.remove("ativo");
  $("#btn-executar").disabled = false;
}

function avisarResultado(resultado) {
  if (resultado.ok) toast(`ran in ${resultado.ms} ms`, "ok");
  else toast(resultado.erro || "execution failed", "erro");
}

export function executar() {
  const aba = abaAtiva();
  if (!aba) {
    toast("open a script before running it", "erro");
    return;
  }
  return executarCodigo(aba);
}

/** `aba` só precisa de `nome` e `conteudo`: o Hub executa código que não está em aba. */
export async function executarCodigo(aba) {
  if (estado.executando) return;
  if (!estado.status?.conectado) {
    toast("client disconnected — attach first", "erro");
    return;
  }
  iniciarExecucao(aba);
  try {
    const resultado = await postar("/api/exec", { codigo: aba.conteudo, dia: diaLocal() });
    avisarResultado(resultado);
    return resultado;
  } catch (erro) {
    toast(`execution failed: ${erro.message}`, "erro");
  } finally {
    encerrarExecucao();
    await pollLog();
    carregarAtividade();
  }
}

export function iniciarSondagens() {
  setInterval(pollStatus, INTERVALO_STATUS_MS);
  setInterval(pollLog, INTERVALO_LOG_MS);
}
