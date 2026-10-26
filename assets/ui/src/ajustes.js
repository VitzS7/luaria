// Tela Settings: fundo, superfícies, editor, cliente, atalhos e temas. Cada
// controle aplica ao vivo e persiste pelo módulo `tema` (ui.json).

import { $, $$, escapar, postar, bytes } from "./util.js";
import { estado } from "./estado.js";
import { toast } from "./dialogo.js";
import { anexar } from "./execucao.js";
import {
  aplicarPredefinido,
  efeitosLigados,
  mudarTema,
  PREDEFINIDOS,
  renovarFundo,
  SINTAXE_PADRAO,
  TODOS_TOKENS,
} from "./tema.js";
import { gerarCodigoTema, lerCodigoTema } from "./compartilhar.js";

const LIMITE_FUNDO = 12 << 20;
const MAX_TEMAS = 20;
const HEX = /^#[0-9a-fA-F]{6}$/;

const percentual = (valor) => `${Math.round(valor * 100)}%`;

function marcarSecao(chave) {
  estado.secao = chave;
  $$(".ajustes-item").forEach((b) => b.classList.toggle("ativo", b.dataset.secao === chave));
  $$(".secao").forEach((s) => s.classList.toggle("ativa", s.dataset.secao === chave));
}

/** Liga um slider ao valor do tema: aplica o rótulo e grava no `chave`. */
function ligarFaixa(id, chave, { escala = 1, sufixo = "" } = {}) {
  const input = $(`#${id}`);
  const saida = $(`#${id}-valor`);
  input.addEventListener("input", () => {
    const valor = Number(input.value) / escala;
    saida.textContent = `${input.value}${sufixo}`;
    pintarFaixa(input);
    mudarTema({ [chave]: valor });
  });
}

function sincronizarFaixa(id, valor, { escala = 1, sufixo = "" } = {}) {
  const input = $(`#${id}`);
  const v = Math.round(valor * escala);
  input.value = String(v);
  $(`#${id}-valor`).textContent = `${v}${sufixo}`;
  pintarFaixa(input);
}

/** Preenche o trilho até o ponto do valor; o CSS só lê --preenchido. */
function pintarFaixa(input) {
  const min = Number(input.min);
  const max = Number(input.max);
  const pct = ((Number(input.value) - min) / (max - min)) * 100;
  input.style.setProperty("--preenchido", `${pct}%`);
}

function sincronizarSegmentado(seletor, atributo, valor) {
  $$(`${seletor} button`).forEach((b) => b.classList.toggle("ativo", b.dataset[atributo] === valor));
}

function sincronizarFundo() {
  const tema = estado.tema;
  const url = tema.wallpaper && tema.wallpaper !== "arquivo" ? tema.wallpaper : "";
  $("#fundo-url").value = url;
  $("#btn-fundo-remover").disabled = !tema.wallpaper;
  sincronizarFaixa("fundo-desfoque", tema.wallpaper_blur, { sufixo: "px" });
  sincronizarFaixa("fundo-forca", tema.wallpaper_forca, { sufixo: "%" });
}

function sincronizarSuperficies() {
  const tema = estado.tema;
  sincronizarFaixa("painel-opacidade", tema.panel_opacity, { escala: 100, sufixo: "%" });
  sincronizarFaixa("painel-desfoque", tema.panel_blur, { sufixo: "px" });
  sincronizarFaixa("painel-raio", tema.panel_radius, { sufixo: "px" });
  sincronizarFaixa("janela-opacidade", tema.window_opacity, { escala: 100, sufixo: "%" });
  $("#confirmar-fechar").checked = tema.confirmar_fechar;
  $("#confirmar-excluir").checked = tema.confirmar_excluir;
  $("#efeitos-visuais").checked = efeitosLigados(tema);
  sincronizarSegmentado("#densidade", "densidade", tema.density);
  sincronizarSegmentado("#nav-layout", "nav", tema.nav);
}

function sincronizarEditor() {
  const tema = estado.tema;
  $("#fonte").value = tema.editor_font_family;
  sincronizarFaixa("fonte-tamanho", tema.editor_font_size, { sufixo: "px" });
}

function renderPresets() {
  const alvo = $("#presets");
  alvo.innerHTML = PREDEFINIDOS.map((p) => {
    const t = p.tokens;
    const ativo = estado.tema.preset === p.id ? " ativo" : "";
    return `<button class="preset${ativo}" data-preset="${p.id}" aria-pressed="${ativo ? "true" : "false"}">
      <span class="preset-amostra" aria-hidden="true">
        <i style="background:${t.window}"></i><i style="background:${t.panel}"></i>
        <i style="background:${t.surface}"></i><i style="background:${p.accent}"></i>
      </span>
      <span class="preset-nome">${escapar(p.nome)}</span>
      <span class="preset-desc">${escapar(p.descricao)}</span>
    </button>`;
  }).join("");
  alvo.querySelectorAll(".preset").forEach((b) =>
    b.addEventListener("click", () => {
      const predefinido = PREDEFINIDOS.find((p) => p.id === b.dataset.preset);
      aplicarPredefinido(predefinido);
      renderTemas();
    }),
  );
}

