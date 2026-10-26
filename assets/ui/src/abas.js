// Abas do editor: cada uma guarda o próprio texto, se está suja, fixada ou em
// somente leitura. Trocar de aba troca o documento do CodeMirror inteiro.

import { $, $$, escapar, icone } from "./util.js";
import { estado } from "./estado.js";
import { definirLeitura, mostrarDocumento, aplicarMarcas } from "./editor.js";
import { dialogo } from "./dialogo.js";
import { salvarAba, criarArquivo, renomearCaminho, novoScript, atualizarArvore } from "./arquivos.js";
import { executar } from "./execucao.js";

/** Conexão do editor com as ações do app. Funções lidas na hora, sem import circular. */
export const acoesEditor = {
  salvar: () => salvarAba(abaAtiva()),
  executar: () => executar(),
  novo: () => novoScript(),
  aoEditar: (texto) => {
    const aba = abaAtiva();
    if (!aba) return;
    aba.conteudo = texto;
    if (!aba.sujo) {
      aba.sujo = true;
      renderAbas();
      atualizarArvore();
    }
  },
};

export const abaAtiva = () => estado.abas.find((a) => a.id === estado.ativa) || null;

/** Fixadas vêm primeiro; a ordem de criação dentro de cada grupo é mantida. */
const ordemVisual = () => [
  ...estado.abas.filter((a) => a.fixada),
  ...estado.abas.filter((a) => !a.fixada),
];

/** Arquivo do disco usa o caminho como id; sem caminho (Hub), o nome é o id. */
export function abrirAba(nome, conteudo, caminho = null) {
  const id = caminho ?? nome;
  let aba = estado.abas.find((a) => a.id === id);
  if (!aba) {
    aba = { id, nome, caminho, conteudo, sujo: false, fixada: false, leitura: false };
    estado.abas = [...estado.abas, aba];
  } else if (conteudo !== undefined) {
    aba.conteudo = conteudo;
  }
  ativarAba(aba.id);
}

/** Renomear/mover/excluir mudam o caminho: abas abertas acompanham, ou fecham se o arquivo sumiu. */
export function atualizarCaminhoAbas(de, para) {
  const dentro = (c) => Boolean(c) && (c === de || c.startsWith(`${de}/`));
  const ativaAntes = estado.ativa;
  const indiceAtiva = estado.abas.findIndex((a) => a.id === ativaAntes);
  estado.abas = estado.abas.flatMap((a) => {
    if (!dentro(a.caminho)) return [a];
    if (!para) return [];
    const caminho = para + a.caminho.slice(de.length);
    return [{ ...a, id: caminho, caminho, nome: caminho.slice(caminho.lastIndexOf("/") + 1) }];
  });
  if (!dentro(ativaAntes)) {
    renderAbas();
    atualizarArvore();
    return;
  }
  if (para) {
    estado.ativa = para + ativaAntes.slice(de.length);
    renderAbas();
    atualizarArvore();
    return;
  }
  estado.ativa = null;
  const proxima = estado.abas[Math.min(indiceAtiva, estado.abas.length - 1)];
  if (proxima) {
    ativarAba(proxima.id);
    return;
  }
  mostrarDocumento("", { acoes: acoesEditor });
  atualizarVazio();
  renderAbas();
  atualizarArvore();
}

/** Aba sem arquivo ainda: nasce vazia e o Salvar pergunta o nome. */
export function abrirRascunho(nome) {
  if (!estado.abas.some((a) => a.nome === nome)) {
    const rascunho = { id: nome, nome, conteudo: "", sujo: false, fixada: false, leitura: false, rascunho: true };
    estado.abas = [...estado.abas, rascunho];
  }
  ativarAba(nome);
}

export function ativarAba(id) {
  const aba = estado.abas.find((a) => a.id === id);
  if (!aba) return;
  estado.ativa = aba.id;
  mostrarDocumento(aba.conteudo, { somenteLeitura: aba.leitura, acoes: acoesEditor });
  atualizarVazio();
  aplicarMarcas(marcasDaAba(aba));
  renderAbas();
  atualizarArvore();
}

/** Problemas da execução só marcam o script que foi executado. */
export function marcasDaAba(aba) {
  return estado.problemas.filter((p) => {
    if (!p.linha) return false;
    return !estado.arquivoExecucao || aba.nome === estado.arquivoExecucao;
  });
}

export function atualizarVazio() {
  const temAba = Boolean(abaAtiva());
  $("#editor-vazio").classList.toggle("visivel", !temAba);
  $("#editor").style.visibility = temAba ? "visible" : "hidden";
}

/** Primeiro "Untitled-N" livre, para Ctrl+T não repetir nome. */
export function novoRascunho() {
  let numero = 1;
  while (estado.abas.some((a) => a.nome === `Untitled-${numero}`)) numero++;
  abrirRascunho(`Untitled-${numero}`);
}

/** Ctrl+1..9: a posição vale na ordem visual (fixadas primeiro), como na barra. */
export function ativarPorPosicao(posicao) {
  const aba = ordemVisual()[posicao - 1];
  if (aba) ativarAba(aba.id);
}

export function ciclarAba(passo) {
  const ordem = ordemVisual();
  if (!ordem.length) return;
  const atual = ordem.findIndex((a) => a.id === estado.ativa);
  const proxima = ordem[(atual + passo + ordem.length) % ordem.length];
  ativarAba(proxima.id);
}

export function fecharAbaAtiva() {
  if (estado.ativa) fecharAba(estado.ativa);
}

