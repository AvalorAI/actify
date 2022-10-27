FROM rust:1.64

WORKDIR /app

COPY ./src ./src
COPY ./Cargo.toml ./Cargo.toml

CMD ["cargo", "test"]