/** Cor que o builder mostra para um campo: a do tema, ou o padrão do modo se o tema não tem. */
function valorDoToken(chave) {
  const { tokens, theme } = estado.tema;
  if (tokens[chave]) return tokens[chave];
  if (chave === "editor_fundo") return tokens.panel;
  return SINTAXE_PADRAO[theme === "light" ? "light" : "dark"][chave];
}

function renderTokens() {
  const alvo = $("#tokens");
  alvo.innerHTML = TODOS_TOKENS.map((t) => {
    const valor = valorDoToken(t.chave).toUpperCase();
    return `<div class="token">
      <input class="token-cor" type="color" value="${escapar(valor)}" data-token="${t.chave}" aria-label="${t.nome}" />
      <span class="token-info"><span class="token-nome">${t.nome}</span><span class="token-desc">${t.desc}</span></span>
      <input class="token-hex mono" value="${escapar(valor)}" data-hex="${t.chave}" maxlength="7" spellcheck="false" aria-label="${t.nome} in hex" />
    </div>`;
  }).join("");
  alvo.querySelectorAll("[data-token]").forEach((input) =>
    input.addEventListener("input", () => definirToken(input.dataset.token, input.value)),
  );
  alvo.querySelectorAll("[data-hex]").forEach((input) =>
    input.addEventListener("change", () => definirToken(input.dataset.hex, input.value.trim())),
  );
}

function definirToken(chave, valor) {
  if (!HEX.test(valor)) return;
  const cor = valor.toUpperCase();
  mudarTema({ tokens: { ...estado.tema.tokens, [chave]: cor } });
  $(`[data-token="${chave}"]`).value = cor;
  $(`[data-hex="${chave}"]`).value = cor;
  renderPresets();
}

function sincronizarEditorTema() {
  $("#tema-acento").value = estado.tema.accent;
  $("#tema-acento-hex").value = estado.tema.accent.toUpperCase();
  $("#tema-modo").value = estado.tema.theme;
  renderTokens();
}

function renderTemas() {
  renderPresets();
  sincronizarEditorTema();
  renderTemasSalvos();
}

function renderTemasSalvos() {
  const alvo = $("#temas-salvos");
  const lista = estado.tema.temas_personalizados;
  if (!lista.length) {
    alvo.innerHTML = `<p class="vazio-linha">No saved themes. Adjust the colors above and click “Save as theme”.</p>`;
    return;
  }
  alvo.innerHTML = `<h3 class="rotulo">Your themes</h3>` + lista.map((t, i) => `
    <div class="tema-salvo">
      <span class="tema-salvo-amostra" style="background:${escapar(t.tokens.panel)};box-shadow:inset 0 0 0 5px ${escapar(t.accent)}"></span>
      <span class="tema-salvo-nome">${escapar(t.nome)}</span>
      <button class="btn" data-aplicar="${i}">Aplicar</button>
      <button class="btn-icone perigo" data-excluir="${i}" aria-label="Delete theme ${escapar(t.nome)}">×</button>
    </div>`).join("");
  alvo.querySelectorAll("[data-aplicar]").forEach((b) =>
    b.addEventListener("click", () => aplicarSalvo(lista[Number(b.dataset.aplicar)])),
  );
  alvo.querySelectorAll("[data-excluir]").forEach((b) =>
    b.addEventListener("click", () => excluirSalvo(Number(b.dataset.excluir))),
  );
}

function aplicarSalvo(tema) {
  mudarTema({ accent: tema.accent, theme: tema.theme, tokens: { ...tema.tokens }, preset: "personalizado" });
  renderTemas();
}

function excluirSalvo(indice) {
  const lista = estado.tema.temas_personalizados.filter((_, i) => i !== indice);
  mudarTema({ temas_personalizados: lista });
  renderTemasSalvos();
}

function salvarComoTema() {
  const nome = $("#tema-nome").value.trim() || `Theme ${estado.tema.temas_personalizados.length + 1}`;
  if (estado.tema.temas_personalizados.length >= MAX_TEMAS) {
    toast(`limit of ${MAX_TEMAS} saved themes; delete one first`, "erro");
    return;
  }
  const novo = {
    nome,
    accent: estado.tema.accent,
    theme: estado.tema.theme,
    tokens: { ...estado.tema.tokens },
  };
  mudarTema({ temas_personalizados: [...estado.tema.temas_personalizados, novo], preset: "personalizado" });
  toast(`theme “${nome}” saved`, "ok");
  renderTemasSalvos();
}

function ligarEditorTema() {
  $("#tema-acento").addEventListener("input", (e) => {
    $("#tema-acento-hex").value = e.target.value.toUpperCase();
    mudarTema({ accent: e.target.value.toUpperCase() });
    renderPresets();
  });
  $("#tema-acento-hex").addEventListener("change", (e) => {
    const valor = e.target.value.trim();
    if (!HEX.test(valor)) return toast("use the #RRGGBB format", "erro");
    mudarTema({ accent: valor.toUpperCase() });
    $("#tema-acento").value = valor;
    renderPresets();
  });
  $("#tema-modo").addEventListener("change", (e) => {
    mudarTema({ theme: e.target.value });
    renderPresets();
  });
  $("#btn-salvar-tema").addEventListener("click", salvarComoTema);
  $("#btn-exportar-tema").addEventListener("click", exportarTema);
  $("#btn-importar-tema").addEventListener("click", importarTema);
}

