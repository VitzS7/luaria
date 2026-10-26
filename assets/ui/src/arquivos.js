// Arquivos do disco: árvore do painel Files (Scripts, Workspace, AutoExec) e as
// ações sobre ela. Tudo passa pela API /api/fs; a árvore e as abas são refeitas
// a partir do que o servidor devolve, nunca mantidas em cópia local.

import { $, $$, adiar, api, escapar, icone, postar, bytes } from "./util.js";
import { estado } from "./estado.js";
import { toast, dialogo, abrirModal, fecharModal } from "./dialogo.js";
import { abrirAba, ativarAba, atualizarCaminhoAbas, renderAbas } from "./abas.js";
import { irPara } from "./navegacao.js";
import { renderGrade } from "./scripts.js";
import { renderRecentes } from "./home.js";

const SEPARADOR = null;
const basename = (caminho) => caminho.slice(caminho.lastIndexOf("/") + 1);
const pastaDe = (caminho) => caminho.slice(0, caminho.lastIndexOf("/"));
const ehRaiz = (caminho) => !caminho.includes("/");
/** `caminho` é a pasta ou um item dentro dela. */
const dentroDe = (caminho, pasta) => caminho === pasta || caminho.startsWith(`${pasta}/`);

// Só para a árvore: a API traz o estado do AutoExec junto com as pastas.
let autoexec = null;
// "carregando" | "erro" | null: enquanto houver, a árvore não mostra "nenhum script".
let situacao = null;
// Linha que recebe o Tab na árvore (roving tabindex).
let focoArvore = "Scripts";
let menuAberto = null;
let caminhoParaMover = null;

function buscarNo(caminho, nos = estado.arvore) {
  for (const no of nos) {
    if (no.caminho === caminho) return no;
    const achado = no.filhos && buscarNo(caminho, no.filhos);
    if (achado) return achado;
  }
  return null;
}

function listarPastas(nos = estado.arvore) {
  return nos.flatMap((no) => (no.tipo === "pasta" ? [no.caminho, ...listarPastas(no.filhos)] : []));
}

/** Mantém o nó que casa e, nas pastas, só os filhos que casam (ancestrais ficam). */
function filtrarNos(nos, termo) {
  return nos.flatMap((no) => {
    const casa = no.nome.toLowerCase().includes(termo);
    if (no.tipo === "arquivo") return casa ? [no] : [];
    const filhos = casa ? no.filhos : filtrarNos(no.filhos, termo);
    return casa || filhos.length ? [{ ...no, filhos }] : [];
  });
}

const htmlMais = (nome) =>
  `<button type="button" class="no-mais" tabindex="-1" aria-label="Actions for ${escapar(nome)}">${icone("i-mais-acoes")}</button>`;

const marcaSuja = (caminho) =>
  estado.abas.some((a) => a.id === caminho && a.sujo) ? '<span class="aba-ponto" title="unsaved"></span>' : "";

function htmlArquivo(no, nivel, foco) {
  const ativo = estado.ativa === no.caminho;
  return `<li role="none"><div class="no arquivo${ativo ? " ativo" : ""}" role="treeitem" aria-level="${nivel}"
      aria-selected="${ativo}" tabindex="${foco}" data-caminho="${escapar(no.caminho)}" data-tipo="arquivo"
      title="${bytes(no.tamanho)}"><span class="no-espaco"></span>${icone("i-arquivo")}
      <span class="arquivo-nome">${escapar(no.nome)}</span>${marcaSuja(no.caminho)}${htmlMais(no.nome)}</div></li>`;
}

function htmlChave() {
  return `<label class="no-chave" title="Run these scripts when the client attaches"><input type="checkbox"
      role="switch" data-autoexec aria-label="AutoExec on"${autoexec.ligado ? " checked" : ""} /></label>`;
}

