# Roteiro de QA — o que você precisa entregar

Este documento lista os **requisitos da sua entrega de QA** para a Agenda de Filmes. O sistema já está pronto; o seu trabalho é **provar que ele cumpre a [Especificação de Requisitos](REQUISITOS.md)** e encontrar onde ele não cumpre.

Marque cada item conforme concluir. Uma entrega "completa" cumpre **todos os itens obrigatórios (🔴)**. Os itens 🟡 são diferenciais para a apresentação.

---

## Estrutura sugerida
```
agenda-filmes/
└── qa/
    ├── README.md                 # como rodar tudo
    ├── requirements.txt          # pytest, selenium, requests
    ├── conftest.py               # sobe o servidor com banco temporário e TEST_ROUTES=1
    ├── postman/
    │   ├── Agenda-Filmes.postman_collection.json
    │   └── Agenda-Local.postman_environment.json
    ├── frontend/test_*.py        # Selenium
    ├── database/test_*.py        # banco
    └── reports/                  # relatório Newman e prints (fora do git)
docs/
    ├── PLANO_DE_TESTES.md        # você escreve
    └── evidencias/               # prints que vão para a apresentação
```

---

## Q1 — Plano de testes 🔴
- [ ] **Q1.1** `docs/PLANO_DE_TESTES.md` com: objetivo, escopo (dentro/fora), estratégia (pirâmide), ambiente e ferramentas com justificativa, critérios de entrada e saída.
- [ ] **Q1.2** Lista de casos de teste com **ID** (`CT-API-01`, `CT-FE-01`, `CT-DB-01`), cenário, pré-condição, passos, resultado esperado e o **requisito coberto** (RFxx / RNxx / RNFxx).
- [ ] **Q1.3** **Matriz de rastreabilidade:** toda RN01–RN17 tem pelo menos 1 teste em alguma camada.
- [ ] **Q1.4** Técnicas usadas citadas com exemplo: partição de equivalência, **análise de valor-limite**, tabela de decisão (transições da RN13), teste de contrato, testes negativos.

## Q2 — API com Postman 🔴
- [ ] **Q2.1** Coleção organizada em pastas: `Smoke`, `Filmes`, `Sessões`, `Transições de status`, `Resumo`, `Negativos`, `Limpeza`.
- [ ] **Q2.2** Ambiente com a variável `baseUrl` (Local `http://localhost:3001` e Docker `http://localhost:8081`).
- [ ] **Q2.3** **Setup** com `POST /api/teste/reset` e `PUT /api/teste/hoje` (data fixa). A coleção tem que passar **em qualquer dia** e **rodando duas vezes seguidas**.
- [ ] **Q2.4** Todos os **14 endpoints** da seção 5 dos requisitos testados, com status e corpo verificados.
- [ ] **Q2.5** Cada regra **RN01 a RN17** com pelo menos um caso **válido e um inválido**; regras com limite numérico testadas **nos dois lados da borda** (tabela abaixo).
- [ ] **Q2.6** Mensagens de erro conferidas com o texto **exato** do catálogo (E01–E40), incluindo o campo `campo`.
- [ ] **Q2.7** Encadeamento: ids criados guardados em variáveis (`pm.collectionVariables.set`) e reaproveitados.
- [ ] **Q2.8** Validação de **contrato** com JSON Schema em pelo menos `POST /api/filmes`, `POST /api/sessoes` e `GET /api/resumo`.
- [ ] **Q2.9** Asserção de tempo (RNF01, < 500 ms) no nível da coleção.
- [ ] **Q2.10** Execução pelo **Newman** com relatório HTML (`newman-reporter-htmlextra`).
- [ ] 🟡 **Q2.11** Teste *data-driven*: um request de cadastro de filme rodando com um CSV/JSON de entradas inválidas (`-d dados.csv`).

