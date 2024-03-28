FROM rust:1.77.0 AS builder
RUN apt-get update -y
RUN apt install -y musl-tools pkg-config make g++ libssl-dev

# ------------------------------- -------------------------------
#          Build OpenSSL for the `musl` build target
# ------------------------------- -------------------------------
RUN \
  ln -s /usr/include/x86_64-linux-gnu/asm /usr/include/x86_64-linux-musl/asm && \
  ln -s /usr/include/asm-generic /usr/include/x86_64-linux-musl/asm-generic && \
  ln -s /usr/include/linux /usr/include/x86_64-linux-musl/linux

WORKDIR /musl
RUN wget https://github.com/openssl/openssl/archive/OpenSSL_1_1_1f.tar.gz
RUN tar zxvf OpenSSL_1_1_1f.tar.gz
WORKDIR /musl/openssl-OpenSSL_1_1_1f/

RUN CC="musl-gcc -fPIE -pie" ./Configure no-shared no-async --prefix=/musl --openssldir=/musl/ssl linux-x86_64
RUN make depend
RUN make -j$(nproc)
RUN make install

RUN wget https://ziglang.org/download/0.11.0/zig-linux-x86_64-0.11.0.tar.xz &&\
  tar xvf zig-linux-x86_64-0.11.0.tar.xz && cp -R zig-linux-x86_64-0.11.0/* /usr/local/bin/

# ------------------------------- -------------------------------
#         Build the rust dependencies to speed cached builds
# ------------------------------- -------------------------------
WORKDIR /usr/src
# Download the target for static linking.
RUN rustup target add x86_64-unknown-linux-musl
RUN rustup target add x86_64-unknown-linux-gnu
RUN cargo install cargo-lambda
# Create a dummy project and build the app's dependencies.
# If the Cargo.toml or Cargo.lock files have not changed,
# we can use the docker build cache and skip these (typically slow) steps.
RUN USER=root cargo new jobber-oauth-handler
WORKDIR /usr/src/jobber-oauth-handler
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release
# ------------------------------- -------------------------------
#                Build the project from src
# ------------------------------- -------------------------------
ENV OPENSSL_DIR=/musl
COPY src ./src
RUN cargo lambda build --release

