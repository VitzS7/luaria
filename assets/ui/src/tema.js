// Aparência: presets, cores, fundo e persistência no ui.json. O servidor é
// quem valida e clampa; aqui só aplicamos o que chegou em CSS variables.

import { adiar, api, postar } from "./util.js";
import { estado } from "./estado.js";

export const TOKENS = [
  { chave: "window", nome: "Window", desc: "window background" },
  { chave: "panel", nome: "Panel", desc: "cards and editor" },
  { chave: "surface", nome: "Surface", desc: "fields and buttons" },
  { chave: "text", nome: "Text", desc: "main text" },
  { chave: "muted", nome: "Muted", desc: "labels and hints" },
  { chave: "border", nome: "Border", desc: "lines and dividers" },
];

// Ardósia azulada: painel translúcido sobre o céu do fundo, texto claro para ler
// sem depender do wallpaper.
/** Cores do editor e da sintaxe. Ficam no tema só quando definidas; sem elas, o modo decide. */
export const TOKENS_EDITOR = [
  { chave: "editor_fundo", nome: "Editor background", desc: "code area" },
  { chave: "sin_keyword", nome: "Keyword", desc: "local, if, function" },
  { chave: "sin_string", nome: "Text", desc: "quoted strings" },
  { chave: "sin_number", nome: "Number", desc: "numeric values" },
  { chave: "sin_comment", nome: "Comment", desc: "lines with --" },
  { chave: "sin_funcao", nome: "Function", desc: "function names" },
  { chave: "sin_builtin", nome: "Builtin", desc: "print, ipairs, game" },
  { chave: "sin_operador", nome: "Operator", desc: "=, .., +, #" },
];

export const TODOS_TOKENS = [...TOKENS, ...TOKENS_EDITOR];

/** Valores que a sintaxe usa quando o tema não define a cor (mesmos de aplicarSintaxe). */
export const SINTAXE_PADRAO = {
  dark: {
    sin_keyword: "#A9D6E5",
    sin_string: "#9BCE8A",
    sin_number: "#E7B75F",
    sin_comment: "#5D6A7A",
    sin_funcao: "#D5E4F6",
    sin_builtin: "#6FD3C4",
    sin_operador: "#93A2B4",
  },
  light: {
    sin_keyword: "#1F5FD0",
    sin_string: "#3F7A34",
    sin_number: "#9A6A12",
    sin_comment: "#8A8F8B",
    sin_funcao: "#243447",
    sin_builtin: "#0F766E",
    sin_operador: "#5A625F",
  },
};

const TOKENS_ESCUROS = {
  window: "#1A2130",
  panel: "#2A3142",
  surface: "#363E52",
  text: "#EEF3F9",
  muted: "#AEB9C9",
  border: "#4B5670",
};

