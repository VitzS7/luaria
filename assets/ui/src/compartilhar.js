// Código de compartilhamento do tema: JSON em base64 (UTF-8). Só cores e modo
// viajam; posição de janela, fundo e preferências ficam de fora de propósito.

import { TODOS_TOKENS } from "./tema.js";

const HEX = /^#[0-9a-fA-F]{6}$/;
const MODOS = ["dark", "light", "amoled"];

function utf8ParaBase64(texto) {
  const bytes = new TextEncoder().encode(texto);
  return btoa(Array.from(bytes, (b) => String.fromCharCode(b)).join(""));
}

function base64ParaUtf8(codigo) {
  const binario = atob(codigo);
  return new TextDecoder().decode(Uint8Array.from(binario, (c) => c.charCodeAt(0)));
}

export function gerarCodigoTema({ nome, accent, theme, tokens }) {
  return utf8ParaBase64(JSON.stringify({ versao: 1, nome, accent, theme, tokens }));
}

/** Valida o código colado. Lança erro legível se não for um tema do Luaria. */
export function lerCodigoTema(codigo) {
  let dados;
  try {
    dados = JSON.parse(base64ParaUtf8(codigo.trim()));
  } catch {
    throw new Error("invalid code: not a base64 theme");
  }
  if (!dados || typeof dados !== "object" || dados.versao !== 1) {
    throw new Error("invalid code: unknown version");
  }
  if (!HEX.test(dados.accent)) throw new Error("invalid code: accent color");
  if (!MODOS.includes(dados.theme)) throw new Error("invalid code: mode");
  const tokens = {};
  for (const { chave } of TODOS_TOKENS) {
    const valor = dados.tokens?.[chave];
    if (HEX.test(valor)) tokens[chave] = valor.toUpperCase();
  }
  if (!Object.keys(tokens).length) throw new Error("code has no colors");
  return {
    nome: typeof dados.nome === "string" ? dados.nome.slice(0, 40) : "",
    accent: dados.accent.toUpperCase(),
    theme: dados.theme,
    tokens,
  };
}
