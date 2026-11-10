// Agenda de Filmes — front-end (JavaScript puro).
// Regra de ouro: quem valida é a API. O front só exibe as mensagens que ela devolve,
// para que todos os cenários de erro possam ser testados também pela interface.

const API = '/api';
const LIMITE_DIARIO_MIN = 480;

const estado = {
  hoje: null,        // data "de hoje" segundo o servidor
  data: null,        // dia exibido na agenda (AAAA-MM-DD)
  filmes: [],        // catálogo completo (para o select de sessões)
  editandoId: null,  // id do filme em edição, ou null
};

const $ = (id) => document.getElementById(id);
const porTestId = (id) => document.querySelector(`[data-testid="${id}"]`);

// ===== Comunicação com a API =====
async function api(caminho, opcoes = {}) {
  const res = await fetch(API + caminho, {
    headers: { 'Content-Type': 'application/json' },
    ...opcoes,
  });
  const corpo = res.status === 204 ? null : await res.json().catch(() => null);
  if (!res.ok) {
    const erro = new Error((corpo && corpo.erro) || `Erro ${res.status} ao falar com o servidor`);
    erro.campo = corpo && corpo.campo;
    erro.status = res.status;
    throw erro;
  }
  return corpo;
}

// ===== Mensagens e erros de campo =====
function mostrarMensagem(texto, tipo = 'sucesso') {
  const caixa = $('mensagem');
  caixa.textContent = texto;
  caixa.className = `mensagem mensagem-${tipo}`;
  caixa.dataset.tipo = tipo;
}

function limparErros(prefixo) {
  document.querySelectorAll(`[data-testid^="erro-${prefixo}-"]`).forEach((el) => { el.textContent = ''; });
  document.querySelectorAll(`#form-${prefixo} .invalido`).forEach((el) => {
    el.classList.remove('invalido');
    el.removeAttribute('aria-invalid');
  });
}

function mostrarErro(prefixo, erro) {
  mostrarMensagem(erro.message, 'erro');
  if (!erro.campo) return;
  const alvo = porTestId(`erro-${prefixo}-${erro.campo}`);
  if (!alvo) return;
  alvo.textContent = erro.message;
  const entrada = alvo.parentElement.querySelector('input, select, textarea');
  if (entrada) {
    entrada.classList.add('invalido');
    entrada.setAttribute('aria-invalid', 'true');
  }
}

// ===== Utilidades de data =====
function somarDias(iso, dias) {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + dias);
  return d.toISOString().slice(0, 10);
}

// "segunda-feira, 9 de novembro de 2026" → "Segunda-feira, 9 de novembro de 2026"
function porExtenso(iso) {
  const texto = new Date(`${iso}T00:00:00Z`).toLocaleDateString('pt-BR', {
    weekday: 'long', day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC',
  });
  return texto.charAt(0).toUpperCase() + texto.slice(1);
}

// Cria um elemento com atributos e filhos (texto sempre via textContent: sem risco de XSS)
function el(tag, atributos = {}, ...filhos) {
  const elemento = document.createElement(tag);
  Object.entries(atributos).forEach(([chave, valor]) => {
    if (chave === 'testid') elemento.dataset.testid = valor;
    else if (chave === 'class') elemento.className = valor;
    else if (chave.startsWith('on')) elemento.addEventListener(chave.slice(2), valor);
    else elemento.setAttribute(chave, valor);
  });
  filhos.flat().forEach((filho) => {
    if (filho == null) return;
    elemento.append(filho instanceof Node ? filho : document.createTextNode(String(filho)));
  });
  return elemento;
}

// ===== Abas =====
function mostrarAba(nome) {
  const agenda = nome === 'agenda';
  $('secao-agenda').classList.toggle('oculto', !agenda);
  $('secao-catalogo').classList.toggle('oculto', agenda);
  $('aba-agenda').classList.toggle('ativa', agenda);
  $('aba-catalogo').classList.toggle('ativa', !agenda);
  $('aba-agenda').setAttribute('aria-selected', String(agenda));
  $('aba-catalogo').setAttribute('aria-selected', String(!agenda));
}

// ===== Agenda =====
async function irParaData(iso) {
  estado.data = iso;
  history.replaceState(null, '', `#${iso}`);
  $('data-agenda').value = iso;
  $('data-extenso').textContent = porExtenso(iso);

  const selo = $('selo-dia');
  const texto = iso === estado.hoje ? 'Hoje' : iso < estado.hoje ? 'Passado' : 'Futuro';
  selo.textContent = texto;
  selo.className = `selo selo-${texto.toLowerCase()}`;
  await carregarAgenda();
}