export const PREDEFINIDOS = [
  {
    id: "vidro-escuro",
    nome: "Dark glass",
    descricao: "Default. Smoked panels over the background.",
    skin: "vidro",
    theme: "dark",
    accent: "#8CC4D4",
    tokens: TOKENS_ESCUROS,
  },
  {
    id: "vidro-claro",
    nome: "Light glass",
    descricao: "Same glass, for light setups.",
    skin: "vidro",
    theme: "light",
    accent: "#2F6F86",
    tokens: {
      window: "#E9EDF1",
      panel: "#F7F9FB",
      surface: "#E3E8EE",
      text: "#15191F",
      muted: "#5B6673",
      border: "#CDD5DE",
    },
  },
  {
    id: "amoled",
    nome: "AMOLED",
    descricao: "Pure black, good for OLED screens.",
    skin: "vidro",
    theme: "amoled",
    accent: "#9BD4E4",
    tokens: {
      window: "#000000",
      panel: "#050506",
      surface: "#101012",
      text: "#F2F2F2",
      muted: "#7D7D85",
      border: "#1C1C20",
    },
  },
  {
    id: "aero",
    nome: "Aero",
    descricao: "Classic blue bar, light panels.",
    skin: "aero",
    theme: "light",
    accent: "#2B7BD0",
    tokens: {
      window: "#D9E6F4",
      panel: "#F3F7FC",
      surface: "#E4ECF6",
      text: "#1B2A3A",
      muted: "#5C6E82",
      border: "#A9BCD3",
    },
  },
  {
    id: "grafite",
    nome: "Graphite",
    descricao: "Warm grey with an amber accent.",
    skin: "vidro",
    theme: "dark",
    accent: "#D9A441",
    tokens: {
      window: "#101113",
      panel: "#16181B",
      surface: "#1E2125",
      text: "#ECEDEE",
      muted: "#94989E",
      border: "#2B2F35",
    },
  },
  {
    id: "floresta",
    nome: "Forest",
    descricao: "Dark moss green.",
    skin: "vidro",
    theme: "dark",
    accent: "#59C98B",
    tokens: {
      window: "#070C0A",
      panel: "#0C1310",
      surface: "#131D18",
      text: "#E4F0E9",
      muted: "#89A094",
      border: "#203028",
    },
  },
  {
    id: "dark",
    nome: "Dark",
    descricao: "Neutral black, no color tint.",
    skin: "vidro",
    theme: "dark",
    accent: "#E6E6E6",
    tokens: {
      window: "#0E0E10", panel: "#161618", surface: "#1F1F23", text: "#F2F2F2",
      muted: "#9A9AA3", border: "#2C2C33", editor_fundo: "#121214",
      sin_keyword: "#F5F5F5", sin_string: "#A8D5A2", sin_number: "#E7B75F", sin_comment: "#6B6B75",
      sin_funcao: "#D6D6DE", sin_builtin: "#8EC9E0", sin_operador: "#9A9AA3",
    },
  },
  {
    id: "light",
    nome: "Light",
    descricao: "Light and neutral, with a blue accent.",
    skin: "vidro",
    theme: "light",
    accent: "#2563EB",
    tokens: {
      window: "#E9ECF0", panel: "#FFFFFF", surface: "#EEF1F5", text: "#1A1D23",
      muted: "#5F6672", border: "#D0D6DE", editor_fundo: "#FBFCFD",
      sin_keyword: "#1F5FD0", sin_string: "#3F7A34", sin_number: "#9A6A12", sin_comment: "#8A8F8B",
      sin_funcao: "#243447", sin_builtin: "#0F766E", sin_operador: "#5A625F",
    },
  },
  {
    id: "dracula",
    nome: "Dracula",
    descricao: "Dark purple with pink and lime green.",
    skin: "vidro",
    theme: "dark",
    accent: "#BD93F9",
    tokens: {
      window: "#21222C", panel: "#282A36", surface: "#343746", text: "#F8F8F2",
      muted: "#A4A9C4", border: "#44475A", editor_fundo: "#262833",
      sin_keyword: "#FF79C6", sin_string: "#F1FA8C", sin_number: "#BD93F9", sin_comment: "#6272A4",
      sin_funcao: "#50FA7B", sin_builtin: "#8BE9FD", sin_operador: "#FFB86C",
    },
  },
  {
    id: "nord",
    nome: "Nord",
    descricao: "Cool, restrained arctic blue.",
    skin: "vidro",
    theme: "dark",
    accent: "#88C0D0",
    tokens: {
      window: "#2E3440", panel: "#3B4252", surface: "#434C5E", text: "#ECEFF4",
      muted: "#A3AEC0", border: "#4C566A", editor_fundo: "#353C4A",
      sin_keyword: "#81A1C1", sin_string: "#A3BE8C", sin_number: "#B48EAD", sin_comment: "#616E88",
      sin_funcao: "#88C0D0", sin_builtin: "#8FBCBB", sin_operador: "#D8DEE9",
    },
  },
  {
    id: "monokai",
    nome: "Monokai",
    descricao: "Dark olive with vivid green and magenta.",
    skin: "vidro",
    theme: "dark",
    accent: "#A6E22E",
    tokens: {
      window: "#1E1F1C", panel: "#272822", surface: "#33342D", text: "#F8F8F2",
      muted: "#A6A68E", border: "#49483E", editor_fundo: "#2A2B25",
      sin_keyword: "#F92672", sin_string: "#E6DB74", sin_number: "#AE81FF", sin_comment: "#75715E",
      sin_funcao: "#A6E22E", sin_builtin: "#66D9EF", sin_operador: "#F8F8F2",
    },
  },
  {
    id: "catppuccin",
    nome: "Catppuccin",
    descricao: "Mocha: pastels on a dark background.",
    skin: "vidro",
    theme: "dark",
    accent: "#CBA6F7",
    tokens: {
      window: "#11111B", panel: "#1E1E2E", surface: "#313244", text: "#CDD6F4",
      muted: "#A6ADC8", border: "#45475A", editor_fundo: "#1A1A2A",
      sin_keyword: "#CBA6F7", sin_string: "#A6E3A1", sin_number: "#FAB387", sin_comment: "#6C7086",
      sin_funcao: "#89B4FA", sin_builtin: "#94E2D5", sin_operador: "#89DCEB",
    },
  },
  {
    id: "onedark",
    nome: "One Dark",
    descricao: "Blue-grey with classic blue.",
    skin: "vidro",
    theme: "dark",
    accent: "#61AFEF",
    tokens: {
      window: "#21252B", panel: "#282C34", surface: "#2F343E", text: "#ABB2BF",
      muted: "#7F848E", border: "#3E4451", editor_fundo: "#23272E",
      sin_keyword: "#C678DD", sin_string: "#98C379", sin_number: "#D19A66", sin_comment: "#5C6370",
      sin_funcao: "#61AFEF", sin_builtin: "#E5C07B", sin_operador: "#56B6C2",
    },
  },
  {
    id: "github-dark",
    nome: "GitHub Dark",
    descricao: "Blue-black, made for reading docs.",
    skin: "vidro",
    theme: "dark",
    accent: "#58A6FF",
    tokens: {
      window: "#010409", panel: "#0D1117", surface: "#161B22", text: "#E6EDF3",
      muted: "#8B949E", border: "#30363D", editor_fundo: "#0D1117",
      sin_keyword: "#FF7B72", sin_string: "#A5D6FF", sin_number: "#79C0FF", sin_comment: "#8B949E",
      sin_funcao: "#D2A8FF", sin_builtin: "#FFA657", sin_operador: "#C9D1D9",
    },
  },
];

