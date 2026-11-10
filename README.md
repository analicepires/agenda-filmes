# 🎬 Agenda de Filmes

Organizador diário de filmes: mantenha um **catálogo** e monte a **agenda de cada dia**, com horário de início, controle de conflitos, limite de 8 horas por dia, status (planejada, assistida, cancelada) e avaliação de 1 a 5 estrelas.

Projeto feito para **praticar QA**. Os requisitos são numerados e têm valores-limite e mensagens exatas, e o sistema traz recursos próprios para facilitar testes automatizados.

| Documento | Para quê |
|-----------|----------|
| [docs/REQUISITOS.md](docs/REQUISITOS.md) | Especificação: requisitos funcionais, regras de negócio, API, mensagens de erro, modelo de dados e recursos para testes |
| [docs/ROTEIRO_QA.md](docs/ROTEIRO_QA.md) | O que a entrega de QA precisa cumprir (Postman, Selenium, banco, bugs, CI) |

## Stack
| Camada | Tecnologia |
|--------|-----------|
| Front-end | HTML + CSS + JavaScript puro (`public/`) |
| Backend | Rust + Axum 0.8 (`src/main.rs`) |
| Banco | SQLite via `rusqlite` |
| Entrega | Docker multi-stage + GitHub Actions (`cargo test` em toda PR) |

## Como rodar
**Com Rust:**
```bash
cargo run
```
Acesse http://localhost:3001. Na primeira execução, o catálogo começa com 12 filmes.

**Com Docker:**
```bash
docker build -t agenda-filmes .
docker run -d -p 8081:80 -v agenda-data:/app/data --name agenda agenda-filmes
```
Acesse http://localhost:8081.

**Para testes** (liga as rotas de reset e de data fixa):
```bash
# Git Bash
TEST_ROUTES=1 DB_PATH=data/teste.db cargo run
# PowerShell
$env:TEST_ROUTES="1"; $env:DB_PATH="data/teste.db"; cargo run
# Docker
docker run -d -p 8081:80 -e TEST_ROUTES=1 agenda-filmes
```

| Variável | Padrão | Descrição |
|----------|--------|-----------|
| `PORT` | 3001 | Porta |
| `DB_PATH` | `data/agenda.db` | Arquivo do banco |
| `STATIC_DIR` | `public` | Pasta do front-end |
| `SEED` | `1` | `0` = começa sem o catálogo inicial |
| `TEST_ROUTES` | desligado | `1` = liga `POST /api/teste/reset` e `PUT /api/teste/hoje` |
| `APP_HOJE` | — | Fixa a data de "hoje" (`AAAA-MM-DD`) |

## Testes do desenvolvedor
```bash
cargo test
```
São 8 testes de unidade e integração da API, que rodam no CI. **Os testes de QA (Postman, Selenium e banco) são a sua entrega**: veja o [roteiro](docs/ROTEIRO_QA.md).

## API (resumo)
| Método | Rota | |
|--------|------|-|
| GET | `/api/health`, `/api/hoje`, `/api/generos` | Utilitários |
| GET, POST | `/api/filmes` | Listar (`?busca=&genero=`) e cadastrar |
| GET, PUT, DELETE | `/api/filmes/{id}` | Ver, editar e excluir |
| GET, POST | `/api/sessoes` | Listar do dia (`?data=&status=`) e agendar |
| GET, PATCH, DELETE | `/api/sessoes/{id}` | Ver, mudar status/nota/comentário e excluir |
| GET | `/api/resumo?data=` | Minutos agendados, contagens e nota média |

Detalhes, exemplos e todas as mensagens de erro: [REQUISITOS.md](docs/REQUISITOS.md#5-api-rest).

## Estrutura
```
agenda-filmes/
├── src/main.rs                # API, regras de negócio, banco e testes do dev
├── public/                    # index.html, style.css, app.js
├── docs/                      # requisitos e roteiro de QA
├── Dockerfile / .dockerignore
└── .github/workflows/ci.yml   # cargo test em PRs e na main
```