function htmlPasta(no, termo, nivel, foco) {
  const aberta = Boolean(termo) || estado.expandidas.has(no.caminho);
  const comChave = no.caminho === "AutoExec" && autoexec;
  const quantidade = comChave ? autoexec.scripts.length : no.filhos.length;
  const filhos = aberta
    ? `<ul class="arvore-itens" role="group">${no.filhos.map((f) => htmlNo(f, termo, nivel + 1)).join("")}</ul>`
    : "";
  return `<li role="none"><div class="no arvore-pasta" role="treeitem" aria-level="${nivel}" aria-expanded="${aberta}"
      tabindex="${foco}" data-caminho="${escapar(no.caminho)}" data-tipo="pasta">
      <svg class="icone chevron"><use href="#i-chevron" /></svg>${icone("i-pasta")}
      <span class="arquivo-nome">${escapar(no.nome)}</span>${comChave ? htmlChave() : ""}
      <span class="contagem">${quantidade}</span>${htmlMais(no.nome)}</div>${filhos}</li>`;
}

function htmlNo(no, termo, nivel) {
  const foco = no.caminho === focoArvore ? 0 : -1;
  return no.tipo === "arquivo" ? htmlArquivo(no, nivel, foco) : htmlPasta(no, termo, nivel, foco);
}

/** Árvore do painel Files, a partir de `estado.arvore`. Pastas fechadas não geram filhos. */
export function atualizarArvore() {
  const alvo = $("#lista-arquivos");
  const termo = (estado.busca || "").trim().toLowerCase();
  const nos = filtrarNos(estado.arvore, termo);
  alvo.innerHTML = nos.map((no) => htmlNo(no, termo, 1)).join("");
  const scriptsVazio = !buscarNo("Scripts")?.filhos.length;
  $("#arvore-vazio").textContent = termo ? "nothing matches the search" : "no scripts yet";
  $("#arvore-vazio").hidden = Boolean(situacao) || !(termo ? nos.length === 0 : scriptsVazio);
  if (!alvo.querySelector('.no[tabindex="0"]')) alvo.querySelector(".no")?.setAttribute("tabindex", "0");
}

function mostrarSituacao(tipo, detalhe = "") {
  situacao = tipo;
  $("#arvore-estado").hidden = !tipo;
  $("#arvore-estado-texto").textContent =
    tipo === "erro" ? `could not read files: ${detalhe}` : "loading files…";
  $("#btn-tentar-arvore").hidden = tipo !== "erro";
}

async function carregarArvore() {
  if (!estado.arvore.length) mostrarSituacao("carregando");
  try {
    const [arvore, dadosAutoexec] = await Promise.all([
      api("/api/fs/arvore"),
      api("/api/autoexec").catch(() => null),
    ]);
    estado.arvore = arvore;
    autoexec = dadosAutoexec;
    mostrarSituacao(null);
  } catch (erro) {
    mostrarSituacao("erro", erro.message);
  }
  atualizarArvore();
}

export async function carregarScripts() {
  estado.scripts = await api("/api/scripts").catch(() => []);
  await carregarArvore();
  renderGrade();
  renderRecentes();
  $("#contagem-scripts").textContent = String(estado.scripts.length);
}

function expandirAte(caminho) {
  const partes = caminho.split("/");
  const ancestrais = partes.slice(0, -1).map((_, i) => partes.slice(0, i + 1).join("/"));
  estado.expandidas = new Set([...estado.expandidas, ...ancestrais]);
}

function alternarPasta(caminho) {
  const expandidas = new Set(estado.expandidas);
  if (expandidas.has(caminho)) expandidas.delete(caminho);
  else expandidas.add(caminho);
  estado.expandidas = expandidas;
  atualizarArvore();
  focarNo(caminho);
}

/** Renomear ou mover troca o prefixo; excluir tira a pasta e o que ficou dentro. */
function remapearExpandidas(de, para) {
  estado.expandidas = new Set(
    [...estado.expandidas]
      .filter((c) => para || !dentroDe(c, de))
      .map((c) => (dentroDe(c, de) ? para + c.slice(de.length) : c)),
  );
}