### Valores-limite obrigatórios
| Regra | Abaixo (inválido) | No limite (válido) | Acima (inválido) |
|-------|-------------------|--------------------|------------------|
| RN01 título | vazio / só espaços | 1 e 100 caracteres | 101 |
| RN02 ano | 1887 | 1888 e ano atual + 5 | ano atual + 6 |
| RN04 duração | 0 | 1 e 300 | 301 |
| RN05 sinopse | — | 500 | 501 |
| RN08 fim do dia | — | termina exatamente 24:00 | termina 24:01 |
| RN09 limite diário | — | total 480 | total 481 |
| RN11 sobreposição | começa 1 min antes do fim da outra | começa no minuto exato do fim | — |
| RN12 data | ontem | hoje | — |
| RN14 nota | 0 | 1 e 5 | 6 |
| RN14 comentário | — | 280 | 281 |
| Horário | `24:00`, `9:00`, `12:60` | `00:00` e `23:59` | — |

## Q3 — Interface com Selenium (Python) 🔴
- [ ] **Q3.1** Padrão **Page Object** (uma classe para a agenda, outra para o catálogo) usando os `data-testid`.
- [ ] **Q3.2** Só **esperas explícitas** (`WebDriverWait`); comece esperando `body[data-pronto="true"]`. Nada de `time.sleep` para esperar a página.
- [ ] **Q3.3** Cada teste começa de um estado conhecido (reset + data fixa via API, numa fixture).
- [ ] **Q3.4** No mínimo estes cenários:
  - [ ] Agendar sessão com sucesso: mensagem, item na lista com horário "início – fim", resumo atualizado.
  - [ ] Agendar em conflito: mensagem E23 exibida, nada é adicionado.
  - [ ] Agendar sem escolher filme: erro E14 **embaixo do campo** e campo com `aria-invalid="true"`.
  - [ ] Ciclo de status: planejada → assistida → avaliar (nota + comentário) → estrelas e comentário aparecem.
  - [ ] Cancelar e reativar: item riscado e contadores do resumo mudando.
  - [ ] Navegação de datas: ◀ ▶ Hoje, selo Passado/Hoje/Futuro e URL com `#data`.
  - [ ] Catálogo: cadastrar, buscar, filtrar por gênero, editar e excluir.
  - [ ] Excluir filme com sessão: mensagem E12 e o filme continua na tabela.
- [ ] **Q3.5** Print salvo ao fim de cada teste (evidência).
- [ ] 🟡 **Q3.6** Rodar com o navegador visível (`HEADLESS=0`) e também headless.
- [ ] 🟡 **Q3.7** Um teste em tela de celular (360 × 800) verificando o RNF06.

## Q4 — Banco de dados 🔴
- [ ] **Q4.1** Os testes sobem o servidor com um `DB_PATH` **temporário** e leem o arquivo direto com `sqlite3`.
- [ ] **Q4.2** **Estrutura:** colunas, tipos e restrições das duas tabelas (`PRAGMA table_info`, `PRAGMA foreign_key_list`).
- [ ] **Q4.3** **Gravação:** ação feita pela API e conferida com `SELECT` (cadastro, agendamento, mudança de status, nota, exclusão).
- [ ] **Q4.4** **Restrições do banco:** inserir direto via SQL uma nota 6, um status `"visto"` e uma duração 0, esperando `sqlite3.IntegrityError`.
- [ ] **Q4.5** **Chave estrangeira:** inserir sessão com `filme_id` inexistente direto no banco. Repare na observação da seção 8 dos requisitos (`PRAGMA foreign_keys`) e explique no plano o que você observou.
- [ ] **Q4.6** **Integridade das regras:** após tentativas que deveriam falhar (passar de 480 min, mesmo filme duas vezes no dia, conflito de horário), o banco não tem registros que violem RN09–RN11 (consulta SQL que soma minutos por dia, por exemplo).
- [ ] **Q4.7** **Persistência:** parar e subir o servidor de novo com o mesmo arquivo; os dados continuam lá (RNF03).
- [ ] **Q4.8** **Segurança:** título `'); DROP TABLE filmes;--` é gravado como texto e a tabela continua existindo (RNF04).

