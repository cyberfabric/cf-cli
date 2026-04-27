ARG BUILDER_IMG_HOST="hub.docker.io"
ARG BUILDER_IMG_NAME="rust"
ARG BUILDER_IMG_TAG="1-bookworm"
ARG BUILD_MODE="release"
ARG BUILDER_MANIFEST
ARG BUILDER_FLAGS=""
ARG ARTIFACT_NAME

ARG RUNNER_IMG_HOST="hub.docker.io"
ARG RUNNER_IMG_NAME="debian"
ARG RUNNER_IMG_TAG="bookworm-slim"
ARG LOCAL_CONFIG_PATH
ARG CONFIG_EXT="yml"

FROM ${BUILDER_IMG_HOST}/${BUILDER_IMG_NAME}:${BUILDER_IMG_TAG} as builder

RUN apt update && \
    apt install -y --no-install-recommends protobuf-compiler libprotobuf-dev && \
    rm -rf /var/lib/apt/lists/*

ENV PROTOC_INCLUDE=/usr/include

WORKDIR /app

COPY . .
RUN set -eu; \
    case $BUILD_MODE in \
      debug) BUILD_FLAG="" ;; \
      release) BUILD_FLAG="--release" ;; \
      *) echo "Invalid BUILD_MODE: $BUILD_MODE. Expected: debug or release" >&2; exit 1 ;; \
    esac; \
    cargo build --manifest-path=$BUILDER_MANIFEST --target-dir /app/target $BUILD_FLAG $BUILDER_FLAGS


FROM ${RUNNER_IMG_HOST}/${RUNNER_IMG_NAME}:${RUNNER_IMG_TAG} as runner

WORKDIR /app

COPY --from=builder /app/target/$BUILD_MODE/$ARTIFACT_NAME /app/$ARTIFACT_NAME
COPY $LOCAL_CONFIG_PATH /app/config/config.$CONFIG_EXT

RUN apt update && \
    apt install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* \
    useradd --system --uid 10001 --shell /usr/sbin/nologin -d /app app && \
    chown -R app:app /app

ENV RUST_LOG=info
ENV CF_CLI_CONFIG="/app/config/$RUNNER_CONFIG"

USER app

ENTRYPOINT ["/app/$ARTIFACT_NAME"]