function focarNo(caminho) {
  const no = [...$("#lista-arquivos").querySelectorAll(".no")].find((n) => n.dataset.caminho === caminho);
  no?.focus();
}

/** Se a aba já está aberta, só ativa: recarregar do disco jogaria fora o que não foi salvo. */
export async function abrirCaminho(caminho) {
  expandirAte(caminho);
  if (estado.abas.some((a) => a.id === caminho)) {
    ativarAba(caminho);
    irPara("editor");
    return true;
  }
  try {
    const dados = await api(`/api/fs/ler?caminho=${encodeURIComponent(caminho)}`);
    abrirAba(basename(caminho), dados.conteudo, caminho);
    irPara("editor");
    return true;
  } catch (erro) {
    toast(`could not open ${caminho}: ${erro.message}`, "erro");
    return false;
  }
}

export const abrirScript = (nome) => abrirCaminho(`Scripts/${nome}`);

export async function salvarAba(aba) {
  if (!aba) {
    toast("no script open", "erro");
    return;
  }
  if (aba.rascunho) {
    const nome = await dialogo({
      titulo: "Save as",
      sub: "The file is created in Scripts/.",
      valor: "script.lua",
      confirmar: "Save",
    });
    if (nome) await criarArquivo(nome, aba.conteudo);
    return;
  }
  try {
    await postar("/api/fs/gravar", { caminho: aba.caminho, conteudo: aba.conteudo });
    estado.abas = estado.abas.map((a) => (a.id === aba.id ? { ...a, sujo: false } : a));
    renderAbas();
    await carregarScripts();
    toast(`${aba.nome} saved`, "ok");
  } catch (erro) {
    toast(`failed to save: ${erro.message}`, "erro");
  }
}

/** Cria o arquivo no disco e abre. Sem `conteudo`, o servidor põe o modelo padrão. */
export async function criarArquivo(nome, conteudo, pai = "Scripts") {
  try {
    const dados = await postar("/api/fs/criar", { pai, nome, tipo: "script", conteudo });
    await carregarScripts();
    await abrirCaminho(dados.caminho);
    toast(`${dados.nome} criado`, "ok");
    return dados.caminho;
  } catch (erro) {
    toast(`could not create: ${erro.message}`, "erro");
    return null;
  }
}

async function novoScriptEm(pai) {
  const nome = await dialogo({
    titulo: "New script",
    sub: `The file is created in ${pai}/.`,
    valor: "script",
    confirmar: "Criar",
  });
  if (nome) await criarArquivo(nome, undefined, pai);
}

export const novoScript = () => novoScriptEm("Scripts");

async function novaPastaEm(pai) {
  const nome = await dialogo({
    titulo: "New folder",
    sub: `The folder is created inside ${pai}.`,
    valor: "nova-pasta",
    confirmar: "Criar",
  });
  if (!nome) return;
  try {
    const dados = await postar("/api/fs/criar", { pai, nome, tipo: "pasta" });
    expandirAte(dados.caminho);
    await carregarScripts();
    toast(`folder ${dados.nome} created`, "ok");
  } catch (erro) {
    toast(`could not create folder: ${erro.message}`, "erro");
  }
}

export async function renomearCaminho(caminho) {
  const pasta = buscarNo(caminho)?.tipo === "pasta";
  const atual = basename(caminho);
  const novo = await dialogo({
    titulo: pasta ? "Rename folder" : "Rename script",
    sub: pasta ? "" : "The name must end in .lua.",
    valor: atual,
    confirmar: "Renomear",
  });
  if (!novo || novo === atual) return;
  try {
    const dados = await postar("/api/fs/renomear", { caminho, nome: novo });
    atualizarCaminhoAbas(caminho, dados.caminho);
    remapearExpandidas(caminho, dados.caminho);
    await carregarScripts();
    focarNo(dados.caminho);
    toast(`renamed to ${dados.nome}`, "ok");
  } catch (erro) {
    toast(`could not rename: ${erro.message}`, "erro");
  }
}

