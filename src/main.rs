//! Agenda de Filmes — organizador diário de sessões de filmes.
//!
//! Backend Rust + Axum + SQLite: serve o front-end (pasta `public/`) e a API
//! REST em `/api`. Os números das regras (RN01, RN02...) seguem
//! `docs/REQUISITOS.md`.

use std::{
    collections::HashMap,
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{rejection::JsonRejection, FromRequest, Path, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use chrono::{Datelike, Local, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir};

// ===== Valores-limite das regras de negócio =====

const LIMITE_DIARIO_MIN: i64 = 480; // RN09
const MINUTOS_NO_DIA: i64 = 1440; // RN08
const TITULO_MAX: usize = 100; // RN01
const SINOPSE_MAX: usize = 500; // RN05
const COMENTARIO_MAX: usize = 280; // RN14
const DURACAO_MIN: i64 = 1; // RN04
const DURACAO_MAX: i64 = 300; // RN04
const ANO_MIN: i64 = 1888; // RN02
const ANOS_NO_FUTURO: i64 = 5; // RN02

const GENEROS: [&str; 11] = [
    "Ação", "Animação", "Aventura", "Comédia", "Documentário", "Drama",
    "Fantasia", "Ficção científica", "Romance", "Suspense", "Terror",
];
const STATUS: [&str; 3] = ["planejada", "assistida", "cancelada"];

const MSG_DATA_INVALIDA: &str = "Data inválida. Use o formato AAAA-MM-DD";
const MSG_HORA_INVALIDA: &str = "Horário inválido. Use o formato HH:MM";

/// Catálogo inicial (carregado quando o banco está vazio).
const FILMES_INICIAIS: [(&str, i64, &str, i64); 12] = [
    ("Ainda Estou Aqui", 2024, "Drama", 137),
    ("A Viagem de Chihiro", 2001, "Animação", 125),
    ("Central do Brasil", 1998, "Drama", 113),
    ("Cidade de Deus", 2002, "Drama", 130),
    ("Corra!", 2017, "Terror", 104),
    ("De Volta para o Futuro", 1985, "Aventura", 116),
    ("Interestelar", 2014, "Ficção científica", 169),
    ("Mad Max: Estrada da Fúria", 2015, "Ação", 120),
    ("O Auto da Compadecida", 2000, "Comédia", 104),
    ("O Poderoso Chefão", 1972, "Drama", 175),
    ("Parasita", 2019, "Suspense", 132),
    ("Toy Story", 1995, "Animação", 81),
];

// ===== Estado =====

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    /// Data "de hoje" fixada para testes. `None` = relógio real.
    hoje_fixo: Arc<Mutex<Option<NaiveDate>>>,
}

impl AppState {
    fn hoje(&self) -> NaiveDate {
        self.hoje_fixo
            .lock()
            .unwrap()
            .unwrap_or_else(|| Local::now().date_naive())
    }
}

// ===== Erros: sempre JSON {"erro": "...", "campo": "..."} =====

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    erro: String,
    campo: Option<&'static str>,
}

impl ApiError {
    fn new(status: StatusCode, erro: impl Into<String>) -> Self {
        Self { status, erro: erro.into(), campo: None }
    }
    fn campo(campo: &'static str, erro: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, erro: erro.into(), campo: Some(campo) }
    }
    fn requisicao(erro: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, erro)
    }
    fn nao_encontrado(erro: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, erro)
    }
    fn conflito(erro: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, erro)
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        eprintln!("Erro no banco: {err}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Erro interno do servidor")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut corpo = json!({ "erro": self.erro });
        if let Some(campo) = self.campo {
            corpo["campo"] = json!(campo);
        }
        (self.status, Json(corpo)).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

/// Extrator de JSON que devolve os erros de corpo no mesmo formato da API.
struct Corpo<T>(T);

impl<S, T> FromRequest<S> for Corpo<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(valor)) => Ok(Corpo(valor)),
            Err(JsonRejection::MissingJsonContentType(_)) => Err(ApiError::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Envie o corpo em JSON (Content-Type: application/json)",
            )),
            Err(JsonRejection::JsonSyntaxError(_)) => Err(ApiError::requisicao("JSON malformado")),
            Err(JsonRejection::JsonDataError(_)) => {
                Err(ApiError::requisicao("Algum campo tem tipo inválido"))
            }
            Err(_) => Err(ApiError::requisicao("Corpo da requisição inválido")),
        }
    }
}

/// Diferencia campo ausente (`None`) de campo enviado como `null` (`Some(None)`).
fn presente<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

// ===== Funções auxiliares (puras, fáceis de testar) =====

fn parse_id(texto: &str) -> ApiResult<i64> {
    texto
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| ApiError::requisicao("O id deve ser um número inteiro positivo"))
}

