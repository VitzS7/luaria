// Console do Output e aba Problemas. Linhas vêm do /api/log; ruído é só
// escondido na tela, nunca descartado, então o filtro pode ser desligado.

import { $, api, escapar } from "./util.js";
import { estado } from "./estado.js";
import { aplicarMarcas, irParaLinha } from "./editor.js";
import { abaAtiva, marcasDaAba } from "./abas.js";
import { irPara } from "./navegacao.js";

const RUIDO = /\[perf\]|Wrap-deformer|\[compat\]/i;
const PREFIXOS = { script: "»", warn: "!", erro: "×", info: "·" };
const LIMITE_DOM = 1500;
const LIMITE_MEMORIA = 2000;
const LIMITE_PROBLEMAS = 300;
// print de 1 MB numa linha só travaria o layout; o texto inteiro fica em memória
const LIMITE_TEXTO = 4000;

const escondida = (linha) => estado.filtroRuido && RUIDO.test(linha.texto);

const cortar = (texto) =>
  texto.length > LIMITE_TEXTO ? `${texto.slice(0, LIMITE_TEXTO)}… (+${texto.length - LIMITE_TEXTO} characters)` : texto;

function montarLinha(linha) {
  const el = document.createElement("div");
  el.className = `log-linha ${linha.tipo}`;
  el.innerHTML = `
    <span class="log-prefixo">${PREFIXOS[linha.tipo] || "·"}</span>
    <span class="log-texto">${escapar(cortar(linha.texto))}</span>
    <span class="log-seq">${linha.linha ? `L${linha.linha}` : ""}</span>`;
  if (linha.linha) {
    el.title = `line ${linha.linha} — click to go`;
    el.classList.add("clicavel");
    el.addEventListener("click", () => {
      irPara("editor");
      irParaLinha(linha.linha);
    });
  }
  return el;
}

function atualizarVazio() {
  const vazio = estado.linhas.length === 0 && estado.painel === "output";
  $("#console-vazio").style.display = vazio ? "" : "none";
}

function rolarConsole() {
  if (!estado.autoScroll) return;
  const corpo = $("#console-linhas");
  corpo.scrollTop = corpo.scrollHeight;
}

// Quem leu o fim do console continua acompanhando; quem subiu para ler não é puxado de volta.
const pertoDoFim = (alvo) => alvo.scrollHeight - alvo.scrollTop - alvo.clientHeight < 40;

// Linhas que chegam no mesmo quadro entram no DOM juntas, não uma a uma.
let filaLinhas = [];
let quadroLinhas = 0;

function cancelarFila() {
  cancelAnimationFrame(quadroLinhas);
  quadroLinhas = 0;
  filaLinhas = [];
}

function enfileirarLinha(el) {
  filaLinhas.push(el);
  quadroLinhas ||= requestAnimationFrame(descarregarLinhas);
}

function descarregarLinhas() {
  quadroLinhas = 0;
  const alvo = $("#console-linhas");
  const perto = pertoDoFim(alvo);
  const pacote = document.createDocumentFragment();
  pacote.append(...filaLinhas);
  filaLinhas = [];
  alvo.appendChild(pacote);
  while (alvo.childElementCount > LIMITE_DOM) alvo.removeChild(alvo.firstElementChild);
  if (perto) rolarConsole();
}

/** Redesenha o console inteiro (ao ligar/desligar o filtro de ruído). */
export function renderConsole() {
  cancelarFila();
  const pacote = document.createDocumentFragment();
  estado.linhas
    .filter((l) => !escondida(l))
    .slice(-LIMITE_DOM)
    .forEach((l) => pacote.appendChild(montarLinha(l)));
  $("#console-linhas").replaceChildren(pacote);
  rolarConsole();
  atualizarVazio();
}

export function adicionarLinha(linha) {
  // corta em lote: copiar o array inteiro a cada linha vira O(n²) num spam de print
  estado.linhas.push(linha);
  if (estado.linhas.length > LIMITE_MEMORIA + 200) estado.linhas.splice(0, estado.linhas.length - LIMITE_MEMORIA);
  if (!escondida(linha)) enfileirarLinha(montarLinha(linha));
  if (linha.tipo === "erro" || linha.tipo === "warn") registrarProblema(linha);
  atualizarVazio();
}

