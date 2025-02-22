mod api;
mod config;
mod database;
mod github;
mod ml;

use clap::{Parser, Subcommand};
use color_eyre::eyre::{Result, WrapErr};
use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::sync::{Arc, LazyLock};
use tracing::info;
use tracing::subscriber::set_global_default;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{prelude::*, Registry};

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

    //I think these are enabled by default but Ill leave this here just in case you want to enable
    //it.
    //https://github.com/open-telemetry/opentelemetry-rust/issues/761
    //let filter_otel = EnvFilter::new("info")
    //.add_directive("hyper=off".parse().unwrap())
    //.add_directive("opentelemetry=off".parse().unwrap())
    //.add_directive("tonic=off".parse().unwrap())
    //.add_directive("h2=off".parse().unwrap())
    //.add_directive("reqwest=off".parse().unwrap());

    let tracer_provider = init_traces();

    let otel_layer = OpenTelemetryLayer::new(tracer_provider.tracer("bountybot")); // .with_filter(filter_otel);

    //u can comment this and end of following line out to disable the console printing.
    let fmt_layer = tracing_subscriber::fmt::layer().pretty();

    let subscriber = Registry::default().with(otel_layer).with(fmt_layer); //<-;
    set_global_default(subscriber).expect("Failed to set subscriber.");

    //sets globals
    global::set_tracer_provider(tracer_provider.clone());

    let meter_provider = init_metrics();
    global::set_meter_provider(meter_provider.clone());

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
        let _patch = github_client
            .get_commit_patch(owner, repo, &commit.oid)
            .await
            .map_err(|e| {
                eprintln!("Failed to get commit patch: {:?}", e);
                e
            })?;

        Ok(())
    }
}