const FUNDO_PADRAO = "/fundo-padrao.svg";
let versaoFundo = Date.now();

const hexParaRgba = (hex, alfa) => {
  const limpo = String(hex).replace("#", "");
  const r = parseInt(limpo.slice(0, 2), 16);
  const g = parseInt(limpo.slice(2, 4), 16);
  const b = parseInt(limpo.slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alfa})`;
};

/** Texto preto ou branco conforme a luminância do destaque. */
function corSobre(hex) {
  const limpo = String(hex).replace("#", "");
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(limpo.slice(i, i + 2), 16) / 255);
  const luz = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return luz > 0.55 ? "#0B0E12" : "#FFFFFF";
}

/** URL do fundo: padrão embutido, arquivo enviado ou URL http(s) do usuário. */
export function urlDoFundo(tema) {
  if (!tema.wallpaper) return FUNDO_PADRAO;
  if (tema.wallpaper === "arquivo") return `/api/fundo?v=${versaoFundo}`;
  return tema.wallpaper;
}

/** Chamado depois de um upload para o navegador não reaproveitar a imagem antiga. */
export function renovarFundo() {
  versaoFundo = Date.now();
}

function aplicarCores(raiz, tema) {
  const t = tema.tokens;
  const opacidade = tema.panel_opacity;
  raiz.style.setProperty("--janela", t.window);
  raiz.style.setProperty("--painel", hexParaRgba(t.panel, opacidade));
  raiz.style.setProperty("--painel-solido", t.panel);
  raiz.style.setProperty("--superficie", t.surface);
  raiz.style.setProperty("--texto", t.text);
  raiz.style.setProperty("--texto-suave", t.muted);
  raiz.style.setProperty("--borda", t.border);
  raiz.style.setProperty("--acento", tema.accent);
  raiz.style.setProperty("--acento-suave", hexParaRgba(tema.accent, 0.16));
  raiz.style.setProperty("--acento-texto", corSobre(tema.accent));
}

/** Cores de editor/sintaxe que o tema definiu; as ausentes saem para o padrão do modo. */
function aplicarCoresEditor(raiz, tokens) {
  for (const { chave } of TOKENS_EDITOR) {
    const nomeCss = `--${chave.replaceAll("_", "-")}`;
    if (tokens[chave]) raiz.style.setProperty(nomeCss, tokens[chave]);
    else raiz.style.removeProperty(nomeCss);
  }
}

function aplicarSintaxe(raiz, tema) {
  if (tema.theme === "light") {
    Object.entries({
      "--sin-keyword": "#1F5FD0",
      "--sin-string": "#3F7A34",
      "--sin-number": "#9A6A12",
      "--sin-comment": "#8A8F8B",
      "--sin-funcao": "#243447",
      "--sin-builtin": "#0F766E",
      "--sin-operador": "#5A625F",
    }).forEach(([k, v]) => raiz.style.setProperty(k, v));
    return;
  }
  raiz.style.setProperty("--sin-keyword", `color-mix(in srgb, ${tema.accent} 72%, white)`);
  Object.entries({
    "--sin-string": "#9BCE8A",
    "--sin-number": "#E7B75F",
    "--sin-comment": "#5D6A7A",
    "--sin-funcao": "#D5E4F6",
    "--sin-builtin": "#6FD3C4",
    "--sin-operador": "#93A2B4",
  }).forEach(([k, v]) => raiz.style.setProperty(k, v));
}

function aplicarFundo(raiz, tema) {
  raiz.style.setProperty("--fundo-imagem", `url("${urlDoFundo(tema)}")`);
  raiz.style.setProperty("--fundo-desfoque", `${tema.wallpaper_blur}px`);
  raiz.style.setProperty("--fundo-forca", String(tema.wallpaper_forca / 100));
}

/** Sem escolha salva, quem tem "reduzir movimento" no sistema começa com efeitos desligados. */
export function efeitosLigados(tema) {
  return tema.efeitos ?? !matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Aplica o tema inteiro. Tudo que a UI pinta vem daqui, nada fixo no CSS. */
export function aplicarTema(tema) {
  const raiz = document.documentElement;
  const corpo = document.body;
  aplicarCores(raiz, tema);
  aplicarSintaxe(raiz, tema);
  aplicarCoresEditor(raiz, tema.tokens);
  aplicarFundo(raiz, tema);
  raiz.style.setProperty("--raio", `${tema.panel_radius}px`);
  raiz.style.setProperty("--desfoque", `${tema.panel_blur}px`);
  raiz.style.setProperty("--fonte-editor", `${tema.editor_font_size}px`);
  raiz.style.setProperty(
    "--fonte-mono",
    `"${tema.editor_font_family}", "IBM Plex Mono", ui-monospace, monospace`,
  );
  raiz.style.colorScheme = tema.theme === "light" ? "light" : "dark";
  raiz.dataset.tema = tema.theme;
  raiz.dataset.densidade = tema.density;
  raiz.dataset.efeitos = efeitosLigados(tema) ? "on" : "off";
  corpo.dataset.skin = tema.skin;
  corpo.dataset.nav = tema.nav;
  aplicarOpacidadeJanela(tema.window_opacity);
}

let opacidadeEnviada = null;

/** Opacidade nativa da janela: só manda ao servidor quando o valor muda. */
function aplicarOpacidadeJanela(valor) {
  if (valor === undefined || valor === opacidadeEnviada) return;
  opacidadeEnviada = valor;
  postar("/api/janela", { acao: "opacidade", valor }).catch((erro) => {
    console.error("window opacity:", erro);
  });
}

export function carregarTema() {
  return api("/api/tema");
}

/** Mostra "salvo" / "salvando…" / erro no rodapé da aba Ajustes. */
function marcarEstado(texto, classe = "") {
  const alvo = document.getElementById("ajustes-estado");
  if (!alvo) return;
  alvo.textContent = texto;
  alvo.dataset.estado = classe;
}

async function salvarAgora() {
  marcarEstado("saving…", "salvando");
  try {
    const resposta = await postar("/api/tema", estado.tema);
    estado.tema = resposta.tema;
    marcarEstado("saved", "ok");
  } catch (erro) {
    marcarEstado(`error: ${erro.message}`, "erro");
  }
}

const agendarSalvar = adiar(salvarAgora, 350);

/** Altera uma ou mais chaves do tema, aplica já e persiste em seguida. */
export function mudarTema(alteracoes) {
  Object.assign(estado.tema, alteracoes);
  aplicarTema(estado.tema);
  agendarSalvar();
}

/** Troca a base visual (skin, modo, cores) sem mexer nas preferências de layout. */
export function aplicarPredefinido(predefinido) {
  mudarTema({
    preset: predefinido.id,
    skin: predefinido.skin,
    theme: predefinido.theme,
    accent: predefinido.accent,
    tokens: { ...predefinido.tokens },
  });
}

