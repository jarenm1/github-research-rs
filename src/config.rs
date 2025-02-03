use color_eyre::eyre::{Result, WrapErr};
use std::env;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct Config {
    pub github_token: String,
    pub mongo_uri: String,
    pub github_graphql_api: String,
    pub db_name: String,
    pub collection_name: String,
    pub host: IpAddr,
    pub port: u16,
    pub default_branch: String,
    pub commits_per_page: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

impl Config {
    pub fn new() -> Result<Self> {
        dotenv::dotenv().ok();

        Ok(Self {
            github_token: env::var("GITHUB_TOKEN")
                .wrap_err("GITHUB_TOKEN environment variable must be set")?,
            mongo_uri: env::var("MONGO_URI")
                .unwrap_or_else(|_| "mongodb://admin:password123@localhost:27017".to_string()),
            github_graphql_api: "https://api.github.com/graphql".to_string(),
            db_name: "commit_db".to_string(),
            collection_name: "commits".to_string(),
            host: "0.0.0.0".parse().wrap_err("Invalid host IP address")?,
            port: 8000,
            default_branch: "main".to_string(),
            commits_per_page: 50,
        })
    }
}