/// Aceita só `AAAA-MM-DD` com zeros à esquerda e data que existe no calendário.
fn parse_data(texto: &str) -> Option<NaiveDate> {
    let b = texto.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    NaiveDate::parse_from_str(texto, "%Y-%m-%d").ok()
}

/// `HH:MM` (00:00 a 23:59) → minutos desde a meia-noite.
fn parse_hora(texto: &str) -> Option<i64> {
    let b = texto.as_bytes();
    let formato_ok = b.len() == 5
        && b[2] == b':'
        && b.iter().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit());
    if !formato_ok {
        return None;
    }
    let h: i64 = texto[0..2].parse().ok()?;
    let m: i64 = texto[3..5].parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

fn fmt_hora(minutos: i64) -> String {
    format!("{:02}:{:02}", minutos / 60, minutos % 60)
}

/// Intervalos [início, fim). Encostar (um termina quando o outro começa) não é sobreposição.
fn sobrepoe(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Remove espaços das pontas; texto vazio vira `None`.
fn texto_opcional(valor: Option<String>) -> Option<String> {
    valor.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Aceita o gênero sem diferenciar maiúsculas e devolve a grafia oficial.
fn normalizar_genero(genero: &str) -> Option<&'static str> {
    let alvo = genero.trim().to_lowercase();
    GENEROS.iter().copied().find(|g| g.to_lowercase() == alvo)
}

fn erro_genero() -> ApiError {
    ApiError::campo("genero", format!("Gênero inválido. Use um destes: {}", GENEROS.join(", ")))
}

fn data_da_query(q: &HashMap<String, String>, hoje: NaiveDate) -> ApiResult<NaiveDate> {
    match q.get("data") {
        None => Ok(hoje),
        Some(texto) => parse_data(texto).ok_or_else(|| ApiError::campo("data", MSG_DATA_INVALIDA)),
    }
}

// ===== Banco de dados =====

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS filmes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    titulo      TEXT    NOT NULL,
    ano         INTEGER NOT NULL,
    genero      TEXT    NOT NULL,
    duracao_min INTEGER NOT NULL CHECK (duracao_min BETWEEN 1 AND 300),
    sinopse     TEXT,
    criado_em   TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE IF NOT EXISTS sessoes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    filme_id   INTEGER NOT NULL REFERENCES filmes(id) ON DELETE RESTRICT,
    data       TEXT    NOT NULL,
    inicio     TEXT    NOT NULL,
    status     TEXT    NOT NULL DEFAULT 'planejada'
               CHECK (status IN ('planejada', 'assistida', 'cancelada')),
    nota       INTEGER CHECK (nota BETWEEN 1 AND 5),
    comentario TEXT,
    criado_em  TEXT    NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS ix_sessoes_data ON sessoes(data);
";

fn abrir_banco(caminho: &str) -> rusqlite::Result<Connection> {
    if let Some(pasta) = PathBuf::from(caminho).parent() {
        if !pasta.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(pasta);
        }
    }
    let conn = Connection::open(caminho)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

fn semear_se_vazio(conn: &Connection) -> rusqlite::Result<usize> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM filmes", [], |r| r.get(0))?;
    if total > 0 {
        return Ok(0);
    }
    for (titulo, ano, genero, duracao) in FILMES_INICIAIS {
        conn.execute(
            "INSERT INTO filmes (titulo, ano, genero, duracao_min) VALUES (?1, ?2, ?3, ?4)",
            params![titulo, ano, genero, duracao],
        )?;
    }
    Ok(FILMES_INICIAIS.len())
}

// ===== Filmes =====

#[derive(Serialize)]
struct Filme {
    id: i64,
    titulo: String,
    ano: i64,
    genero: String,
    duracao_min: i64,
    sinopse: Option<String>,
    total_sessoes: i64,
    criado_em: String,
}

const SELECT_FILME: &str = "SELECT f.*, \
    (SELECT COUNT(*) FROM sessoes s WHERE s.filme_id = f.id) AS total_sessoes FROM filmes f";

fn linha_filme(r: &Row) -> rusqlite::Result<Filme> {
    Ok(Filme {
        id: r.get("id")?,
        titulo: r.get("titulo")?,
        ano: r.get("ano")?,
        genero: r.get("genero")?,
        duracao_min: r.get("duracao_min")?,
        sinopse: r.get("sinopse")?,
        total_sessoes: r.get("total_sessoes")?,
        criado_em: r.get("criado_em")?,
    })
}

fn buscar_filme(conn: &Connection, id: i64) -> rusqlite::Result<Option<Filme>> {
    conn.query_row(&format!("{SELECT_FILME} WHERE f.id = ?1"), [id], linha_filme)
        .optional()
}

