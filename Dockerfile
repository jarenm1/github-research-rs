# Builder stage
FROM rust:1.84.1-slim-bookworm as builder

WORKDIR /usr/src/app

# Install required system dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev curl && \
    rm -rf /var/lib/apt/lists/*

# Copy and build the application
COPY . .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /usr/src/app/target/release/github-research-rs /app/

# Set environment variables
ENV RUST_LOG=info

# Expose the port your application uses
EXPOSE 8000

# Run the binary
CMD ["./github-research-rs", "serve"]