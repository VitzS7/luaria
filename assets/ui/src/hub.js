// Hub online de scripts (robloxscripts.com). Tudo que vem da API é de
// terceiros: entra no DOM só por textContent/atributos validados, nunca innerHTML.

import { $, adiar } from "./util.js";
import { estado } from "./estado.js";
import { abrirModal, dialogo, fecharModal, toast } from "./dialogo.js";
import { abrirAba, ativarAba } from "./abas.js";
import { irPara } from "./navegacao.js";
import { executarCodigo } from "./execucao.js";

const URL_API = "https://robloxscripts.com/api/v1/scripts";
const POR_PAGINA = 24;
const TIMEOUT_MS = 10000;
const ESQUELETOS = 12;
// cards por quadro: com 40 o layout não trava ao chegar uma página cheia
const LOTE_CARTOES = 40;

const ORDENS = [
  ["trending", "Trending"],
  ["newest", "Newest"],
  ["oldest", "Oldest"],
  ["most-liked", "Most liked"],
  ["most-viewed", "Most viewed"],
  ["title-asc", "Title A → Z"],
];
const ACESSOS = [
  ["", "All"],
  ["free", "Free"],
  ["key", "Key"],
  ["paid", "Pago"],
];

const hub = {
  q: "",
  sort: "trending",
  access: "",
  verified: false,
  pagina: 0,
  temMais: false,
  carregando: false,
  requisicao: null,
  iniciado: false,
  itens: [],
  aberto: null,
  confirmados: new Set(),
};

const numero = new Intl.NumberFormat("en-US", { notation: "compact" });

function el(tag, classe, texto) {
  const no = document.createElement(tag);
  if (classe) no.className = classe;
  if (texto !== undefined) no.textContent = texto;
  return no;
}

const texto = (v) => (typeof v === "string" ? v : "");
const nomeJogo = (s) => texto(s.game) || texto(s.games?.[0]) || "Universal";
const autorDe = (s) => texto(s.author?.username) || "desconhecido";

function urlImagem(valor) {
  try {
    const url = new URL(valor);
    return url.protocol === "https:" || url.protocol === "http:" ? url.href : null;
  } catch {
    return null;
  }
}

function selo(classe, rotulo) {
  return el("span", `hub-selo ${classe}`, rotulo);
}

function cartao(script, indice) {
  const art = el("article", "cartao hub-cartao");
  art.tabIndex = 0;
  art.dataset.indice = String(indice);

  const imagem = el("div", "hub-img");
  const src = urlImagem(script.image);
  if (src) {
    const img = document.createElement("img");
    img.loading = "lazy";
    img.decoding = "async";
    img.alt = "";
    img.referrerPolicy = "no-referrer";
    img.addEventListener("load", () => img.classList.add("pronta"));
    img.addEventListener("error", () => img.remove());
    img.src = src;
    imagem.append(img);
  }
  imagem.append(el("span", "hub-fonte", "RobloxScripts"));

  const corpo = el("div", "hub-corpo-cartao");
  const rodape = el("div", "hub-rodape");
  rodape.append(el("span", "hub-curtidas", `♥ ${numero.format(Number(script.likes) || 0)}`));
  if (script.accessType === "paid") rodape.append(selo("paid", "PAID"));
  else if (script.accessType === "key" || script.key) rodape.append(selo("key", "KEY"));
  if (script.author?.verified) rodape.append(selo("ok", "✓"));
  corpo.append(el("h3", "hub-titulo", texto(script.title) || "Untitled"), el("p", "hub-jogo", nomeJogo(script)), rodape);

  art.append(imagem, corpo);
  return art;
}

function esqueleto() {
  const art = el("div", "cartao hub-cartao hub-esqueleto");
  art.setAttribute("aria-hidden", "true");
  const corpo = el("div", "hub-corpo-cartao");
  const titulo = el("div", "hub-titulo");
  titulo.append(el("div", "hub-bloco"));
  const rodape = el("div", "hub-rodape");
  rodape.append(el("div", "hub-bloco"));
  corpo.append(titulo, el("div", "hub-bloco"), rodape);
  art.append(el("div", "hub-img"), corpo);
  return art;
}