#[derive(Deserialize)]
struct FilmeEntrada {
    titulo: Option<String>,
    ano: Option<i64>,
    genero: Option<String>,
    duracao_min: Option<i64>,
    sinopse: Option<String>,
}

struct FilmeValido {
    titulo: String,
    ano: i64,
    genero: &'static str,
    duracao_min: i64,
    sinopse: Option<String>,
}

/// RN01 a RN05. Os campos são validados nesta ordem: titulo, ano, genero, duracao_min, sinopse.
fn validar_filme(e: FilmeEntrada, hoje: NaiveDate) -> ApiResult<FilmeValido> {
    let titulo = texto_opcional(e.titulo)
        .ok_or_else(|| ApiError::campo("titulo", "O título é obrigatório"))?;
    if titulo.chars().count() > TITULO_MAX {
        return Err(ApiError::campo(
            "titulo",
            format!("O título deve ter no máximo {TITULO_MAX} caracteres"),
        ));
    }

    let ano_max = hoje.year() as i64 + ANOS_NO_FUTURO;
    let ano = e.ano.ok_or_else(|| ApiError::campo("ano", "O ano é obrigatório"))?;
    if !(ANO_MIN..=ano_max).contains(&ano) {
        return Err(ApiError::campo("ano", format!("O ano deve estar entre {ANO_MIN} e {ano_max}")));
    }

    let genero = texto_opcional(e.genero)
        .ok_or_else(|| ApiError::campo("genero", "O gênero é obrigatório"))?;
    let genero = normalizar_genero(&genero).ok_or_else(erro_genero)?;

    let duracao_min = e
        .duracao_min
        .ok_or_else(|| ApiError::campo("duracao_min", "A duração é obrigatória"))?;
    if !(DURACAO_MIN..=DURACAO_MAX).contains(&duracao_min) {
        return Err(ApiError::campo(
            "duracao_min",
            format!("A duração deve estar entre {DURACAO_MIN} e {DURACAO_MAX} minutos"),
        ));
    }

    let sinopse = texto_opcional(e.sinopse);
    if sinopse.as_ref().is_some_and(|s| s.chars().count() > SINOPSE_MAX) {
        return Err(ApiError::campo(
            "sinopse",
            format!("A sinopse deve ter no máximo {SINOPSE_MAX} caracteres"),
        ));
    }

    Ok(FilmeValido { titulo, ano, genero, duracao_min, sinopse })
}

