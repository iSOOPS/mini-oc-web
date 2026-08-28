use mini_oc_web::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,mini_oc_web=debug".into()),
        )
        .init();

    let state = AppState::from_env().await?;
    let bind = format!("{}:{}", state.config.web_bind, state.config.web_port);
    tracing::info!(%bind, "mini-oc-web starting");

    let app = mini_oc_web::routes::build_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}