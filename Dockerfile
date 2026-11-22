# Static musl binary on an empty base image: no shell, no package manager,
# nothing to patch but oldpad itself.
FROM rust:1.98-alpine AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src src
COPY web web
# Shown on the admin's settings page.
ARG OLDPAD_COMMIT=""
ENV OLDPAD_COMMIT=$OLDPAD_COMMIT
RUN cargo build --release --locked && mkdir -m 700 /empty

FROM scratch
COPY --from=build /src/target/release/oldpad /oldpad
# A new named volume takes this owner, so the unprivileged user can save to it.
COPY --from=build --chown=65534:65534 /empty /data
ENV OLDPAD_BIND=0.0.0.0:8080 OLDPAD_DATA_DIR=/data
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/oldpad"]
