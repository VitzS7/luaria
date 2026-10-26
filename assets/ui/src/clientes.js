// Popover de clientes Roblox no rodapé do painel Files. A lista vem de /api/clientes
// e é sondada a cada 5 s, mesmo com o popover fechado, porque o chip da barra
// mostra o cliente ativo a partir dela. Sem `setInterval`: cada volta só agenda a
// próxima quando a anterior terminou, então as requisições não se acumulam.

import { $, api, escapar, icone, postar } from "./util.js";
import { estado } from "./estado.js";
import { toast } from "./dialogo.js";
import { pollStatus, renderStatus } from "./execucao.js";

const INTERVALO_MS = 5000;
const ROTULO_STATUS = { anexado: "attached", menu: "in menu", jogo: "in game", morto: "dead", nao_anexado: "not attached" };

let aberto = false;
let emVoo = false;
let agendado = 0;
let erro = null;
let ultimaChave = null;

const botao = () => $("#btn-clientes");
const popover = () => $("#pop-clientes");
const corpo = () => $("#clientes-corpo");

function htmlFoto(cliente) {
  // Sem user_id não há headshot; o ícone fica sozinho como silhueta.
  const imagem = cliente.user_id
    ? `<img loading="lazy" width="36" height="36" alt="" src="/api/headshot?uid=${escapar(cliente.user_id)}" />`
    : "";
  return `<span class="cliente-foto">${icone("i-usuario")}${imagem}</span>`;
}

function htmlIdentidade(cliente) {
  if (!cliente.display && !cliente.nome) {
    return `<span class="cliente-nome">Jogador desconhecido</span>
      <span class="cliente-dica">execute algo para identificar</span>`;
  }
  const login = cliente.display && cliente.nome ? `<span class="cliente-login">@${escapar(cliente.nome)}</span>` : "";
  return `<span class="cliente-nome">${escapar(cliente.display || cliente.nome)}</span>${login}`;
}

function htmlCliente(cliente) {
  const acao = cliente.anexado ? "desanexar" : "anexar";
  const rotuloAcao = cliente.anexado ? "Detach" : "Attach";
  const status = ROTULO_STATUS[cliente.status] ?? cliente.status;
  return `<li class="cliente" data-pid="${escapar(cliente.pid)}"${cliente.ativo ? ' aria-current="true"' : ""}>
      ${htmlFoto(cliente)}
      <div class="cliente-info">
        <div class="cliente-titulo">${htmlIdentidade(cliente)}</div>
        <div class="cliente-meta">UserId ${escapar(cliente.user_id ?? "—")} · PID ${escapar(cliente.pid)} · PlaceId ${escapar(cliente.place_id ?? "—")}</div>
        <div class="cliente-situacao">
          <span class="cliente-status" data-status="${escapar(cliente.status)}">${escapar(status)}</span>
          ${cliente.ativo ? '<span class="cliente-tag">active</span>' : ""}
        </div>
      </div>
      <div class="cliente-acoes">
        <button type="button" class="btn cliente-acao" data-acao="${acao}" data-pid="${escapar(cliente.pid)}">${rotuloAcao}</button>
        <label class="cliente-alvo"><input type="checkbox" data-alvo="${escapar(cliente.pid)}"${cliente.alvo ? " checked" : ""} /> target</label>
      </div>
    </li>`;
}

const ESQUELETO = `<ul class="cliente-lista" aria-hidden="true">${'<li class="cliente cliente-esqueleto"><span class="cliente-foto"></span><span class="cliente-bloco"></span></li>'.repeat(3)}</ul>`;

function renderizar() {
  const alvo = corpo();
  if (erro) {
    alvo.innerHTML = `<div class="clientes-aviso"><p>could not list clients: ${escapar(erro)}</p>
      <button type="button" class="btn" data-tentar>Tentar de novo</button></div>`;
    return;
  }
  if (!estado.clientes) {
    alvo.innerHTML = ESQUELETO;
    return;
  }
  if (!estado.clientes.length) {
    alvo.innerHTML = '<div class="clientes-aviso"><p>no open Roblox client</p></div>';
    return;
  }
  const todos = estado.clientes.every((c) => c.alvo);
  alvo.innerHTML = `<label class="clientes-todos"><input type="checkbox" data-todos${todos ? " checked" : ""} /> All as target</label>
    <ul class="cliente-lista">${estado.clientes.map(htmlCliente).join("")}</ul>`;
}

