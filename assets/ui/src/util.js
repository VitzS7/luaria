// Funções pequenas usadas em todo módulo. Tudo que vem do servidor passa por
// `escapar` antes de virar HTML: nome de script ou saída de jogo é dado.

export const $ = (sel, raiz = document) => raiz.querySelector(sel);
export const $$ = (sel, raiz = document) => Array.from(raiz.querySelectorAll(sel));

export const escapar = (texto) =>
  String(texto).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );

export const icone = (id) => `<svg class="icone"><use href="#${id}"/></svg>`;

export const bytes = (n) => {
  if (!n) return "0 B";
  if (n < 1024) return `${n} B`;
  return `${(n / 1024).toFixed(1).replace(".", ",")} KB`;
};

export const plural = (n, um, varios) => `${n} ${n === 1 ? um : varios}`;

/** Data local no formato AAAA-MM-DD: é a chave do mapa de atividade. */
export const diaLocal = (data = new Date()) => {
  const mes = String(data.getMonth() + 1).padStart(2, "0");
  const dia = String(data.getDate()).padStart(2, "0");
  return `${data.getFullYear()}-${mes}-${dia}`;
};

/** "há 6 dias", "há 3 h" — o timestamp do servidor vem em segundos. */
export const tempoRelativo = (segundos) => {
  if (!segundos) return "—";
  const diff = Date.now() / 1000 - segundos;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} h ago`;
  return `${Math.floor(diff / 86400)} days ago`;
};

export const dataCurta = (segundos) => {
  if (!segundos) return "—";
  return new Date(segundos * 1000).toLocaleDateString("en-US", {
    day: "2-digit",
    month: "short",
  });
};

export async function api(rota, opcoes = {}) {
  const resp = await fetch(rota, opcoes);
  const tipo = resp.headers.get("content-type") || "";
  const dados = tipo.includes("json") ? await resp.json() : await resp.text();
  if (!resp.ok) throw new Error((dados && dados.erro) || resp.statusText);
  return dados;
}

export const postar = (rota, corpo) =>
  api(rota, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(corpo),
  });

/** Adia `fn` até o fim das chamadas seguidas (ex.: arrastar um slider). */
export function adiar(fn, ms) {
  let agendado = 0;
  return (...args) => {
    clearTimeout(agendado);
    agendado = setTimeout(() => fn(...args), ms);
  };
}

/**
 * Mede quadros e travamentos durante `ms`. Usa longtask quando o WebKit expõe;
 * senão conta quadros com mais de 50 ms pelo delta do rAF.
 */
export function medirFrames(ms = 3000) {
  const longtask = typeof PerformanceObserver !== "undefined" && PerformanceObserver.supportedEntryTypes?.includes("longtask");
  const duracoes = [];
  const observador = longtask
    ? new PerformanceObserver((lista) => lista.getEntries().forEach((e) => duracoes.push(e.duration)))
    : null;
  observador?.observe({ type: "longtask" });

  return new Promise((resolver) => {
    const deltas = [];
    const inicio = performance.now();
    let ultimo = inicio;
    const quadro = (agora) => {
      deltas.push(agora - ultimo);
      ultimo = agora;
      if (agora - inicio < ms) return requestAnimationFrame(quadro);
      observador?.disconnect();
      const pausas = observador ? duracoes : deltas.filter((d) => d > 50);
      resolver({
        origem: longtask ? "longtask" : "raf",
        quadros: deltas.length,
        fps: Math.round(deltas.length / ((agora - inicio) / 1000)),
        travamentos: pausas.length,
        piorMs: Math.round(Math.max(0, ...pausas)),
      });
    };
    requestAnimationFrame(quadro);
  });
}

window.__perf = { medirFrames };