async function carregarAgenda() {
  try {
    const [sessoes, resumo] = await Promise.all([
      api(`/sessoes?data=${estado.data}`),
      api(`/resumo?data=${estado.data}`),
    ]);
    renderizarSessoes(sessoes);
    renderizarResumo(resumo);
  } catch (erro) {
    mostrarMensagem(`Não foi possível carregar a agenda: ${erro.message}`, 'erro');
  }
}

function renderizarResumo(r) {
  $('resumo-minutos').textContent = `${r.minutos_agendados} / ${r.limite_min} min`;
  $('resumo-livres').textContent = `${r.minutos_livres} min livres`;
  $('resumo-planejadas').textContent = r.planejadas;
  $('resumo-assistidas').textContent = r.assistidas;
  $('resumo-canceladas').textContent = r.canceladas;
  $('resumo-nota-media').textContent = r.nota_media == null ? '—' : r.nota_media.toFixed(1).replace('.', ',');
  const pct = Math.min(100, (r.minutos_agendados / r.limite_min) * 100);
  $('resumo-barra-preenchida').style.width = `${pct}%`;
  $('resumo-barra').setAttribute('aria-valuenow', r.minutos_agendados);
  $('resumo-barra').classList.toggle('cheia', r.minutos_livres === 0);
}

const ROTULO_STATUS = { planejada: 'Planejada', assistida: 'Assistida', cancelada: 'Cancelada' };

function renderizarSessoes(sessoes) {
  const lista = $('lista-sessoes');
  lista.replaceChildren(...sessoes.map(criarItemSessao));
  $('agenda-vazia').classList.toggle('oculto', sessoes.length > 0);
  $('total-sessoes').textContent = sessoes.length;
}

function criarItemSessao(s) {
  const acoes = el('div', { class: 'acoes-sessao' });
  if (s.status === 'planejada') {
    acoes.append(
      el('button', { class: 'btn-sucesso', testid: 'btn-assistir', onclick: () => mudarStatus(s, 'assistida') }, '✓ Assistida'),
      el('button', { class: 'btn-secundario', testid: 'btn-cancelar', onclick: () => mudarStatus(s, 'cancelada') }, 'Cancelar'),
    );
  }
  if (s.status === 'cancelada') {
    acoes.append(el('button', { class: 'btn-secundario', testid: 'btn-reativar', onclick: () => mudarStatus(s, 'planejada') }, 'Reativar'));
  }
  acoes.append(el('button', { class: 'btn-perigo', testid: 'btn-excluir-sessao', onclick: () => excluirSessao(s) }, 'Excluir'));

  const item = el('li', { class: `sessao sessao-${s.status}`, testid: `sessao-${s.id}`, 'data-status': s.status },
    el('div', { class: 'sessao-horario', testid: 'sessao-horario' }, `${s.inicio} – ${s.fim}`),
    el('div', { class: 'sessao-info' },
      el('strong', { class: 'sessao-titulo', testid: 'sessao-titulo' }, s.titulo),
      el('span', { class: 'texto-suave' }, `${s.genero} · ${s.duracao_min} min`),
      s.nota ? el('span', { class: 'estrelas', testid: 'sessao-nota', title: `Nota ${s.nota}` }, '★'.repeat(s.nota) + '☆'.repeat(5 - s.nota)) : null,
      s.comentario ? el('em', { class: 'sessao-comentario', testid: 'sessao-comentario' }, `“${s.comentario}”`) : null,
    ),
    el('span', { class: `status status-${s.status}`, testid: 'sessao-status' }, ROTULO_STATUS[s.status]),
    acoes,
  );
  if (s.status === 'assistida') item.append(criarFormAvaliacao(s));
  return item;
}