export const renomearScript = (nome) => renomearCaminho(`Scripts/${nome}`);

async function duplicarArquivo(caminho) {
  try {
    const dados = await postar("/api/fs/duplicar", { caminho });
    expandirAte(dados.caminho);
    await carregarScripts();
    focarNo(dados.caminho);
    toast(`${dados.nome} criado`, "ok");
  } catch (erro) {
    toast(`could not duplicate: ${erro.message}`, "erro");
  }
}

export async function excluirCaminho(caminho) {
  const pasta = buscarNo(caminho)?.tipo === "pasta";
  const nome = basename(caminho);
  const ok = await dialogo({
    titulo: pasta ? "Delete folder?" : "Delete script?",
    sub: pasta ? `${nome} and everything inside it will be deleted from disk.` : `${nome} will be deleted from disk.`,
    campo: false,
    confirmar: "Delete",
    perigo: true,
  });
  if (!ok) return;
  try {
    await postar("/api/fs/excluir", { caminho });
    atualizarCaminhoAbas(caminho, null);
    remapearExpandidas(caminho, null);
    await carregarScripts();
    toast(`${nome} deleted`, "ok");
  } catch (erro) {
    toast(`could not delete: ${erro.message}`, "erro");
  }
}

export const excluirScript = (nome) => excluirCaminho(`Scripts/${nome}`);

function abrirMover(caminho) {
  const pai = pastaDe(caminho);
  const destinos = listarPastas().filter((p) => p !== pai && !dentroDe(p, caminho));
  if (!destinos.length) {
    toast("there is no other folder to move to", "info");
    return;
  }
  caminhoParaMover = caminho;
  $("#mover-sub").textContent = `Choose the folder for ${basename(caminho)}.`;
  $("#mover-destino").innerHTML = destinos.map((p) => `<option value="${escapar(p)}">${escapar(p)}</option>`).join("");
  abrirModal($("#modal-mover"));
  $("#mover-destino").focus();
}

async function confirmarMover() {
  const origem = caminhoParaMover;
  const destino = $("#mover-destino").value;
  fecharModal($("#modal-mover"));
  caminhoParaMover = null;
  if (!origem || !destino) return;
  try {
    const dados = await postar("/api/fs/mover", { caminho: origem, destino });
    atualizarCaminhoAbas(origem, dados.caminho);
    remapearExpandidas(origem, dados.caminho);
    expandirAte(dados.caminho);
    await carregarScripts();
    focarNo(dados.caminho);
    toast(`moved to ${destino}`, "ok");
  } catch (erro) {
    toast(`could not move: ${erro.message}`, "erro");
  }
}

async function abrirNoGerenciador(caminho) {
  try {
    await postar("/api/fs/abrir", { caminho });
  } catch (erro) {
    toast(`could not open folder: ${erro.message}`, "erro");
  }
}

async function alternarAutoexec(input) {
  const ligado = input.checked;
  try {
    const dados = await postar("/api/autoexec", { ligado });
    autoexec = { ...autoexec, ligado: dados.ligado };
    toast(dados.ligado ? "AutoExec on" : "AutoExec off", "ok");
  } catch (erro) {
    input.checked = !ligado;
    toast(`could not change AutoExec: ${erro.message}`, "erro");
  }
}