function mostrarEstado({ vazio = false, erro = null, mais = false }) {
  $("#hub-vazio").hidden = !vazio;
  $("#hub-erro").hidden = erro === null;
  if (erro !== null) $("#hub-erro-msg").textContent = erro;
  $("#hub-mais").hidden = !mais;
}

function montarUrl(pagina) {
  const p = new URLSearchParams({ limit: String(POR_PAGINA), page: String(pagina), sort: hub.sort });
  if (hub.q) p.set("q", hub.q);
  if (hub.access) p.set("access", hub.access);
  if (hub.verified) p.set("verified", "true");
  return `${URL_API}?${p}`;
}

function mensagemErro(erro, estourou) {
  if (!navigator.onLine) return "No internet connection.";
  if (estourou) return "The Hub did not respond in 10 s.";
  return erro.message || "Could not reach the Hub.";
}

async function buscar(pagina, sinal) {
  const resp = await fetch(montarUrl(pagina), { signal: sinal, headers: { Accept: "application/json" } });
  if (!resp.ok) throw new Error(`Hub responded ${resp.status}`);
  const json = await resp.json();
  if (!Array.isArray(json?.data)) throw new Error("Unexpected response from the Hub");
  return json;
}

function haMaisPaginas(json, pagina) {
  const pg = json.pagination || {};
  const total = pg.totalPages ?? pg.lastPage ?? pg.pages ?? pg.pageCount;
  if (Number.isFinite(total)) return pagina < total;
  return json.data.length >= POR_PAGINA;
}

let filaCartoes = [];
let quadroCartoes = 0;

function esvaziarFilaCartoes() {
  cancelAnimationFrame(quadroCartoes);
  quadroCartoes = 0;
  filaCartoes = [];
}

// Fila única e ordenada: uma página nova chegando antes do fim da anterior não pode pular cards.
function enfileirarCartoes(novos) {
  filaCartoes.push(...novos);
  quadroCartoes ||= requestAnimationFrame(desenharLote);
}

function desenharLote() {
  quadroCartoes = 0;
  const pacote = document.createDocumentFragment();
  filaCartoes.splice(0, LOTE_CARTOES).forEach(({ script, indice }) => pacote.append(cartao(script, indice)));
  $("#hub-grade").append(pacote);
  if (filaCartoes.length) quadroCartoes = requestAnimationFrame(desenharLote);
}

/** `reiniciar` descarta o que está na tela (nova busca/filtro); senão acrescenta a próxima página. */
async function carregar(reiniciar) {
  hub.requisicao?.abort();
  const ctrl = new AbortController();
  hub.requisicao = ctrl;
  let estourou = false;
  const relogio = setTimeout(() => {
    estourou = true;
    ctrl.abort();
  }, TIMEOUT_MS);

  const grade = $("#hub-grade");
  if (reiniciar) {
    hub.pagina = 0;
    hub.itens = [];
    esvaziarFilaCartoes();
    grade.replaceChildren();
  }
  const proxima = hub.pagina + 1;
  hub.carregando = true;
  mostrarEstado({});
  const folhas = Array.from({ length: ESQUELETOS }, esqueleto);
  grade.append(...folhas);

  try {
    const json = await buscar(proxima, ctrl.signal);
    if (hub.requisicao !== ctrl) return;
    folhas.forEach((f) => f.remove());
    const inicio = hub.itens.length;
    hub.itens.push(...json.data);
    hub.pagina = proxima;
    hub.temMais = haMaisPaginas(json, proxima) && json.data.length > 0;
    enfileirarCartoes(json.data.map((script, i) => ({ script, indice: inicio + i })));
    mostrarEstado({ vazio: hub.itens.length === 0, mais: hub.temMais });
  } catch (erro) {
    // abort por nova requisição: quem substituiu já cuida da tela
    if (hub.requisicao !== ctrl) return;
    folhas.forEach((f) => f.remove());
    mostrarEstado({ erro: mensagemErro(erro, estourou) });
  } finally {
    clearTimeout(relogio);
    if (hub.requisicao === ctrl) hub.carregando = false;
  }
}