async function exportarTema() {
  const tema = estado.tema;
  const codigo = gerarCodigoTema({
    nome: $("#tema-nome").value.trim() || "Theme",
    accent: tema.accent,
    theme: tema.theme,
    tokens: tema.tokens,
  });
  $("#tema-codigo").value = codigo;
  try {
    await navigator.clipboard.writeText(codigo);
    toast("code copied", "ok");
  } catch {
    toast("code generated: copy it from the box below", "info");
  }
}

function importarTema() {
  try {
    const importado = lerCodigoTema($("#tema-codigo").value);
    mudarTema({
      accent: importado.accent,
      theme: importado.theme,
      tokens: { ...estado.tema.tokens, ...importado.tokens },
      preset: "personalizado",
    });
    if (importado.nome) $("#tema-nome").value = importado.nome;
    renderTemas();
    renderTokens();
    toast(`theme${importado.nome ? ` “${importado.nome}”` : ""} imported`, "ok");
  } catch (erro) {
    toast(erro.message, "erro");
  }
}

function ligarFundo() {
  $("#fundo-url").addEventListener("change", (e) => {
    const url = e.target.value.trim();
    if (url && !/^https?:\/\/\S+$/.test(url)) return toast("the image needs an http(s) URL", "erro");
    mudarTema({ wallpaper: url });
    sincronizarFundo();
  });
  $("#btn-fundo-enviar").addEventListener("click", () => $("#fundo-arquivo").click());
  $("#fundo-arquivo").addEventListener("change", (e) => enviarFundo(e.target.files[0]));
  $("#btn-fundo-remover").addEventListener("click", removerFundo);
  ligarFaixa("fundo-desfoque", "wallpaper_blur", { sufixo: "px" });
  ligarFaixa("fundo-forca", "wallpaper_forca", { sufixo: "%" });
}

async function enviarFundo(arquivo) {
  if (!arquivo) return;
  if (arquivo.size > LIMITE_FUNDO) return toast(`image is ${bytes(arquivo.size)}; the limit is 12 MB`, "erro");
  try {
    const resposta = await fetch("/api/fundo", {
      method: "POST",
      headers: { "Content-Type": arquivo.type || "application/octet-stream" },
      body: arquivo,
    });
    const dados = await resposta.json();
    if (!resposta.ok) throw new Error(dados.erro || resposta.statusText);
    renovarFundo();
    mudarTema({ wallpaper: "arquivo" });
    sincronizarFundo();
    toast("background updated", "ok");
  } catch (erro) {
    toast(`could not upload the background: ${erro.message}`, "erro");
  }
}

async function removerFundo() {
  try {
    await postar("/api/fundo/remover", {});
    mudarTema({ wallpaper: "" });
    sincronizarFundo();
  } catch (erro) {
    toast(`could not remove the background: ${erro.message}`, "erro");
  }
}

function ligarSuperficies() {
  ligarFaixa("painel-opacidade", "panel_opacity", { escala: 100, sufixo: "%" });
  ligarFaixa("painel-desfoque", "panel_blur", { sufixo: "px" });
  ligarFaixa("painel-raio", "panel_radius", { sufixo: "px" });
  ligarFaixa("janela-opacidade", "window_opacity", { escala: 100, sufixo: "%" });
  $("#efeitos-visuais").addEventListener("change", (e) => mudarTema({ efeitos: e.target.checked }));
  $$("#densidade button").forEach((b) =>
    b.addEventListener("click", () => {
      mudarTema({ density: b.dataset.densidade });
      sincronizarSuperficies();
    }),
  );
  $$("#nav-layout button").forEach((b) =>
    b.addEventListener("click", () => {
      mudarTema({ nav: b.dataset.nav });
      sincronizarSuperficies();
    }),
  );
}

function ligarConfirmacoes() {
  for (const [id, chave] of [
    ["confirmar-fechar", "confirmar_fechar"],
    ["confirmar-excluir", "confirmar_excluir"],
  ]) {
    $(`#${id}`).addEventListener("change", (e) => mudarTema({ [chave]: e.target.checked }));
  }
}

function ligarEditor() {
  $("#fonte").addEventListener("change", (e) => mudarTema({ editor_font_family: e.target.value }));
  ligarFaixa("fonte-tamanho", "editor_font_size", { sufixo: "px" });
}

export function renderAjustes() {
  sincronizarFundo();
  sincronizarSuperficies();
  sincronizarEditor();
  renderTemas();
}

export function ligarAjustes() {
  $$(".ajustes-item").forEach((b) => b.addEventListener("click", () => marcarSecao(b.dataset.secao)));
  marcarSecao(estado.secao);
  $("#btn-anexar").addEventListener("click", anexar);
  ligarFundo();
  ligarSuperficies();
  ligarConfirmacoes();
  ligarEditor();
  ligarEditorTema();
}
