FROM rust:1.66 as build
# create a shell project
RUN USER=root cargo new --bin ito
WORKDIR /ito
# copy manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
# install dependencies
RUN cargo build --release
# ensure there's no source
RUN rm src/*.rs
# copy source
COPY ./src ./src
# includes templates, required to compile
COPY ./templates ./templates
# build release
RUN rm ./target/release/deps/ito*
RUN cargo build --release

# runtime container
FROM debian:buster-slim
RUN apt-get update && apt-get install -y libsqlite3-dev
COPY --from=build /ito/target/release/ito .
CMD ["./ito"]
