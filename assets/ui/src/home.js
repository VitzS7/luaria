// Home: mapa de atividade (contagem real de execuções por dia), scripts
// recentes e changelog. Sem dados, cada bloco mostra o próprio estado vazio.

import { $, api, diaLocal, escapar, icone, plural, tempoRelativo } from "./util.js";
import { estado } from "./estado.js";
import { abrirScript } from "./arquivos.js";

const SEMANAS = 53;
const MESES = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const diaRelativo = (base, deslocamento) =>
  new Date(base.getFullYear(), base.getMonth(), base.getDate() + deslocamento);

const contagemDe = (data) => estado.atividade[diaLocal(data)] || 0;

/** Primeiro domingo da grade: 53 semanas terminando na semana atual. */
function inicioDaGrade(hoje) {
  const domingo = diaRelativo(hoje, -hoje.getDay());
  return diaRelativo(domingo, -(SEMANAS - 1) * 7);
}

function nivelDe(count, maximo) {
  if (!count) return 0;
  const razao = count / maximo;
  if (razao <= 0.25) return 1;
  if (razao <= 0.5) return 2;
  if (razao <= 0.75) return 3;
  return 4;
}

function rotuloMeses(inicio, colunas) {
  let anterior = -1;
  const itens = [];
  for (let c = 0; c < colunas; c++) {
    const mes = diaRelativo(inicio, c * 7).getMonth();
    if (mes !== anterior) itens.push(`<span style="grid-column:${c + 1}">${MESES[mes]}</span>`);
    anterior = mes;
  }
  return `<div class="meses" aria-hidden="true">${itens.join("")}</div>`;
}

function grade(hoje, inicio) {
  const counts = [];
  for (let c = 0; c < SEMANAS; c++) {
    for (let r = 0; r < 7; r++) {
      const data = diaRelativo(inicio, c * 7 + r);
      counts.push({ data, count: data > hoje ? null : contagemDe(data) });
    }
  }
  const maximo = Math.max(1, ...counts.map((x) => x.count || 0));
  const celulas = counts
    .map(({ data, count }) => {
      if (count === null) return `<span class="celula futuro" aria-hidden="true"></span>`;
      const texto = `${data.toLocaleDateString("en-US")}: ${plural(count, "run", "runs")}`;
      return `<span class="celula nivel-${nivelDe(count, maximo)}" title="${escapar(texto)}"></span>`;
    })
    .join("");
  return `<div class="heat">${rotuloMeses(inicio, SEMANAS)}<div class="celulas">${celulas}</div></div>`;
}

function barrasSemanais(hoje, inicio) {
  const semanas = [];
  for (let c = 0; c < SEMANAS; c++) {
    let soma = 0;
    for (let r = 0; r < 7; r++) {
      const data = diaRelativo(inicio, c * 7 + r);
      if (data <= hoje) soma += contagemDe(data);
    }
    semanas.push(soma);
  }
  const maximo = Math.max(1, ...semanas);
  const barras = semanas
    .map((s, i) => {
      const alturaPct = Math.round((s / maximo) * 100);
      const data = diaRelativo(inicio, i * 7);
      const titulo = `week of ${data.toLocaleDateString("en-US")}: ${plural(s, "run", "runs")}`;
      return `<span class="semana-barra" style="height:${alturaPct}%" title="${escapar(titulo)}"></span>`;
    })
    .join("");
  return `<div class="semanal"><div class="semanas">${barras}</div>${rotuloMeses(inicio, SEMANAS)}</div>`;
}

/** Série acumulada desde a primeira execução até hoje, em SVG. */
function linhaAcumulada(hoje) {
  const chaves = Object.keys(estado.atividade).sort();
  if (!chaves.length) return "";
  const primeiro = new Date(`${chaves[0]}T00:00:00`);
  const dias = Math.max(1, Math.round((hoje - primeiro) / 86400000));
  const largura = 1000;
  const altura = 200;
  let total = 0;
  let totalFinal = 0;
  const pontos = [];
  for (let d = 0; d <= dias; d++) {
    total += contagemDe(diaRelativo(primeiro, d));
    totalFinal = total;
    pontos.push(total);
  }
  const x = (i) => (dias ? (i / dias) * largura : 0);
  const y = (v) => altura - (totalFinal ? (v / totalFinal) * (altura - 12) : 0) - 6;
  const linha = pontos.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  return `<svg class="acumulado" viewBox="0 0 ${largura} ${altura}" preserveAspectRatio="none" role="img" aria-label="Cumulative runs: ${totalFinal}">
    <polygon points="0,${altura} ${linha} ${largura},${altura}" class="area"/>
    <polyline points="${linha}" class="traco"/>
  </svg><div class="acumulado-rotulo"><span>${primeiro.toLocaleDateString("en-US")}</span><span>${totalFinal} in total</span></div>`;
}

