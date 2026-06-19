mod app;
mod bot;
mod config;
mod provider;
mod streaming;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_target(true)
        .with_line_number(true)
        .init();

    let config = config::Config::from_env()?;
    let (state, updates_rx) = app::AppState::new(config).await?;

    bot::run(state, updates_rx).await
}