export function fecharAba(id) {
  const indice = estado.abas.findIndex((a) => a.id === id);
  if (indice < 0) return;
  const aba = estado.abas[indice];
  if (!aba.sujo || !estado.tema.confirmar_fechar) {
    removerAba(indice);
    return;
  }
  dialogo({
    titulo: "Discard changes?",
    sub: `${aba.nome} has unsaved changes.`,
    campo: false,
    confirmar: "Descartar",
    perigo: true,
  }).then((ok) => {
    if (ok) removerAba(estado.abas.findIndex((a) => a.id === id));
  });
}

export function removerAba(indice) {
  if (indice < 0) return;
  const restantes = estado.abas.filter((_, i) => i !== indice);
  estado.abas = restantes;
  const proxima = restantes[Math.max(0, indice - 1)] || restantes[0] || null;
  if (proxima) {
    ativarAba(proxima.id);
    return;
  }
  estado.ativa = null;
  mostrarDocumento("", { acoes: acoesEditor });
  atualizarVazio();
  renderAbas();
  atualizarArvore();
}

/** Cores de marcação da aba: só visual, ficam na sessão (não vão para o ui.json). */
const CORES_ABA = ["#8CC4D4", "#59C98B", "#E7B75F", "#FF7B72", "#B58CFF"];

export function renderAbas() {
  const alvo = $("#abas");
  alvo.innerHTML = "";
  for (const aba of ordemVisual()) {
    const el = document.createElement("div");
    el.className = `aba${aba.id === estado.ativa ? " ativa" : ""}${aba.fixada ? " fixada" : ""}${aba.cor ? " com-cor" : ""}`;
    if (aba.cor) el.style.setProperty("--cor-aba", aba.cor);
    el.setAttribute("role", "tab");
    el.setAttribute("aria-selected", String(aba.id === estado.ativa));
    el.dataset.id = aba.id;
    el.innerHTML = `
      ${icone("i-arquivo")}
      <span class="aba-nome">${escapar(aba.nome)}</span>
      ${aba.sujo ? '<span class="aba-ponto" title="unsaved"></span>' : ""}
      ${aba.fixada ? "" : `<button class="aba-fechar" title="Close" aria-label="Close ${escapar(aba.nome)}">${icone("i-x")}</button>`}`;
    el.addEventListener("click", (evento) => {
      if (evento.target.closest(".aba-fechar")) return;
      ativarAba(aba.id);
    });
    el.addEventListener("contextmenu", (evento) => {
      evento.preventDefault();
      abrirMenuAba(evento, aba.id);
    });
    el.querySelector(".aba-fechar")?.addEventListener("click", (evento) => {
      evento.stopPropagation();
      fecharAba(aba.id);
    });
    alvo.appendChild(el);
  }
}

let abaDoMenu = null;

function abrirMenuAba(evento, id) {
  abaDoMenu = id;
  const aba = estado.abas.find((a) => a.id === id);
  const menu = $("#menu-aba");
  $('[data-acao="fixar"]', menu).innerHTML = `${icone("i-fixar")}${aba.fixada ? "Unpin" : "Pin"}`;
  $('[data-acao="leitura"]', menu).innerHTML = `${icone("i-cadeado")}${aba.leitura ? "Allow editing" : "Read-only"}`;
  menu.hidden = false;
  menu.style.left = `${Math.min(evento.clientX, window.innerWidth - 200)}px`;
  menu.style.top = `${Math.min(evento.clientY, window.innerHeight - 260)}px`;
}

function fecharMenuAba() {
  $("#menu-aba").hidden = true;
  abaDoMenu = null;
}

function alterarAba(id, alteracoes) {
  estado.abas = estado.abas.map((a) => (a.id === id ? { ...a, ...alteracoes } : a));
}

async function acaoMenuAba(acao) {
  const aba = estado.abas.find((a) => a.id === abaDoMenu);
  if (!aba) return;
  const id = aba.id;
  if (acao === "fixar") alterarAba(id, { fixada: !aba.fixada });
  if (acao === "leitura") {
    alterarAba(id, { leitura: !aba.leitura });
    if (estado.ativa === id) definirLeitura(!aba.leitura);
  }
  if (acao === "cor") {
    const proxima = CORES_ABA[(CORES_ABA.indexOf(aba.cor) + 1) % CORES_ABA.length];
    alterarAba(id, { cor: proxima });
  }
  if (acao === "renomear" && aba.caminho) await renomearCaminho(aba.caminho);
  if (acao === "duplicar") await criarArquivo(copiaDe(aba.nome), aba.conteudo, pastaDaAba(aba));
  if (acao === "salvar-como") await salvarComo(aba);
  if (acao === "fechar") fecharAba(id);
  renderAbas();
}

/** `nome.lua` vira `nome-copia.lua`; o servidor ainda desambigua se já existir. */
const copiaDe = (nome) => nome.replace(/(\.lua|\.luau)?$/, "-copia.lua");

/** A cópia vai para a mesma pasta do arquivo; rascunho sem caminho cai em Scripts. */
const pastaDaAba = (aba) => (aba.caminho ? aba.caminho.slice(0, aba.caminho.lastIndexOf("/")) : "Scripts");

async function salvarComo(aba) {
  const nome = await dialogo({
    titulo: "Save as",
    sub: "Creates a new script with this tab content.",
    valor: copiaDe(aba.nome),
    confirmar: "Salvar",
  });
  if (!nome) return;
  await criarArquivo(nome, aba.conteudo);
}

export function ligarMenuAbas() {
  $("#menu-aba").addEventListener("click", (evento) => {
    const item = evento.target.closest("[data-acao]");
    if (!item) return;
    $("#menu-aba").hidden = true;
    acaoMenuAba(item.dataset.acao);
  });
  document.addEventListener("click", (evento) => {
    if (!evento.target.closest("#menu-aba")) fecharMenuAba();
  });
  $$(".abas").forEach((el) => el.addEventListener("scroll", fecharMenuAba));
}

