FROM rust:latest AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang

COPY ./ ./

ARG BINARY=rdf-diff-store
RUN cargo build --release --bin ${BINARY}


FROM rust:latest

ENV TZ=Europe/Oslo
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

ARG BINARY
COPY --from=builder /build/target/release/${BINARY} /release

CMD ["/release"]