function registrarProblema(linha) {
  const chave = `${linha.tipo}:${linha.linha || 0}:${linha.texto}`;
  if (estado.problemas.length >= LIMITE_PROBLEMAS || estado.problemas.some((p) => p.chave === chave)) return;
  estado.problemas = [
    ...estado.problemas,
    {
      chave,
      tipo: linha.tipo,
      texto: linha.texto,
      linha: linha.linha || null,
      arquivo: estado.arquivoExecucao || abaAtiva()?.nome || null,
    },
  ];
  renderProblemas();
  aplicarMarcas(abaAtiva() ? marcasDaAba(abaAtiva()) : []);
}

export function limparProblemas() {
  estado.problemas = [];
  renderProblemas();
}

function itemProblema(problema) {
  const botao = document.createElement("button");
  botao.className = `problema ${problema.tipo}`;
  botao.innerHTML = `
    <span class="problema-marca" aria-hidden="true"></span>
    <span class="problema-texto">
      ${escapar(problema.texto)}
      <span class="problema-arquivo">${escapar(problema.arquivo || "client")}</span>
    </span>
    <span class="problema-linha">${problema.linha ? `L${problema.linha}` : "—"}</span>`;
  if (problema.linha) {
    botao.addEventListener("click", () => {
      irPara("editor");
      irParaLinha(problema.linha);
    });
  }
  return botao;
}

export function renderProblemas() {
  const lista = $("#lista-problemas");
  const contador = $("#contador-problemas");
  contador.textContent = String(estado.problemas.length);
  contador.classList.toggle("quente", estado.problemas.some((p) => p.tipo === "erro"));
  lista.innerHTML = "";
  estado.problemas.forEach((p) => lista.appendChild(itemProblema(p)));
  $("#problemas-vazio").style.display = estado.problemas.length ? "none" : "";
}

export function trocarPainel(painel) {
  estado.painel = painel;
  $("#btn-painel-output").classList.toggle("ativo", painel === "output");
  $("#btn-painel-problemas").classList.toggle("ativo", painel === "problemas");
  $("#console-linhas").style.display = painel === "output" ? "" : "none";
  $("#problemas").hidden = painel !== "problemas";
  atualizarVazio();
}

export function limparConsole() {
  estado.linhas = [];
  cancelarFila();
  $("#console-linhas").replaceChildren();
  limparProblemas();
  aplicarMarcas([]);
  atualizarVazio();
}

let logEmVoo = null;

/** Uma consulta por vez: o intervalo e o fim da execução pediam o mesmo
 * `desde` em paralelo e a mesma linha entrava duas vezes no console. */
export function pollLog() {
  logEmVoo ??= (async () => {
    try {
      const novas = await api(`/api/log?desde=${estado.ultimoSeq}`);
      for (const linha of novas) {
        // o servidor devolve seq > desde; guardar seq + 1 pulava a primeira linha de cada lote
        if (linha.seq <= estado.ultimoSeq) continue;
        estado.ultimoSeq = linha.seq;
        adicionarLinha(linha);
      }
    } catch {
      /* servidor caiu; o próximo ciclo tenta de novo */
    } finally {
      logEmVoo = null;
    }
  })();
  return logEmVoo;
}

export function ligarConsole() {
  $("#btn-painel-output").addEventListener("click", () => trocarPainel("output"));
  $("#btn-painel-problemas").addEventListener("click", () => trocarPainel("problemas"));
  $("#btn-filtro").addEventListener("click", () => {
    estado.filtroRuido = !estado.filtroRuido;
    $("#btn-filtro").classList.toggle("ativo", estado.filtroRuido);
    renderConsole();
  });
  $("#btn-autoscroll").addEventListener("click", () => {
    estado.autoScroll = !estado.autoScroll;
    $("#btn-autoscroll").classList.toggle("ativo", estado.autoScroll);
  });
  $("#btn-limpar").addEventListener("click", limparConsole);
  $("#btn-limpar-editor").addEventListener("click", limparConsole);
}
