// Diálogos de confirmação/entrada, toasts e o helper de modal. Um diálogo por
// vez: abrir outro resolve o anterior com "cancelado".

import { $, escapar, icone } from "./util.js";

let resolverDialogo = null;

export function abrirModal(el) {
  el.hidden = false;
  el.dataset.aberto = "sim";
}

export function fecharModal(el) {
  el.hidden = true;
  delete el.dataset.aberto;
}

export const modalAberto = () => document.querySelector('.modal-fundo[data-aberto="sim"]');

export function dialogo({ titulo, sub = "", valor = "", campo = true, confirmar = "Confirm", perigo = false }) {
  if (resolverDialogo) resolverDialogo(false);
  const fundo = $("#dialogo-fundo");
  const entrada = $("#dialogo-campo");
  const botao = $("#dialogo-confirmar");

  $("#dialogo-titulo").textContent = titulo;
  $("#dialogo-sub").textContent = sub;
  entrada.hidden = !campo;
  entrada.value = valor;
  botao.textContent = confirmar;
  botao.className = `btn ${perigo ? "btn-perigo" : "btn-primario"}`;
  abrirModal(fundo);

  return new Promise((resolve) => {
    resolverDialogo = resolve;
    setTimeout(() => foco(campo, entrada, botao, valor), 20);
  });
}

function foco(campo, entrada, botao, valor) {
  if (!campo) return botao.focus();
  entrada.focus();
  // Seleciona só o nome sem extensão: o usuário quer trocar o nome, não o `.lua`.
  const ponto = valor.lastIndexOf(".");
  entrada.setSelectionRange(0, ponto > 0 ? ponto : valor.length);
}

export function fecharDialogo(resultado) {
  fecharModal($("#dialogo-fundo"));
  const resolver = resolverDialogo;
  resolverDialogo = null;
  if (resolver) resolver(resultado);
}

export function confirmarDialogo() {
  const campo = $("#dialogo-campo");
  const valor = campo.hidden ? true : campo.value.trim();
  if (!campo.hidden && !valor) return;
  fecharDialogo(valor);
}

const ICONES_TOAST = { erro: "i-alerta", ok: "i-check", info: "i-terminal" };

/** Toast some sozinho; erro fica um pouco mais, para dar tempo de ler. */
export function toast(texto, tipo = "info") {
  const alvo = $("#toasts");
  const el = document.createElement("div");
  el.className = `toast ${tipo}`;
  el.innerHTML = `${icone(ICONES_TOAST[tipo] || "i-terminal")}<span>${escapar(texto)}</span>`;
  alvo.appendChild(el);
  const duracao = tipo === "erro" ? 5200 : 3000;
  setTimeout(() => {
    el.classList.add("saindo");
    setTimeout(() => el.remove(), 200);
  }, duracao);
}