const recarregar = () => carregar(true);

function opcoesSelect(select, lista, valor) {
  select.replaceChildren(
    ...lista.map(([id, nome]) => {
      const op = el("option", "", nome);
      op.value = id;
      op.selected = id === valor;
      return op;
    }),
  );
}

export function mostrarVisao(visao) {
  const hubAtivo = visao === "hub";
  $("#meus-corpo").hidden = hubAtivo;
  $("#hub-corpo").hidden = !hubAtivo;
  $("#acoes-meus").hidden = hubAtivo;
  $("#acoes-hub").hidden = !hubAtivo;
  document.querySelectorAll("#scripts-visao button").forEach((b) => {
    const ativo = b.dataset.visao === visao;
    b.classList.toggle("ativo", ativo);
    b.setAttribute("aria-pressed", String(ativo));
  });
  if (hubAtivo && !hub.iniciado) {
    hub.iniciado = true;
    recarregar();
  }
}

// ---------------------------- detalhes ----------------------------

function variantesDe(script) {
  const lista = [];
  if (texto(script.script)) lista.push({ nome: "Main", codigo: script.script });
  if (Array.isArray(script.variants)) {
    script.variants.forEach((v, i) => {
      const codigo = typeof v === "string" ? v : texto(v?.script) || texto(v?.code);
      if (!codigo) return;
      const nome = (typeof v === "object" && (texto(v.title) || texto(v.name) || texto(v.label))) || `Version ${i + 1}`;
      lista.push({ nome, codigo });
    });
  }
  return lista;
}

function secao(rotulo, conteudo) {
  const s = el("div");
  s.append(el("div", "hub-det-rotulo", rotulo), conteudo);
  return s;
}

function lista(itens) {
  const ul = el("ul", "hub-det-lista");
  ul.append(...itens.filter((t) => typeof t === "string" && t).map((t) => el("li", "", t)));
  return ul;
}

function abrirDetalhes(script) {
  const variantes = variantesDe(script);
  hub.aberto = { script, variantes, atual: 0 };

  $("#hub-det-titulo").textContent = texto(script.title) || "Untitled";
  const corpo = $("#hub-det-corpo");
  const meta = el("div", "hub-det-meta");
  meta.append(
    el("span", "", `por ${autorDe(script)}${script.author?.verified ? " ✓" : ""}`),
    el("span", "", `· ${nomeJogo(script)}`),
    el("span", "", `· ${numero.format(Number(script.views) || 0)} views`),
    el("span", "", `· ${numero.format(Number(script.executions) || 0)} runs`),
    el("span", "", `· ♥ ${numero.format(Number(script.likes) || 0)}`),
  );
  if (script.accessType === "paid") meta.append(selo("paid", "PAID"));
  else if (script.accessType === "key" || script.key) meta.append(selo("key", "KEY"));

  const partes = [meta];
  if (texto(script.description)) partes.push(el("p", "hub-det-desc", script.description));
  if (Array.isArray(script.features) && script.features.length) partes.push(secao("Features", lista(script.features)));
  if (Array.isArray(script.tags) && script.tags.length) partes.push(secao("Tags", lista(script.tags)));
  corpo.replaceChildren(...partes);

  const seletor = $("#hub-det-variante");
  seletor.hidden = variantes.length < 2;
  opcoesSelect(seletor, variantes.map((v, i) => [String(i), v.nome]), "0");

  const pago = script.accessType === "paid";
  const semCodigo = variantes.length === 0;
  const executar = $("#hub-det-executar");
  executar.disabled = pago || semCodigo;
  executar.querySelector("span").textContent = pago ? "Paid" : "Run";
  $("#hub-det-editor").disabled = semCodigo;
  $("#hub-det-copiar").disabled = semCodigo;
  abrirModal($("#modal-hub"));
}

const codigoAtual = () => hub.aberto?.variantes[hub.aberto.atual]?.codigo ?? "";

