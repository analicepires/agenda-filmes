# ===== Etapa 1: compila o backend Rust =====
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# Compila só as dependências primeiro (fica em cache entre builds)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

# ===== Etapa 2: imagem final enxuta =====
FROM debian:bookworm-slim
WORKDIR /app

# tzdata: "hoje" segue o fuso de TZ (padrão: horário de Brasília)
RUN apt-get update && apt-get install -y --no-install-recommends tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/agenda-filmes /usr/local/bin/agenda-filmes
COPY public ./public

ENV PORT=80 \
    DB_PATH=/app/data/agenda.db \
    STATIC_DIR=/app/public \
    TZ=America/Sao_Paulo

# O banco SQLite fica em um volume para sobreviver à recriação do container
VOLUME ["/app/data"]
EXPOSE 80

CMD ["agenda-filmes"]
