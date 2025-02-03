mod api;
mod config;
mod database;
mod github;
mod ml;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, WrapErr};
use opentelemetry::{global, trace::TracerProvider, InstrumentationScope, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::{trace, Resource};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{Arc, LazyLock};
use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::{prelude::*, EnvFilter, Registry};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Process {
        owner: String,
        repo: String,
        #[arg(default_value = "main")]
        branch: String,
    },
    User {
        username: String,
    },
    Serve,
}

static RESOURCE: LazyLock<Resource> = LazyLock::new(|| {
    Resource::builder()
        .with_service_name("basic-otlp-example-http")
        .build()
});

fn init_logs() -> SdkLoggerProvider {
    let exporter = LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .expect("Failed to create log exporter");

    SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(RESOURCE.clone())
        .build()
}

fn init_traces() -> SdkTracerProvider {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary) //can be changed to `Protocol::HttpJson` to export in JSON format
        .build()
        .expect("Failed to create trace exporter");

    SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(RESOURCE.clone())
        .build()
}

fn init_metrics() -> SdkMeterProvider {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary) //can be changed to `Protocol::HttpJson` to export in JSON format
        .build()
        .expect("Failed to create metric exporter");

    SdkMeterProvider::builder()
        .with_periodic_exporter(exporter)
        .with_resource(RESOURCE.clone())
        .build()
}

#[tokio::main]
async fn main() -> Result<()> {
    let logger_provider = init_logs();

    // Create a new OpenTelemetryTracingBridge using the above LoggerProvider.
    let otel_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    // let exporter = LogExporter::builder()
    //     .with_http()
    //     .with_protocol(Protocol::HttpBinary)
    //     .build()
    //     .expect("Failed to create log exporter");

    // SdkLoggerProvider::builder()
    //     .with_batch_exporter(exporter)
    //     .with_resource(RESOURCE.clone())
    //     .build();
    //
    //

    // let tracer = opentelemetry_otlp::SpanExporter

    // // Set up file-based JSON tracing
    // let file = std::fs::OpenOptions::new()
    //     .create(true)
    //     .append(true)
    //     .open("traces.json")
    //     .wrap_err("Failed to open traces.json")?;
    //
    // // Create a JSON exporter that writes to our file
    // let exporter = opentelemetry_stdout::SpanExporter::builder()
    //     .with_writer(file)
    //     .build();
    //
    // // Create a new tracer provider
    // let provider = trace::TracerProvider::builder()
    //     .with_simple_exporter(exporter)
    //     .with_config(
    //         trace::config().with_resource(Resource::new(vec![KeyValue::new(
    //             "service.name",
    //             "github-research",
    //         )])),
    //     )
    //     .build();
    //
    // // Set it as the global provider
    // global::set_tracer_provider(provider);
    //
    // let tracer = global::tracer("github-research");
    // let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    //
    // // Set up the JSON formatting layer
    // let fmt_layer = tracing_subscriber::fmt::layer()
    //     .json()
    //     .with_file(true)
    //     .with_line_number(true)
    //     .with_thread_ids(true)
    //     .with_target(true)
    //     .with_level(true)
    //     .with_current_span(true)
    //     .with_span_list(true);
    //
    // let filter_layer = EnvFilter::try_from_default_env()
    //     .or_else(|_| EnvFilter::try_new("debug"))
    //     .wrap_err("Failed to create EnvFilter")?;
    tracing_subscriber::registry()
        .with(otel_layer)
        // .with(fmt_layer)
        .init();

    let tracer_provider = init_traces();
    global::set_tracer_provider(tracer_provider.clone());

    let meter_provider = init_metrics();
    global::set_meter_provider(meter_provider.clone());

    let common_scope_attributes = vec![KeyValue::new("scope-key", "scope-value")];
    let scope = InstrumentationScope::builder("basic")
        .with_version("1.0")
        .with_attributes(common_scope_attributes)
        .build();

    // let tracer = global::tracer_with_scope(scope.clone());
    // let meter = global::meter_with_scope(scope);

    // let tracy_layer = tracing_tracy::TracyLayer::default();

    // Initialize the tracing subscriber
    // Registry::default()
    //     .with(tracy_layer)
    //     .with(otel_layer)
    //     // .with(filter_layer)
    //     // .with(fmt_layer)
    //     // .with(telemetry)
    //     .with(ErrorLayer::default())
    //     .try_init()
    //     .wrap_err("Failed to set up global tracing subscriber")?;

    color_eyre::install().wrap_err("Failed to install color-eyre error handler")?;

    // Register a shutdown handler
    // tokio::spawn(async move {
    //     tokio::signal::ctrl_c().await.unwrap();
    //     // Gracefully shut down the tracer provider
    //     drop(provider);
    // });

    // Load environment variables from .env file
    dotenv::dotenv().ok();

    let config = config::Config::new()?;
    let db = database::MongoDb::new(config.clone())
        .await
        .wrap_err("Failed to initialize MongoDB connection")?;
    let github_client = github::GitHubClient::new(config.clone());
    let machine_learning =
        ml::MachineLearning::new().wrap_err("Failed to initialize embedding generator")?;

    info!("Starting API server on {}:{}", config.host, config.port);
    let app_state = Arc::new(api::types::AppState {
        db,
        config: config.clone(),
        machine_learning,
        github_client,
    });
    let app = api::create_router(app_state);

    let listener = tokio::net::TcpListener::bind((config.host, config.port))
        .await
        .wrap_err_with(|| format!("Failed to bind server to {}:{}", config.host, config.port))?;
    axum::serve(listener, app)
        .await
        .wrap_err("Failed to start API server")?;

    tracer_provider.shutdown()?;
    meter_provider.shutdown()?;
    logger_provider.shutdown()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_repository() -> Result<()> {
        // Load test environment variables
        dotenv::dotenv().ok();

        // Initialize real components with test configuration
        let config = config::Config::new()?;
        let github_client = github::GitHubClient::new(config);

        // Use a smaller test repository
        let owner = "octocat";
        let repo = "Hello-World";
        let branch = "master";

        // Get commits
        let commits = github_client
            .get_commits(owner, repo, Some(branch), None)
            .await
            .map_err(|e| {
                eprintln!("Failed to get commits: {e:?}");
                e
            })?;

        assert!(!commits.is_empty(), "Should have found at least one commit");

        // Test processing the first commit
        let commit = &commits[0];
        let patch = github_client
            .get_commit_patch(owner, repo, &commit.oid)
            .await
            .map_err(|e| {
                eprintln!("Failed to get commit patch: {:?}", e);
                e
            })?;

        Ok(())
    }
}