function criarFormAvaliacao(s) {
  const nota = el('select', { testid: 'avaliar-nota', 'aria-label': 'Nota' },
    el('option', { value: '' }, 'Sem nota'),
    [1, 2, 3, 4, 5].map((n) => el('option', { value: n }, `${n} ★`)));
  nota.value = s.nota || '';
  const comentario = el('input', { type: 'text', testid: 'avaliar-comentario', placeholder: 'Comentário (até 280 caracteres)', 'aria-label': 'Comentário' });
  comentario.value = s.comentario || '';

  return el('form', { class: 'avaliacao', testid: 'form-avaliacao', novalidate: '', onsubmit: async (e) => {
    e.preventDefault();
    try {
      await api(`/sessoes/${s.id}`, {
        method: 'PATCH',
        body: JSON.stringify({ nota: nota.value ? Number(nota.value) : null, comentario: comentario.value }),
      });
      mostrarMensagem('Avaliação salva.');
      await carregarAgenda();
    } catch (erro) {
      mostrarMensagem(erro.message, 'erro');
    }
  } }, nota, comentario, el('button', { type: 'submit', class: 'btn-secundario', testid: 'btn-salvar-avaliacao' }, 'Salvar avaliação'));
}

const MSG_STATUS = {
  assistida: (t) => `"${t}" marcado como assistido.`,
  cancelada: (t) => `Sessão de "${t}" cancelada.`,
  planejada: (t) => `Sessão de "${t}" reativada.`,
};

async function mudarStatus(sessao, status) {
  try {
    await api(`/sessoes/${sessao.id}`, { method: 'PATCH', body: JSON.stringify({ status }) });
    mostrarMensagem(MSG_STATUS[status](sessao.titulo));
    await carregarAgenda();
  } catch (erro) {
    mostrarMensagem(erro.message, 'erro');
  }
}

async function excluirSessao(sessao) {
  try {
    await api(`/sessoes/${sessao.id}`, { method: 'DELETE' });
    mostrarMensagem(`Sessão de "${sessao.titulo}" excluída.`);
    await Promise.all([carregarAgenda(), carregarFilmes()]);
  } catch (erro) {
    mostrarMensagem(erro.message, 'erro');
  }
}

async function agendar(evento) {
  evento.preventDefault();
  limparErros('sessao');
  const filmeId = $('sessao-filme').value;
  const corpo = {
    filme_id: filmeId ? Number(filmeId) : null,
    data: estado.data,
    inicio: $('sessao-inicio').value || null,
  };
  try {
    const sessao = await api('/sessoes', { method: 'POST', body: JSON.stringify(corpo) });
    mostrarMensagem(`"${sessao.titulo}" agendado das ${sessao.inicio} às ${sessao.fim}.`);
    $('form-sessao').reset();
    await Promise.all([carregarAgenda(), carregarFilmes()]);
  } catch (erro) {
    mostrarErro('sessao', erro);
  }
}

// ===== Catálogo =====
async function carregarFilmes() {
  try {
    estado.filmes = await api('/filmes');
    preencherSelectFilmes();
    await renderizarCatalogo();
  } catch (erro) {
    mostrarMensagem(`Não foi possível carregar o catálogo: ${erro.message}`, 'erro');
  }
}

function preencherSelectFilmes() {
  const select = $('sessao-filme');
  const escolhido = select.value;
  select.replaceChildren(
    el('option', { value: '' }, 'Selecione um filme...'),
    ...estado.filmes.map((f) => el('option', { value: f.id }, `${f.titulo} (${f.ano}) · ${f.duracao_min} min`)),
  );
  select.value = estado.filmes.some((f) => String(f.id) === escolhido) ? escolhido : '';
}

async function renderizarCatalogo() {
  const busca = $('busca-filme').value.trim();
  const genero = $('filtro-genero').value;
  const params = new URLSearchParams();
  if (busca) params.set('busca', busca);
  if (genero) params.set('genero', genero);
  const filmes = params.toString() ? await api(`/filmes?${params}`) : estado.filmes;

  $('lista-filmes').replaceChildren(...filmes.map((f) => el('tr', { testid: `filme-${f.id}` },
    el('td', { testid: 'filme-titulo-celula' }, f.titulo),
    el('td', {}, f.ano),
    el('td', {}, f.genero),
    el('td', {}, `${f.duracao_min} min`),
    el('td', { testid: 'filme-sessoes' }, f.total_sessoes),
    el('td', { class: 'acoes-tabela' },
      el('button', { class: 'btn-secundario', testid: 'btn-editar-filme', onclick: () => editarFilme(f) }, 'Editar'),
      el('button', { class: 'btn-perigo', testid: 'btn-excluir-filme', onclick: () => excluirFilme(f) }, 'Excluir'),
    ),
  )));
  $('catalogo-vazio').classList.toggle('oculto', filmes.length > 0);
  $('total-filmes').textContent = `${filmes.length} ${filmes.length === 1 ? 'filme' : 'filmes'}`;
}