/** Só redesenha quando a lista ou o erro mudou: a sondagem não derruba foco nem recarrega fotos. */
function atualizarTela() {
  const chave = JSON.stringify([erro, estado.clientes]);
  if (chave === ultimaChave) return;
  ultimaChave = chave;
  renderizar();
  renderStatus();
}

function agendar() {
  clearTimeout(agendado);
  if (document.hidden) return;
  agendado = setTimeout(atualizar, INTERVALO_MS);
}

async function atualizar() {
  if (emVoo) return;
  emVoo = true;
  clearTimeout(agendado);
  try {
    estado.clientes = await api("/api/clientes");
    erro = null;
  } catch (falha) {
    erro = falha.message;
  } finally {
    emVoo = false;
    atualizarTela();
    agendar();
  }
}

async function enviar(rota, corpoRequisicao) {
  const dados = await postar(rota, corpoRequisicao);
  if (!dados.ok) throw new Error(dados.erro);
  return dados;
}

async function acaoCliente(botaoAcao) {
  const { acao, pid } = botaoAcao.dataset;
  botaoAcao.disabled = true;
  try {
    await enviar(`/api/clientes/${acao}`, { pid: Number(pid) });
    toast(`${acao === "anexar" ? "attached" : "detached"} on pid ${pid}`, "ok");
    await Promise.all([atualizar(), pollStatus()]);
  } catch (falha) {
    toast(`failed to ${acao === "anexar" ? "attach" : "detach"}: ${falha.message}`, "erro");
  } finally {
    botaoAcao.disabled = false;
  }
}

async function gravarAlvo(pids, input, marcado) {
  try {
    await enviar("/api/clientes/alvo", { pids });
    await atualizar();
  } catch (falha) {
    input.checked = !marcado;
    toast(`could not change target: ${falha.message}`, "erro");
  }
}

function alternarAlvo(input) {
  const pid = Number(input.dataset.alvo);
  const atuais = estado.clientes.filter((c) => c.alvo).map((c) => c.pid);
  const pids = input.checked ? [...atuais, pid] : atuais.filter((p) => p !== pid);
  gravarAlvo(pids, input, input.checked);
}

function alternarTodos(input) {
  gravarAlvo(input.checked ? "todos" : [], input, input.checked);
}

function ligarCorpo() {
  const alvo = corpo();
  alvo.addEventListener("click", (evento) => {
    const botaoAcao = evento.target.closest("[data-acao]");
    if (botaoAcao) acaoCliente(botaoAcao);
    else if (evento.target.closest("[data-tentar]")) atualizar();
  });
  alvo.addEventListener("change", (evento) => {
    if (evento.target.matches("[data-alvo]")) alternarAlvo(evento.target);
    else if (evento.target.matches("[data-todos]")) alternarTodos(evento.target);
  });
  // Erro de imagem não borbulha: a captura pega a foto que não carregou e deixa a silhueta.
  alvo.addEventListener(
    "error",
    (evento) => {
      if (evento.target.tagName === "IMG") evento.target.remove();
    },
    true,
  );
}

function abrir() {
  aberto = true;
  popover().hidden = false;
  botao().setAttribute("aria-expanded", "true");
  atualizarTela();
  atualizar();
}

function fechar() {
  aberto = false;
  popover().hidden = true;
  botao().setAttribute("aria-expanded", "false");
}

export function ligarClientes() {
  ligarCorpo();
  botao().addEventListener("click", () => (aberto ? fechar() : abrir()));
  document.addEventListener("keydown", (evento) => {
    if (evento.key !== "Escape" || !aberto) return;
    fechar();
    botao().focus();
  });
  document.addEventListener("pointerdown", (evento) => {
    if (aberto && !popover().contains(evento.target) && !botao().contains(evento.target)) fechar();
  });
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) atualizar();
  });
  atualizar();
}