function itensDoNo(no) {
  const { caminho } = no;
  const raiz = ehRaiz(caminho);
  const acao = (rotulo, simbolo, fn, perigo = false) => ({ rotulo, simbolo, fn, perigo });
  const renomear = acao("Renomear", "i-lapis", () => renomearCaminho(caminho));
  const excluir = acao("Delete", "i-lixo", () => excluirCaminho(caminho), true);
  if (no.tipo === "pasta") {
    const itens = [
      acao("New script", "i-plus", () => novoScriptEm(caminho)),
      acao("New folder", "i-pasta", () => novaPastaEm(caminho)),
    ];
    if (!raiz) itens.push(renomear);
    itens.push(acao("Open in file manager", "i-pasta", () => abrirNoGerenciador(caminho)));
    return raiz ? itens : [...itens, SEPARADOR, excluir];
  }
  return [
    acao("Open", "i-arquivo", () => abrirCaminho(caminho)),
    renomear,
    acao("Duplicar", "i-copiar", () => duplicarArquivo(caminho)),
    acao("Mover", "i-seta", () => abrirMover(caminho)),
    SEPARADOR,
    excluir,
  ];
}

function fecharMenuArvore() {
  menuAberto?.remove();
  menuAberto = null;
}

function abrirMenuArvore(x, y, itens) {
  fecharMenuArvore();
  const menu = document.createElement("div");
  menu.className = "menu contexto";
  menu.setAttribute("role", "menu");
  menu.innerHTML = itens
    .map((item, i) =>
      item === SEPARADOR
        ? '<div class="menu-sep" role="separator"></div>'
        : `<button type="button" role="menuitem" data-indice="${i}"${item.perigo ? ' class="perigo"' : ""}>${icone(item.simbolo)}${escapar(item.rotulo)}</button>`,
    )
    .join("");
  menu.addEventListener("click", (evento) => {
    const botao = evento.target.closest("[data-indice]");
    if (!botao) return;
    const { fn } = itens[Number(botao.dataset.indice)];
    fecharMenuArvore();
    fn();
  });
  document.body.appendChild(menu);
  menu.style.left = `${Math.max(8, Math.min(x, innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, innerHeight - menu.offsetHeight - 8))}px`;
  menuAberto = menu;
}

const itensDoElemento = (no) => itensDoNo(buscarNo(no.dataset.caminho));

function cliqueNaArvore(evento) {
  const no = evento.target.closest(".no");
  if (!no || evento.target.closest(".no-chave")) return;
  const botaoMais = evento.target.closest(".no-mais");
  if (botaoMais) {
    const { left, bottom } = botaoMais.getBoundingClientRect();
    abrirMenuArvore(left, bottom + 4, itensDoElemento(no));
    return;
  }
  if (no.dataset.tipo === "pasta") alternarPasta(no.dataset.caminho);
  else abrirCaminho(no.dataset.caminho);
}

/** Teclas só valem com foco na própria linha; botão e switch dentro dela têm o seu. */
function teclarArvore(evento) {
  const no = evento.target.closest(".no");
  if (!no || evento.target !== no) return;
  const { caminho } = no.dataset;
  const pasta = no.dataset.tipo === "pasta";
  const expandida = no.getAttribute("aria-expanded") === "true";
  const linhas = $$(".no", $("#lista-arquivos"));
  const i = linhas.indexOf(no);
  const acoes = {
    ArrowDown: () => linhas[i + 1]?.focus(),
    ArrowUp: () => linhas[i - 1]?.focus(),
    ArrowRight: () => {
      if (!pasta) return;
      if (expandida) linhas[i + 1]?.focus();
      else alternarPasta(caminho);
    },
    ArrowLeft: () => {
      if (pasta && expandida) alternarPasta(caminho);
      else no.closest("ul")?.closest("li")?.querySelector(":scope > .no")?.focus();
    },
    Enter: () => (pasta ? alternarPasta(caminho) : abrirCaminho(caminho)),
    F2: () => !ehRaiz(caminho) && renomearCaminho(caminho),
    Delete: () => !ehRaiz(caminho) && excluirCaminho(caminho),
  };
  const acao = acoes[evento.key];
  if (!acao) return;
  evento.preventDefault();
  acao();
}