function lerFormFilme() {
  const numero = (id) => ($(id).value.trim() === '' ? null : Number($(id).value));
  return {
    titulo: $('filme-titulo').value,
    ano: numero('filme-ano'),
    genero: $('filme-genero').value || null,
    duracao_min: numero('filme-duracao'),
    sinopse: $('filme-sinopse').value,
  };
}

async function salvarFilme(evento) {
  evento.preventDefault();
  limparErros('filme');
  const editando = estado.editandoId;
  try {
    const filme = await api(editando ? `/filmes/${editando}` : '/filmes', {
      method: editando ? 'PUT' : 'POST',
      body: JSON.stringify(lerFormFilme()),
    });
    mostrarMensagem(editando ? `"${filme.titulo}" atualizado.` : `"${filme.titulo}" adicionado ao catálogo.`);
    sairDaEdicao();
    await carregarFilmes();
  } catch (erro) {
    mostrarErro('filme', erro);
  }
}

function editarFilme(f) {
  estado.editandoId = f.id;
  $('filme-titulo').value = f.titulo;
  $('filme-ano').value = f.ano;
  $('filme-genero').value = f.genero;
  $('filme-duracao').value = f.duracao_min;
  $('filme-sinopse').value = f.sinopse || '';
  $('form-filme-titulo').textContent = `Editar filme: ${f.titulo}`;
  $('btn-cancelar-edicao').classList.remove('oculto');
  limparErros('filme');
  $('filme-titulo').focus();
}

function sairDaEdicao() {
  estado.editandoId = null;
  $('form-filme').reset();
  $('form-filme-titulo').textContent = 'Novo filme';
  $('btn-cancelar-edicao').classList.add('oculto');
  limparErros('filme');
}

async function excluirFilme(f) {
  try {
    await api(`/filmes/${f.id}`, { method: 'DELETE' });
    mostrarMensagem(`"${f.titulo}" removido do catálogo.`);
    if (estado.editandoId === f.id) sairDaEdicao();
    await carregarFilmes();
  } catch (erro) {
    mostrarMensagem(erro.message, 'erro');
  }
}

async function carregarGeneros() {
  const generos = await api('/generos');
  const opcoes = () => generos.map((g) => el('option', { value: g }, g));
  $('filme-genero').replaceChildren(el('option', { value: '' }, 'Selecione...'), ...opcoes());
  $('filtro-genero').replaceChildren(el('option', { value: '' }, 'Todos os gêneros'), ...opcoes());
}

// ===== Eventos =====
$('aba-agenda').addEventListener('click', () => mostrarAba('agenda'));
$('aba-catalogo').addEventListener('click', () => mostrarAba('catalogo'));
$('dia-anterior').addEventListener('click', () => irParaData(somarDias(estado.data, -1)));
$('dia-seguinte').addEventListener('click', () => irParaData(somarDias(estado.data, 1)));
$('ir-hoje').addEventListener('click', () => irParaData(estado.hoje));
$('data-agenda').addEventListener('change', (e) => { if (e.target.value) irParaData(e.target.value); });
$('form-sessao').addEventListener('submit', agendar);
$('form-filme').addEventListener('submit', salvarFilme);
$('btn-cancelar-edicao').addEventListener('click', sairDaEdicao);
$('filtro-genero').addEventListener('change', renderizarCatalogo);
let buscaTimer;
$('busca-filme').addEventListener('input', () => {
  clearTimeout(buscaTimer);
  buscaTimer = setTimeout(renderizarCatalogo, 300);
});

// ===== Início =====
// Abrir a página com #AAAA-MM-DD na URL mostra direto aquele dia (útil nos testes).
async function iniciar() {
  try {
    estado.hoje = (await api('/hoje')).hoje;
    await carregarGeneros();
    await carregarFilmes();
    const daUrl = location.hash.slice(1);
    await irParaData(/^\d{4}-\d{2}-\d{2}$/.test(daUrl) ? daUrl : estado.hoje);
    document.body.dataset.pronto = 'true'; // sinal para os testes: página carregada
  } catch (erro) {
    mostrarMensagem(`Servidor indisponível: ${erro.message}`, 'erro');
  }
}

iniciar();