async function executarAberto() {
  const { script } = hub.aberto || {};
  const codigo = codigoAtual();
  if (!script || !codigo || script.accessType === "paid") return;
  const chave = String(script.id ?? script.slug ?? script.title);
  if (!hub.confirmados.has(chave)) {
    const ok = await dialogo({
      titulo: "Run third-party script?",
      sub: `Third-party script: ${texto(script.title) || "Untitled"} by ${autorDe(script)}. It runs inside the game with full access.`,
      campo: false,
      confirmar: "Executar",
      perigo: true,
    });
    if (!ok) return;
    hub.confirmados.add(chave);
  }
  executarCodigo({ nome: texto(script.title) || "hub", conteudo: codigo });
}

function nomeUnico(titulo) {
  const base = (titulo.replace(/[^\p{L}\p{N} _.-]+/gu, "").trim().slice(0, 40) || "hub") + ".lua";
  let nome = base;
  for (let n = 2; estado.abas.some((a) => a.nome === nome); n++) nome = base.replace(/\.lua$/, ` (${n}).lua`);
  return nome;
}

function abrirNoEditor() {
  const codigo = codigoAtual();
  if (!codigo) return;
  const nome = nomeUnico(texto(hub.aberto.script.title));
  abrirAba(nome, codigo);
  // rascunho: Salvar pergunta o nome em vez de sobrescrever scripts/ às cegas
  estado.abas = estado.abas.map((a) => (a.id === nome ? { ...a, rascunho: true, sujo: true } : a));
  ativarAba(nome);
  fecharModal($("#modal-hub"));
  irPara("editor");
}

async function copiarAberto() {
  try {
    await navigator.clipboard.writeText(codigoAtual());
    toast("script copied", "ok");
  } catch {
    toast("could not copy", "erro");
  }
}

// ------------------------------ ligação ------------------------------

function abrirPorEvento(alvo) {
  const art = alvo.closest(".hub-cartao:not(.hub-esqueleto)");
  if (art) abrirDetalhes(hub.itens[Number(art.dataset.indice)]);
}

export function ligarHub() {
  opcoesSelect($("#hub-ordem"), ORDENS, hub.sort);
  opcoesSelect($("#hub-acesso"), ACESSOS, hub.access);

  document.querySelectorAll("#scripts-visao button").forEach((b) =>
    b.addEventListener("click", () => mostrarVisao(b.dataset.visao)),
  );

  const buscarDepois = adiar(recarregar, 300);
  $("#hub-busca").addEventListener("input", (e) => {
    hub.q = e.target.value.trim();
    buscarDepois();
  });
  $("#hub-ordem").addEventListener("change", (e) => {
    hub.sort = e.target.value;
    recarregar();
  });
  $("#hub-acesso").addEventListener("change", (e) => {
    hub.access = e.target.value;
    recarregar();
  });
  $("#hub-verificado").addEventListener("change", (e) => {
    hub.verified = e.target.checked;
    recarregar();
  });

  $("#hub-tentar").addEventListener("click", () => carregar(hub.itens.length === 0));
  const maisPagina = () => {
    if (hub.temMais && !hub.carregando) carregar(false);
  };
  $("#hub-carregar-mais").addEventListener("click", maisPagina);
  new IntersectionObserver((entradas) => entradas.some((e) => e.isIntersecting) && maisPagina(), {
    rootMargin: "400px",
  }).observe($("#hub-mais"));
  window.addEventListener("online", () => {
    if (hub.iniciado && !$("#hub-erro").hidden) $("#hub-tentar").click();
  });

  const grade = $("#hub-grade");
  grade.addEventListener("click", (e) => abrirPorEvento(e.target));
  grade.addEventListener("keydown", (e) => e.key === "Enter" && abrirPorEvento(e.target));

  $("#hub-det-variante").addEventListener("change", (e) => {
    if (hub.aberto) hub.aberto.atual = Number(e.target.value);
  });
  $("#hub-det-executar").addEventListener("click", executarAberto);
  $("#hub-det-editor").addEventListener("click", abrirNoEditor);
  $("#hub-det-copiar").addEventListener("click", copiarAberto);
  const modal = $("#modal-hub");
  modal.querySelectorAll("[data-fechar]").forEach((b) => b.addEventListener("click", () => fecharModal(modal)));
  modal.addEventListener("click", (e) => e.target === modal && fecharModal(modal));
}
