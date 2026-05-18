use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::get,
};
use leptos::prelude::*;

const SITE_CSS: &str = include_str!("site.css");

pub fn app() -> Router {
    Router::new()
        .route("/", get(home))
        .route("/healthz", get(healthz))
        .route("/assets/site.css", get(stylesheet))
}

async fn home() -> Html<String> {
    Html(render_homepage())
}

async fn healthz() -> &'static str {
    "ok\n"
}

async fn stylesheet() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        SITE_CSS,
    )
}

pub fn render_homepage() -> String {
    view! { <Homepage/> }.to_html()
}

#[component]
fn Homepage() -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <meta
                    name="description"
                    content="SealPort is a planned all-Rust encrypted backup CLI for self-hosted, scriptable backups."
                />
                <title>"SealPort - encrypted backups, same everywhere"</title>
                <link rel="stylesheet" href="/assets/site.css"/>
            </head>
            <body>
                <main>
                    <section class="hero">
                        <div class="hero-copy">
                            <p class="eyebrow">"sealport.cc"</p>
                            <h1>"Encrypted backups. Same everywhere."</h1>
                            <p class="lead">
                                "SealPort is an all-Rust backup CLI being built for operators who need inspectable, scriptable, client-side encrypted backups without platform drama."
                            </p>
                            <div class="actions" aria-label="Primary links">
                                <a class="button primary" href="https://github.com/dunamismax/sealport">"View source"</a>
                                <a class="button secondary" href="#status">"Project status"</a>
                            </div>
                        </div>
                        <div class="terminal" aria-label="SealPort command preview">
                            <div class="terminal-bar">
                                <span></span>
                                <span></span>
                                <span></span>
                            </div>
                            <pre><code>"sealport init s3://company-backups/laptops\nsealport backup ~/Documents --tag laptop --jsonl\nsealport snapshots --json\nsealport restore latest ~/restore-test\nsealport check --read-data-subset 5%"</code></pre>
                        </div>
                    </section>

                    <section class="status-strip" id="status" aria-label="Current status">
                        <div>
                            <span class="label">"Status"</span>
                            <strong>"Pre-v1, pre-backup-engine"</strong>
                        </div>
                        <div>
                            <span class="label">"Stable command"</span>
                            <strong>"sealport"</strong>
                        </div>
                        <div>
                            <span class="label">"Runtime target"</span>
                            <strong>"CLI-only backup tool"</strong>
                        </div>
                    </section>

                    <section class="section">
                        <div class="section-heading">
                            <p class="eyebrow">"What it is for"</p>
                            <h2>"Owned backup infrastructure for people who restore under pressure."</h2>
                        </div>
                        <div class="feature-grid">
                            <article>
                                <h3>"Client-side encryption"</h3>
                                <p>"Repository contents and sensitive metadata are designed to be encrypted before they leave the machine."</p>
                            </article>
                            <article>
                                <h3>"Scriptable by default"</h3>
                                <p>"Stdout is data, stderr is diagnostics, and JSON or JSONL output is part of the command contract."</p>
                            </article>
                            <article>
                                <h3>"Local and S3-compatible targets"</h3>
                                <p>"The v1 storage path is focused on local filesystems and S3-compatible object storage before provider sprawl."</p>
                            </article>
                            <article>
                                <h3>"Cross-platform with evidence"</h3>
                                <p>"Windows, macOS, Linux, FreeBSD, and NetBSD support will be claimed only after CI, tests, and artifacts exist."</p>
                            </article>
                        </div>
                    </section>

                    <section class="section split">
                        <div>
                            <p class="eyebrow">"Design boundaries"</p>
                            <h2>"Small surface, serious repository model."</h2>
                            <p>
                                "SealPort is not a GUI, daemon, SaaS dashboard, scheduler, FUSE mount, or compatibility layer for another backup format. The first job is boring restore reliability."
                            </p>
                        </div>
                        <ul class="checklist">
                            <li>"Original encrypted repository format"</li>
                            <li>"Compressed, deduplicated snapshot target"</li>
                            <li>"Authenticated objects and tamper detection"</li>
                            <li>"Two-phase prune and interruption-safe operations planned"</li>
                        </ul>
                    </section>

                    <section class="section release">
                        <div>
                            <p class="eyebrow">"Self-hosting shape"</p>
                            <h2>"A lightweight Rust web server for this public homepage."</h2>
                            <p>
                                "This site is served by an Axum binary rendering Leptos views on the server. It has no database, no client-side app bundle, and a simple health endpoint for reverse proxies."
                            </p>
                        </div>
                        <div class="deploy">
                            <span>"Binary"</span>
                            <code>"sealport-web"</code>
                            <span>"Default bind"</span>
                            <code>"0.0.0.0:8080"</code>
                            <span>"Override"</span>
                            <code>"SEALPORT_WEB_ADDR=127.0.0.1:8080"</code>
                        </div>
                    </section>
                </main>
                <footer>
                    <span>"SealPort"</span>
                    <a href="https://github.com/dunamismax/sealport">"GitHub"</a>
                    <a href="https://sealport.cc/">"sealport.cc"</a>
                </footer>
            </body>
        </html>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body, http::Request};
    use tower::ServiceExt;

    #[test]
    fn homepage_renders_honest_project_status() {
        let html = render_homepage();

        assert!(html.contains("Encrypted backups. Same everywhere."));
        assert!(html.contains("Pre-v1, pre-backup-engine"));
        assert!(html.contains("sealport-web"));
        assert!(html.contains("/assets/site.css"));
    }

    #[tokio::test]
    async fn routes_serve_homepage_health_and_css() {
        let app = app();

        let home = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(home.status(), StatusCode::OK);

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let css = app
            .oneshot(
                Request::builder()
                    .uri("/assets/site.css")
                    .body(body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(css.status(), StatusCode::OK);
        assert_eq!(
            css.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/css; charset=utf-8"
        );
    }
}