function ligarArvore() {
  const alvo = $("#lista-arquivos");
  alvo.addEventListener("click", cliqueNaArvore);
  alvo.addEventListener("keydown", teclarArvore);
  alvo.addEventListener("change", (evento) => {
    if (evento.target.matches("[data-autoexec]")) alternarAutoexec(evento.target);
  });
  alvo.addEventListener("focusin", (evento) => {
    if (!evento.target.matches(".no")) return;
    focoArvore = evento.target.dataset.caminho;
    $$(".no", alvo).forEach((n) => {
      n.tabIndex = n === evento.target ? 0 : -1;
    });
  });
  alvo.addEventListener("contextmenu", (evento) => {
    const no = evento.target.closest(".no");
    if (!no) return;
    evento.preventDefault();
    abrirMenuArvore(evento.clientX, evento.clientY, itensDoElemento(no));
  });
  $("#arvore").addEventListener("scroll", fecharMenuArvore);
  document.addEventListener("pointerdown", (evento) => {
    if (menuAberto && !menuAberto.contains(evento.target)) fecharMenuArvore();
  });
  document.addEventListener("keydown", (evento) => {
    if (evento.key === "Escape") fecharMenuArvore();
  });
}

function abrirDialogoAbrir() {
  $("#abrir-caminho").value = "";
  abrirModal($("#modal-abrir"));
  $("#abrir-caminho").focus();
}

async function confirmarAbrir() {
  const caminho = $("#abrir-caminho").value.trim();
  if (!caminho) {
    toast("type a path, e.g. Scripts/sub/x.lua", "erro");
    return;
  }
  if (await abrirCaminho(caminho)) fecharModal($("#modal-abrir"));
}

async function importarArquivo(evento) {
  const entrada = evento.target;
  const arquivo = entrada.files[0];
  entrada.value = "";
  if (!arquivo) return;
  try {
    const texto = await arquivo.text();
    if (await criarArquivo(arquivo.name, texto)) fecharModal($("#modal-abrir"));
  } catch (erro) {
    toast(`could not import ${arquivo.name}: ${erro.message}`, "erro");
  }
}

function ligarDialogosArquivos() {
  $("#abrir-confirmar").addEventListener("click", confirmarAbrir);
  $("#abrir-caminho").addEventListener("keydown", (evento) => {
    if (evento.key === "Enter") confirmarAbrir();
  });
  $("#abrir-pasta").addEventListener("click", () => abrirNoGerenciador($("#abrir-caminho").value.trim() || "Scripts"));
  $("#abrir-importar").addEventListener("click", () => $("#abrir-arquivo").click());
  $("#abrir-arquivo").addEventListener("change", importarArquivo);
  $$("#modal-abrir [data-raiz]").forEach((b) =>
    b.addEventListener("click", () => {
      $("#abrir-caminho").value = `${b.dataset.raiz}/`;
      $("#abrir-caminho").focus();
    }),
  );
  $("#mover-confirmar").addEventListener("click", confirmarMover);
  for (const id of ["modal-abrir", "modal-mover"]) {
    const fundo = $(`#${id}`);
    fundo.addEventListener("click", (evento) => {
      if (evento.target === fundo) fecharModal(fundo);
    });
    $$("[data-fechar]", fundo).forEach((b) => b.addEventListener("click", () => fecharModal(fundo)));
  }
}

export function ligarArquivos() {
  const buscar = adiar(atualizarArvore, 120);
  $("#busca-arquivos").addEventListener("input", (e) => {
    estado.busca = e.target.value;
    buscar();
  });
  $("#btn-novo").addEventListener("click", novoScript);
  $("#btn-novo-vazio").addEventListener("click", novoScript);
  $("#btn-abrir").addEventListener("click", abrirDialogoAbrir);
  $("#btn-tentar-arvore").addEventListener("click", carregarScripts);
  ligarArvore();
  ligarDialogosArquivos();
}