## Q5 — Testes exploratórios e bugs 🔴
- [ ] **Q5.1** Pelo menos **1 sessão de teste exploratório** de 30 min, documentada (missão, o que foi feito, o que foi achado). Sugestões de missão abaixo.
- [ ] **Q5.2** Todo bug ou dúvida de requisito registrado com: ID, título, severidade, prioridade, passos, esperado × atual, evidência e requisito violado.
- [ ] **Q5.3** Teste de XSS pela interface (RNF04): cadastrar `<img src=x onerror=alert(1)>` como título e verificar que aparece como texto.

### Pistas para explorar
O sistema foi escrito seguindo os requisitos, mas **nenhum sistema está livre de defeitos**. E nenhum requisito está livre de ambiguidade. Pontos que merecem investigação:
- Agendar **hoje** às 08:00 quando já são 20:00: o requisito (RN12) só compara a data. Isso faz sentido para o usuário?
- Editar o **título** de um filme que já foi assistido: o histórico das sessões passadas muda junto?
- Sessão cancelada impede excluir o filme (RN15). É o comportamento que o usuário espera?
- A tela de agenda depende da data do **servidor** (`/api/hoje`), não da do navegador. O que acontece perto da meia-noite, ou com o servidor em outro fuso?
- Dois navegadores abertos agendando ao mesmo tempo: o limite de 480 min se mantém?
- Campos numéricos recebendo decimais (`"duracao_min": 90.5`) ou texto: a mensagem ajuda o usuário?
- Acessibilidade: dá para usar tudo só com o teclado?

## Q6 — Integração contínua 🟡
- [ ] **Q6.1** Adicionar ao `.github/workflows/ci.yml` um job que sobe o servidor e roda a coleção com Newman.
- [ ] **Q6.2** Adicionar os testes de banco ao CI (não precisam de navegador).
- [ ] **Q6.3** Ativar a proteção da `main` exigindo os checks, e provar o bloqueio com uma PR de teste quebrado.

## Q7 — Relatório e apresentação 🔴
- [ ] **Q7.1** Relatório de execução no plano: quantidade de testes por camada, passou/falhou, bugs encontrados por severidade, data e ambiente.
- [ ] **Q7.2** `qa/README.md` explicando como rodar cada camada em uma máquina nova.
- [ ] **Q7.3** Evidências em `docs/evidencias/`: relatório Newman, prints do Selenium e saída dos testes de banco.

---

## Definição de pronto
A entrega de QA está pronta quando:
1. todos os itens 🔴 estão marcados;
2. `cargo test`, Newman e `pytest qa` rodam **duas vezes seguidas** sem falhas inesperadas, em qualquer dia;
3. cada RN aparece na matriz de rastreabilidade;
4. cada bug encontrado tem passos reproduzíveis e evidência.

## Dicas rápidas
**Subir o servidor para testes**
```bash
# PowerShell
$env:TEST_ROUTES="1"; $env:DB_PATH="data/teste.db"; cargo run
# Git Bash
TEST_ROUTES=1 DB_PATH=data/teste.db cargo run
```

**Pre-request do Postman para começar limpo, com data fixa**
```javascript
const base = pm.environment.get('baseUrl');
pm.sendRequest({ url: base + '/api/teste/reset', method: 'POST' });
pm.sendRequest({
  url: base + '/api/teste/hoje', method: 'PUT',
  header: { 'Content-Type': 'application/json' },
  body: { mode: 'raw', raw: JSON.stringify({ hoje: '2026-11-10' }) }
});
```

**Selenium: abrir um dia específico e esperar a página**
```python
driver.get(f"{base_url}/#2026-11-10")
WebDriverWait(driver, 10).until(
    lambda d: d.find_element(By.TAG_NAME, "body").get_attribute("data-pronto") == "true")
```

**Windows + pytest + Selenium:** se aparecer `OSError: [WinError 6]` ao abrir o Chrome, crie `qa/pytest.ini` com:
```ini
[pytest]
addopts = --capture=sys
```
(A captura padrão do pytest redireciona o `stdin` e o Selenium Manager herda um identificador inválido.)

**Banco: ligar a chave estrangeira na sua conexão**
```python
conn = sqlite3.connect(db_path)
conn.execute("PRAGMA foreign_keys = ON")
```