function sequenciaAtual(hoje) {
  let dia = hoje;
  if (!contagemDe(dia)) dia = diaRelativo(hoje, -1);
  let seq = 0;
  while (contagemDe(dia) > 0) {
    seq++;
    dia = diaRelativo(dia, -1);
  }
  return seq;
}

function resumo(hoje) {
  const total = Object.values(estado.atividade).reduce((a, b) => a + b, 0);
  const prefixo = String(hoje.getFullYear());
  const noAno = Object.entries(estado.atividade)
    .filter(([dia]) => dia.startsWith(prefixo))
    .reduce((a, [, n]) => a + n, 0);
  const seq = sequenciaAtual(hoje);
  return `<span><strong>${total}</strong> no total</span>
    <span><strong>${noAno}</strong> em ${prefixo}</span>
    <span><strong>${seq}</strong> ${seq === 1 ? "day in a row" : "days in a row"}</span>`;
}

export function renderAtividade() {
  const corpo = $("#atividade-corpo");
  const hoje = new Date();
  const inicio = inicioDaGrade(hoje);
  const total = Object.values(estado.atividade).reduce((a, b) => a + b, 0);
  const modos = { diario: grade, semanal: barrasSemanais };
  let conteudo;
  if (estado.atividadeModo === "acumulado") conteudo = linhaAcumulada(hoje);
  else conteudo = modos[estado.atividadeModo](hoje, inicio);
  corpo.dataset.vazio = total ? "nao" : "sim";
  corpo.innerHTML = total
    ? conteudo
    : `<div class="atividade-vazio"><p>No runs recorded yet.</p><span>Each successful run colors today.</span></div>${grade(hoje, inicio)}`;
  $("#atividade-resumo").innerHTML = total ? resumo(hoje) : "";
  document.querySelectorAll("#atividade-modo button").forEach((b) =>
    b.classList.toggle("ativo", b.dataset.modo === estado.atividadeModo),
  );
}


export async function carregarAtividade() {
  try {
    estado.atividade = await api("/api/atividade");
  } catch {
    estado.atividade = {};
  }
  renderAtividade();
}

export function renderRecentes() {
  const alvo = $("#recentes");
  const recentes = [...estado.scripts].sort((a, b) => b.modificado - a.modificado).slice(0, 5);
  if (!recentes.length) {
    alvo.innerHTML = `<li class="vazio-linha">No scripts yet. Create one in Scripts.</li>`;
    return;
  }
  alvo.innerHTML = recentes
    .map(
      (s) => `<li><button class="recente" data-nome="${escapar(s.nome)}">
        ${icone("i-arquivo")}<span class="recente-nome">${escapar(s.nome)}</span>
        <span class="recente-meta">em scripts/</span>
        <span class="recente-quando">${tempoRelativo(s.modificado)}</span></button></li>`,
    )
    .join("");
  alvo.querySelectorAll(".recente").forEach((b) =>
    b.addEventListener("click", () => abrirScript(b.dataset.nome)),
  );
}

export async function carregarChangelog() {
  const alvo = $("#changelog");
  let versoes = [];
  try {
    versoes = await api("/api/changelog");
  } catch {
    alvo.innerHTML = `<p class="vazio-linha">Could not read the changelog.</p>`;
    return;
  }
  if (!versoes.length) {
    alvo.innerHTML = `<p class="vazio-linha">No changelog: the project has no CHANGELOG.md.</p>`;
    return;
  }
  alvo.innerHTML = versoes
    .slice(0, 4)
    .map(
      (v, i) => `<article class="versao">
        <header><strong>${escapar(v.versao)}</strong>${i === 0 ? '<span class="selo-ultimo">latest</span>' : ""}</header>
        <time>${escapar(v.data)}</time>
        <ul>${v.notas.slice(0, 3).map((n) => `<li>${escapar(n)}</li>`).join("")}</ul>
      </article>`,
    )
    .join("");
}

export function ligarHome() {
  document.querySelectorAll("#atividade-modo button").forEach((b) =>
    b.addEventListener("click", () => {
      estado.atividadeModo = b.dataset.modo;
      renderAtividade();
    }),
  );
  carregarChangelog();
}