/// RN06: título + ano únicos, sem diferenciar maiúsculas/minúsculas.
fn checar_duplicado(conn: &Connection, f: &FilmeValido, ignorar: Option<i64>) -> ApiResult<()> {
    let alvo = f.titulo.to_lowercase();
    let existe = conn
        .prepare("SELECT id, titulo FROM filmes WHERE ano = ?1")?
        .query_map([f.ano], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .any(|(id, titulo)| Some(id) != ignorar && titulo.to_lowercase() == alvo);
    if existe {
        return Err(ApiError::conflito("Já existe um filme com este título e ano"));
    }
    Ok(())
}

async fn listar_filmes(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<Filme>>> {
    let genero = match q.get("genero").map(|g| g.trim()).filter(|g| !g.is_empty()) {
        None => None,
        Some(g) => Some(normalizar_genero(g).ok_or_else(erro_genero)?),
    };
    let busca = q.get("busca").map(|b| b.trim().to_lowercase()).unwrap_or_default();

    let conn = st.db.lock().unwrap();
    let mut filmes = conn
        .prepare(SELECT_FILME)?
        .query_map([], linha_filme)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    filmes.retain(|f| {
        genero.map_or(true, |g| f.genero == g) && f.titulo.to_lowercase().contains(&busca)
    });
    filmes.sort_by_key(|f| (f.titulo.to_lowercase(), f.ano));
    Ok(Json(filmes))
}

async fn obter_filme(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Filme>> {
    let id = parse_id(&id)?;
    let conn = st.db.lock().unwrap();
    let filme = buscar_filme(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Filme não encontrado"))?;
    Ok(Json(filme))
}

async fn criar_filme(
    State(st): State<AppState>,
    Corpo(entrada): Corpo<FilmeEntrada>,
) -> ApiResult<(StatusCode, Json<Filme>)> {
    let f = validar_filme(entrada, st.hoje())?;
    let conn = st.db.lock().unwrap();
    checar_duplicado(&conn, &f, None)?;
    conn.execute(
        "INSERT INTO filmes (titulo, ano, genero, duracao_min, sinopse) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![f.titulo, f.ano, f.genero, f.duracao_min, f.sinopse],
    )?;
    let filme = buscar_filme(&conn, conn.last_insert_rowid())?
        .ok_or_else(|| ApiError::nao_encontrado("Filme não encontrado"))?;
    Ok((StatusCode::CREATED, Json(filme)))
}

async fn atualizar_filme(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Corpo(entrada): Corpo<FilmeEntrada>,
) -> ApiResult<Json<Filme>> {
    let id = parse_id(&id)?;
    let f = validar_filme(entrada, st.hoje())?;
    let conn = st.db.lock().unwrap();
    let atual = buscar_filme(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Filme não encontrado"))?;
    checar_duplicado(&conn, &f, Some(id))?;

    // RN07: mudar a duração quebraria os horários já planejados
    if f.duracao_min != atual.duracao_min {
        let planejadas: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessoes WHERE filme_id = ?1 AND status = 'planejada'",
            [id],
            |r| r.get(0),
        )?;
        if planejadas > 0 {
            return Err(ApiError::conflito(
                "Não é possível alterar a duração de um filme com sessões planejadas",
            ));
        }
    }

    conn.execute(
        "UPDATE filmes SET titulo = ?1, ano = ?2, genero = ?3, duracao_min = ?4, sinopse = ?5 WHERE id = ?6",
        params![f.titulo, f.ano, f.genero, f.duracao_min, f.sinopse, id],
    )?;
    let filme = buscar_filme(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Filme não encontrado"))?;
    Ok(Json(filme))
}

async fn excluir_filme(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let conn = st.db.lock().unwrap();
    let filme = buscar_filme(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Filme não encontrado"))?;
    // RN15
    if filme.total_sessoes > 0 {
        return Err(ApiError::conflito(
            "Este filme tem sessões na agenda e não pode ser excluído",
        ));
    }
    conn.execute("DELETE FROM filmes WHERE id = ?1", [id])?;
    Ok(StatusCode::NO_CONTENT)
}

// ===== Sessões =====

#[derive(Serialize)]
struct Sessao {
    id: i64,
    filme_id: i64,
    titulo: String,
    genero: String,
    duracao_min: i64,
    data: String,
    inicio: String,
    fim: String,
    status: String,
    nota: Option<i64>,
    comentario: Option<String>,
    criado_em: String,
}

const SELECT_SESSAO: &str = "SELECT s.id, s.filme_id, f.titulo, f.genero, f.duracao_min, \
    s.data, s.inicio, s.status, s.nota, s.comentario, s.criado_em \
    FROM sessoes s JOIN filmes f ON f.id = s.filme_id";

fn linha_sessao(r: &Row) -> rusqlite::Result<Sessao> {
    let inicio: String = r.get("inicio")?;
    let duracao_min: i64 = r.get("duracao_min")?;
    Ok(Sessao {
        id: r.get("id")?,
        filme_id: r.get("filme_id")?,
        titulo: r.get("titulo")?,
        genero: r.get("genero")?,
        duracao_min,
        data: r.get("data")?,
        fim: fmt_hora(parse_hora(&inicio).unwrap_or(0) + duracao_min),
        inicio,
        status: r.get("status")?,
        nota: r.get("nota")?,
        comentario: r.get("comentario")?,
        criado_em: r.get("criado_em")?,
    })
}

fn buscar_sessao(conn: &Connection, id: i64) -> rusqlite::Result<Option<Sessao>> {
    conn.query_row(&format!("{SELECT_SESSAO} WHERE s.id = ?1"), [id], linha_sessao)
        .optional()
}

fn sessoes_do_dia(conn: &Connection, data: &str) -> rusqlite::Result<Vec<Sessao>> {
    conn.prepare(&format!("{SELECT_SESSAO} WHERE s.data = ?1 ORDER BY s.inicio, s.id"))?
        .query_map([data], linha_sessao)?
        .collect()
}

/// RN10, RN11 e RN09, nesta ordem. Sessões canceladas não ocupam a agenda.
fn checar_agenda(
    conn: &Connection,
    data: &str,
    filme_id: i64,
    inicio: i64,
    duracao: i64,
    ignorar: Option<i64>,
) -> ApiResult<()> {
    let ocupadas: Vec<Sessao> = sessoes_do_dia(conn, data)?
        .into_iter()
        .filter(|s| s.status != "cancelada" && Some(s.id) != ignorar)
        .collect();

    if ocupadas.iter().any(|s| s.filme_id == filme_id) {
        return Err(ApiError::conflito("Este filme já está agendado neste dia"));
    }

    let nova = (inicio, inicio + duracao);
    let choque = ocupadas.iter().find(|s| {
        let ini = parse_hora(&s.inicio).unwrap_or(0);
        sobrepoe(nova, (ini, ini + s.duracao_min))
    });
    if let Some(s) = choque {
        return Err(ApiError::conflito(format!(
            "Conflito de horário com \"{}\" ({} às {})",
            s.titulo, s.inicio, s.fim
        )));
    }

    let minutos: i64 = ocupadas.iter().map(|s| s.duracao_min).sum();
    if minutos + duracao > LIMITE_DIARIO_MIN {
        return Err(ApiError::conflito(format!(
            "Limite diário de {LIMITE_DIARIO_MIN} minutos excedido: já há {minutos} min agendados e este filme tem {duracao} min"
        )));
    }
    Ok(())
}

async fn listar_sessoes(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Vec<Sessao>>> {
    let data = data_da_query(&q, st.hoje())?;
    let status = match q.get("status") {
        None => None,
        Some(s) => Some(STATUS.iter().copied().find(|x| x == s).ok_or_else(|| {
            ApiError::campo("status", "Status inválido. Use: planejada, assistida ou cancelada")
        })?),
    };
    let conn = st.db.lock().unwrap();
    let mut sessoes = sessoes_do_dia(&conn, &data.to_string())?;
    if let Some(status) = status {
        sessoes.retain(|s| s.status == status);
    }
    Ok(Json(sessoes))
}

async fn obter_sessao(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Sessao>> {
    let id = parse_id(&id)?;
    let conn = st.db.lock().unwrap();
    let sessao = buscar_sessao(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Sessão não encontrada"))?;
    Ok(Json(sessao))
}

#[derive(Deserialize)]
struct SessaoEntrada {
    filme_id: Option<i64>,
    data: Option<String>,
    inicio: Option<String>,
}

async fn criar_sessao(
    State(st): State<AppState>,
    Corpo(e): Corpo<SessaoEntrada>,
) -> ApiResult<(StatusCode, Json<Sessao>)> {
    let filme_id = e.filme_id.ok_or_else(|| ApiError::campo("filme_id", "O filme é obrigatório"))?;
    let data = texto_opcional(e.data).ok_or_else(|| ApiError::campo("data", "A data é obrigatória"))?;
    let data = parse_data(&data).ok_or_else(|| ApiError::campo("data", MSG_DATA_INVALIDA))?;
    let inicio = texto_opcional(e.inicio)
        .ok_or_else(|| ApiError::campo("inicio", "O horário de início é obrigatório"))?;
    let inicio = parse_hora(&inicio).ok_or_else(|| ApiError::campo("inicio", MSG_HORA_INVALIDA))?;

    // RN12
    if data < st.hoje() {
        return Err(ApiError::campo("data", "Não é possível agendar em uma data passada"));
    }

    let conn = st.db.lock().unwrap();
    let filme = buscar_filme(&conn, filme_id)?
        .ok_or_else(|| ApiError::campo("filme_id", "Filme não encontrado"))?;

    // RN08
    if inicio + filme.duracao_min > MINUTOS_NO_DIA {
        return Err(ApiError::campo("inicio", "A sessão deve terminar no mesmo dia (até 24:00)"));
    }

    let data = data.to_string();
    checar_agenda(&conn, &data, filme_id, inicio, filme.duracao_min, None)?;

    conn.execute(
        "INSERT INTO sessoes (filme_id, data, inicio) VALUES (?1, ?2, ?3)",
        params![filme_id, data, fmt_hora(inicio)],
    )?;
    let sessao = buscar_sessao(&conn, conn.last_insert_rowid())?
        .ok_or_else(|| ApiError::nao_encontrado("Sessão não encontrada"))?;
    Ok((StatusCode::CREATED, Json(sessao)))
}

#[derive(Deserialize)]
struct SessaoAtualizacao {
    #[serde(default, deserialize_with = "presente")]
    status: Option<Option<String>>,
    #[serde(default, deserialize_with = "presente")]
    nota: Option<Option<i64>>,
    #[serde(default, deserialize_with = "presente")]
    comentario: Option<Option<String>>,
}

async fn atualizar_sessao(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Corpo(e): Corpo<SessaoAtualizacao>,
) -> ApiResult<Json<Sessao>> {
    let id = parse_id(&id)?;
    if e.status.is_none() && e.nota.is_none() && e.comentario.is_none() {
        return Err(ApiError::requisicao("Informe ao menos um campo: status, nota ou comentario"));
    }

    // 1) Formato dos campos (400)
    let novo_status = match &e.status {
        None => None,
        Some(s) => {
            let s = s.as_deref().unwrap_or("").trim();
            Some(STATUS.iter().copied().find(|x| *x == s).ok_or_else(|| {
                ApiError::campo("status", "Status inválido. Use: planejada, assistida ou cancelada")
            })?)
        }
    };
    let nota = match e.nota {
        Some(Some(n)) if !(1..=5).contains(&n) => {
            return Err(ApiError::campo("nota", "A nota deve ser um número inteiro de 1 a 5"));
        }
        outra => outra,
    };
    let comentario = e.comentario.map(texto_opcional);
    if let Some(Some(c)) = &comentario {
        if c.chars().count() > COMENTARIO_MAX {
            return Err(ApiError::campo(
                "comentario",
                format!("O comentário deve ter no máximo {COMENTARIO_MAX} caracteres"),
            ));
        }
    }

    // 2) Existe? (404)
    let hoje = st.hoje();
    let conn = st.db.lock().unwrap();
    let atual = buscar_sessao(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Sessão não encontrada"))?;
    let data = parse_data(&atual.data).unwrap_or(hoje);

    // 3) Transições de status (409) — RN13
    let status_final = novo_status.map(String::from).unwrap_or_else(|| atual.status.clone());
    if status_final != atual.status {
        match (atual.status.as_str(), status_final.as_str()) {
            ("assistida", _) => {
                return Err(ApiError::conflito("Uma sessão assistida não pode mudar de status"));
            }
            ("cancelada", "assistida") => {
                return Err(ApiError::conflito(
                    "Reative a sessão (status planejada) antes de marcá-la como assistida",
                ));
            }
            (_, "assistida") if data > hoje => {
                return Err(ApiError::conflito(
                    "Não é possível marcar como assistida uma sessão futura",
                ));
            }
            ("cancelada", "planejada") => {
                if data < hoje {
                    return Err(ApiError::conflito(
                        "Não é possível reativar uma sessão de data passada",
                    ));
                }
                let inicio = parse_hora(&atual.inicio).unwrap_or(0);
                checar_agenda(&conn, &atual.data, atual.filme_id, inicio, atual.duracao_min, Some(id))?;
            }
            _ => {}
        }
    }

    // RN14: nota só para sessão assistida
    if matches!(nota, Some(Some(_))) && status_final != "assistida" {
        return Err(ApiError::conflito("A nota só pode ser dada a sessões assistidas"));
    }

    let nota_final = nota.unwrap_or(atual.nota);
    let comentario_final = comentario.unwrap_or(atual.comentario);
    conn.execute(
        "UPDATE sessoes SET status = ?1, nota = ?2, comentario = ?3 WHERE id = ?4",
        params![status_final, nota_final, comentario_final, id],
    )?;
    let sessao = buscar_sessao(&conn, id)?.ok_or_else(|| ApiError::nao_encontrado("Sessão não encontrada"))?;
    Ok(Json(sessao))
}

async fn excluir_sessao(State(st): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let conn = st.db.lock().unwrap();
    if conn.execute("DELETE FROM sessoes WHERE id = ?1", [id])? == 0 {
        return Err(ApiError::nao_encontrado("Sessão não encontrada"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ===== Resumo do dia =====

#[derive(Serialize)]
struct Resumo {
    data: String,
    limite_min: i64,
    minutos_agendados: i64,
    minutos_livres: i64,
    total: usize,
    planejadas: usize,
    assistidas: usize,
    canceladas: usize,
    nota_media: Option<f64>,
}

async fn resumo(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Resumo>> {
    let data = data_da_query(&q, st.hoje())?.to_string();
    let conn = st.db.lock().unwrap();
    let sessoes = sessoes_do_dia(&conn, &data)?;
    let conta = |status: &str| sessoes.iter().filter(|s| s.status == status).count();
    let minutos_agendados: i64 = sessoes
        .iter()
        .filter(|s| s.status != "cancelada")
        .map(|s| s.duracao_min)
        .sum();
    let notas: Vec<i64> = sessoes.iter().filter_map(|s| s.nota).collect();
    let nota_media = (!notas.is_empty()).then(|| {
        let media = notas.iter().sum::<i64>() as f64 / notas.len() as f64;
        (media * 10.0).round() / 10.0
    });
    Ok(Json(Resumo {
        data,
        limite_min: LIMITE_DIARIO_MIN,
        minutos_agendados,
        minutos_livres: LIMITE_DIARIO_MIN - minutos_agendados,
        total: sessoes.len(),
        planejadas: conta("planejada"),
        assistidas: conta("assistida"),
        canceladas: conta("cancelada"),
        nota_media,
    }))
}

// ===== Rotas simples =====

async fn health() -> &'static str {
    "ok"
}

async fn hoje(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "hoje": st.hoje().to_string() }))
}

async fn generos() -> Json<[&'static str; 11]> {
    Json(GENEROS)
}

async fn rota_inexistente() -> ApiError {
    ApiError::nao_encontrado("Rota não encontrada")
}

async fn metodo_nao_permitido() -> ApiError {
    ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "Método não permitido nesta rota")
}

// ===== Rotas de apoio a testes (só com TEST_ROUTES=1) =====

/// Apaga tudo, zera os ids e recarrega o catálogo inicial (`?vazio=true` não recarrega).
async fn teste_reset(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let conn = st.db.lock().unwrap();
    conn.execute_batch(
        "DELETE FROM sessoes; DELETE FROM filmes;
         DELETE FROM sqlite_sequence WHERE name IN ('filmes', 'sessoes');",
    )?;
    let filmes = if q.get("vazio").map(String::as_str) == Some("true") {
        0
    } else {
        semear_se_vazio(&conn)?
    };
    Ok(Json(json!({ "filmes": filmes })))
}

#[derive(Deserialize)]
struct HojeEntrada {
    #[serde(default, deserialize_with = "presente")]
    hoje: Option<Option<String>>,
}

/// Fixa a data "de hoje" (`{"hoje": "2026-01-10"}`) ou volta ao relógio real (`{"hoje": null}`).
async fn teste_definir_hoje(
    State(st): State<AppState>,
    Corpo(e): Corpo<HojeEntrada>,
) -> ApiResult<Json<serde_json::Value>> {
    let nova = match e.hoje {
        None => return Err(ApiError::campo("hoje", "Informe hoje: \"AAAA-MM-DD\" ou null")),
        Some(None) => None,
        Some(Some(texto)) => {
            Some(parse_data(&texto).ok_or_else(|| ApiError::campo("hoje", MSG_DATA_INVALIDA))?)
        }
    };
    *st.hoje_fixo.lock().unwrap() = nova;
    Ok(Json(json!({ "hoje": st.hoje().to_string(), "fixo": nova.is_some() })))
}

// ===== App =====

fn app(conn: Connection, static_dir: PathBuf, hoje_fixo: Option<NaiveDate>, rotas_teste: bool) -> Router {
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        hoje_fixo: Arc::new(Mutex::new(hoje_fixo)),
    };

    let mut api = Router::new()
        .route("/health", get(health))
        .route("/hoje", get(hoje))
        .route("/generos", get(generos))
        .route("/filmes", get(listar_filmes).post(criar_filme))
        .route("/filmes/{id}", get(obter_filme).put(atualizar_filme).delete(excluir_filme))
        .route("/sessoes", get(listar_sessoes).post(criar_sessao))
        .route("/sessoes/{id}", get(obter_sessao).patch(atualizar_sessao).delete(excluir_sessao))
        .route("/resumo", get(resumo));
    if rotas_teste {
        api = api
            .route("/teste/reset", post(teste_reset))
            .route("/teste/hoje", put(teste_definir_hoje));
    }
    let api = api
        .fallback(rota_inexistente)
        .method_not_allowed_fallback(metodo_nao_permitido)
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback_service(ServeDir::new(static_dir))
        .layer(CorsLayer::permissive())
}

#[tokio::main]
async fn main() {
    let port: u16 = env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3001);
    let db_path = env::var("DB_PATH").unwrap_or_else(|_| "data/agenda.db".into());
    let static_dir = PathBuf::from(env::var("STATIC_DIR").unwrap_or_else(|_| "public".into()));
    let rotas_teste = env::var("TEST_ROUTES").is_ok_and(|v| v == "1");
    let hoje_fixo = env::var("APP_HOJE").ok().map(|t| {
        parse_data(&t).expect("APP_HOJE deve estar no formato AAAA-MM-DD")
    });

    let conn = abrir_banco(&db_path).expect("não foi possível abrir o banco SQLite");
    if env::var("SEED").map_or(true, |v| v != "0") {
        semear_se_vazio(&conn).expect("não foi possível carregar o catálogo inicial");
    }

    let router = app(conn, static_dir, hoje_fixo, rotas_teste);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("porta em uso");
    println!("Agenda de Filmes em http://localhost:{port} (banco: {db_path})");
    if rotas_teste {
        println!("Rotas de teste ATIVAS: POST /api/teste/reset, PUT /api/teste/hoje");
    }

    axum::serve(listener, router)
        .with_graceful_shutdown(sinal_de_parada())
        .await
        .unwrap();
}

// Encerra com Ctrl+C ou com o SIGTERM enviado pelo `docker stop`.
async fn sinal_de_parada() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("falha ao registrar SIGTERM")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

// ===== Testes do desenvolvedor (os testes de QA ficam fora do código) =====

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    const HOJE: &str = "2026-11-10";

    fn test_app() -> Router {
        app(abrir_banco(":memory:").unwrap(), PathBuf::from("public"), parse_data(HOJE), true)
    }

    async fn chamar(app: &Router, metodo: &str, uri: &str, corpo: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(metodo).uri(uri);
        let body = match corpo {
            Some(c) => {
                req = req.header("content-type", "application/json");
                Body::from(c.to_string())
            }
            None => Body::empty(),
        };
        let resp = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn novo_filme(app: &Router, titulo: &str, duracao: i64) -> i64 {
        let corpo = json!({ "titulo": titulo, "ano": 2000, "genero": "Drama", "duracao_min": duracao });
        let (status, filme) = chamar(app, "POST", "/api/filmes", Some(corpo)).await;
        assert_eq!(status, StatusCode::CREATED, "{filme}");
        filme["id"].as_i64().unwrap()
    }

    #[test]
    fn parse_hora_aceita_somente_hh_mm_validas() {
        assert_eq!(parse_hora("00:00"), Some(0));
        assert_eq!(parse_hora("23:59"), Some(1439));
        for invalida in ["24:00", "9:00", "12:60", "ab:cd", "12h00", ""] {
            assert_eq!(parse_hora(invalida), None, "{invalida}");
        }
    }

    #[test]
    fn parse_data_exige_formato_e_dia_existente() {
        assert!(parse_data("2028-02-29").is_some());
        for invalida in ["2026-2-01", "2026-02-30", "01/02/2026", "2026-13-01"] {
            assert!(parse_data(invalida).is_none(), "{invalida}");
        }
    }

    #[test]
    fn sessoes_que_apenas_encostam_nao_se_sobrepoem() {
        assert!(!sobrepoe((60, 120), (120, 180)));
        assert!(sobrepoe((60, 121), (120, 180)));
        assert!(sobrepoe((0, 1440), (600, 700)));
    }

    #[test]
    fn genero_ignora_maiusculas_e_espacos() {
        assert_eq!(normalizar_genero("  ficção CIENTÍFICA "), Some("Ficção científica"));
        assert_eq!(normalizar_genero("Faroeste"), None);
    }

    #[tokio::test]
    async fn filme_duplicado_ignora_maiusculas() {
        let app = test_app();
        novo_filme(&app, "Interestelar", 169).await;
        let corpo = json!({ "titulo": "INTERESTELAR", "ano": 2000, "genero": "Drama", "duracao_min": 169 });
        let (status, erro) = chamar(&app, "POST", "/api/filmes", Some(corpo)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(erro["erro"], "Já existe um filme com este título e ano");
    }

    #[tokio::test]
    async fn agenda_bloqueia_conflito_e_limite_diario() {
        let app = test_app();
        let a = novo_filme(&app, "A", 240).await;
        let b = novo_filme(&app, "B", 240).await;
        let c = novo_filme(&app, "C", 1).await;
        let sessao = |filme: i64, inicio: &str| json!({ "filme_id": filme, "data": HOJE, "inicio": inicio });

        let (s, _) = chamar(&app, "POST", "/api/sessoes", Some(sessao(a, "08:00"))).await;
        assert_eq!(s, StatusCode::CREATED);
        let (s, erro) = chamar(&app, "POST", "/api/sessoes", Some(sessao(b, "11:59"))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert!(erro["erro"].as_str().unwrap().starts_with("Conflito de horário"));
        // encostando: termina 12:00, começa 12:00 → ok; total = 480 (no limite)
        let (s, _) = chamar(&app, "POST", "/api/sessoes", Some(sessao(b, "12:00"))).await;
        assert_eq!(s, StatusCode::CREATED);
        // 481 minutos → acima do limite
        let (s, erro) = chamar(&app, "POST", "/api/sessoes", Some(sessao(c, "20:00"))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert!(erro["erro"].as_str().unwrap().starts_with("Limite diário"));
    }

    #[tokio::test]
    async fn sessao_futura_nao_pode_ser_assistida() {
        let app = test_app();
        let filme = novo_filme(&app, "Futuro", 100).await;
        let corpo = json!({ "filme_id": filme, "data": "2026-11-11", "inicio": "20:00" });
        let (_, sessao) = chamar(&app, "POST", "/api/sessoes", Some(corpo)).await;
        let uri = format!("/api/sessoes/{}", sessao["id"]);
        let (s, erro) = chamar(&app, "PATCH", &uri, Some(json!({ "status": "assistida" }))).await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(erro["erro"], "Não é possível marcar como assistida uma sessão futura");
    }

    #[tokio::test]
    async fn erros_de_corpo_tambem_sao_json() {
        let app = test_app();
        let req = Request::builder().method("POST").uri("/api/filmes")
            .header("content-type", "application/json").body(Body::from("{")).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["erro"], "JSON malformado");

        let (s, erro) = chamar(&app, "GET", "/api/nao-existe", None).await;
        assert_eq!((s, erro["erro"].as_str()), (StatusCode::NOT_FOUND, Some("Rota não encontrada")));
    }
}
