// Biblioteca de scripts: grade de cartões com trecho do código, busca e o
// modal de filtros. Ordenação e filtro são só locais (sem fonte externa).

import { $, adiar, $$, bytes, dataCurta, escapar, icone, plural } from "./util.js";
import { estado } from "./estado.js";
import { abrirModal, fecharModal } from "./dialogo.js";
import { abrirScript, novoScript, renomearScript, excluirScript } from "./arquivos.js";

const ORDENS = [
  { id: "recentes", nome: "Most recent" },
  { id: "antigos", nome: "Oldest" },
  { id: "az", nome: "A → Z" },
  { id: "maior", nome: "Largest first" },
];

const MOSTRAR = [
  { id: "todos", nome: "All" },
  { id: "com-codigo", nome: "With code" },
  { id: "vazios", nome: "Empty" },
];

const COMPARADORES = {
  recentes: (a, b) => b.modificado - a.modificado,
  antigos: (a, b) => a.modificado - b.modificado,
  az: (a, b) => a.nome.localeCompare(b.nome, "pt-BR"),
  maior: (a, b) => b.tamanho - a.tamanho,
};

const ehVazio = (s) => s.tamanho === 0 || !s.previa;

/** Aplica busca, filtro de "Mostrar" e ordenação, nessa ordem. */
export function listaFiltrada() {
  const termo = estado.buscaScripts.toLowerCase();
  return estado.scripts
    .filter((s) => {
      if (!termo) return true;
      return s.nome.toLowerCase().includes(termo) || s.trecho.toLowerCase().includes(termo);
    })
    .filter((s) => {
      if (estado.mostrar === "com-codigo") return !ehVazio(s);
      if (estado.mostrar === "vazios") return ehVazio(s);
      return true;
    })
    .sort(COMPARADORES[estado.ordem] || COMPARADORES.recentes);
}

function cartao(script) {
  const meta = `${plural(script.linhas, "line", "lines")} · ${bytes(script.tamanho)} · ${dataCurta(script.modificado)}`;
  return `<article class="cartao" tabindex="0" data-nome="${escapar(script.nome)}">
    <div class="cartao-trecho" aria-hidden="true"><pre>${escapar(script.trecho || "(empty)")}</pre></div>
    <div class="cartao-corpo">
      <h3 class="cartao-nome">${escapar(script.nome)}</h3>
      <p class="cartao-meta">${meta}</p>
    </div>
    <div class="cartao-acoes">
      <button class="btn-icone" data-acao="renomear" title="Rename" aria-label="Rename ${escapar(script.nome)}">${icone("i-lapis")}</button>
      <button class="btn-icone perigo" data-acao="excluir" title="Delete" aria-label="Delete ${escapar(script.nome)}">${icone("i-lixo")}</button>
    </div>
  </article>`;
}

function ligarCartoes(alvo) {
  alvo.querySelectorAll(".cartao").forEach((el) => {
    const nome = el.dataset.nome;
    el.addEventListener("click", (evento) => {
      const acao = evento.target.closest("[data-acao]");
      if (!acao) return abrirScript(nome);
      evento.stopPropagation();
      if (acao.dataset.acao === "renomear") renomearScript(nome);
      if (acao.dataset.acao === "excluir") excluirScript(nome);
    });
    el.addEventListener("keydown", (evento) => {
      if (evento.key === "Enter") abrirScript(nome);
    });
  });
}

export function renderGrade() {
  const alvo = $("#grade-scripts");
  const lista = listaFiltrada();
  const semNada = estado.scripts.length === 0;
  alvo.innerHTML = lista.map(cartao).join("");
  ligarCartoes(alvo);
  $("#scripts-vazio").hidden = !semNada;
  $("#scripts-sem-resultado").hidden = semNada || lista.length > 0;
  alvo.hidden = semNada || lista.length === 0;
  atualizarSelosFiltro();
}

function atualizarSelosFiltro() {
  const ativos = (estado.ordem !== "recentes" ? 1 : 0) + (estado.mostrar !== "todos" ? 1 : 0);
  const selo = $("#badge-filtros");
  selo.hidden = ativos === 0;
  selo.textContent = String(ativos);
  $("#contagem-scripts").textContent = String(estado.scripts.length);
}

function opcoesRadio(alvo, grupo, lista, marcado, aoMudar) {
  alvo.innerHTML = lista
    .map(
      (op) => `<label class="opcao"><input type="radio" name="${grupo}" value="${op.id}" ${
        op.id === marcado ? "checked" : ""
      }><span>${escapar(op.nome)}</span></label>`,
    )
    .join("");
  alvo.querySelectorAll("input").forEach((input) =>
    input.addEventListener("change", () => aoMudar(input.value)),
  );
}

function renderFiltros() {
  opcoesRadio($("#filtro-ordem"), "ordem", ORDENS, estado.ordem, (v) => {
    estado.ordem = v;
    renderGrade();
  });
  opcoesRadio($("#filtro-mostrar"), "mostrar", MOSTRAR, estado.mostrar, (v) => {
    estado.mostrar = v;
    renderGrade();
  });
}

function abrirFiltros() {
  renderFiltros();
  abrirModal($("#modal-filtros"));
}

function limparFiltros() {
  estado.ordem = "recentes";
  estado.mostrar = "todos";
  renderFiltros();
  renderGrade();
}

export function ligarScripts() {
  const buscar = adiar(renderGrade, 120);
  $("#busca-scripts").addEventListener("input", (e) => {
    estado.buscaScripts = e.target.value;
    buscar();
  });
  $("#btn-criar").addEventListener("click", novoScript);
  $("#btn-filtros").addEventListener("click", abrirFiltros);
  $("#filtros-limpar").addEventListener("click", limparFiltros);
  $$("#modal-filtros [data-fechar]").forEach((b) =>
    b.addEventListener("click", () => fecharModal($("#modal-filtros"))),
  );
  $("#modal-filtros").addEventListener("click", (e) => {
    if (e.target.id === "modal-filtros") fecharModal($("#modal-filtros"));
  });
}


