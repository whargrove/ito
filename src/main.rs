use std::net::SocketAddr;

use anyhow::Result;
use askama::Template;
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Router, Server,
};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::Deserialize;
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    // todo path to db from config
    let manager = SqliteConnectionManager::file("./data/ito.db");
    let pool = r2d2::Pool::new(manager)?;
    pool.get()?.execute_batch(
        "BEGIN;
        CREATE TABLE IF NOT EXISTS links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            alias TEXT NOT NULL,
            target_url TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_links_alias ON links (alias);
        COMMIT;",
    )?;

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/favicon.ico", get(favicon))
        .route("/:alias", get(redirect_to_target))
        .route("/links", post(create_link))
        .route("/links/:id", delete(delete_link))
        .with_state(pool);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    Server::bind(&addr).serve(app.into_make_service()).await?;
    Ok(())
}

struct ItoError {
    err: anyhow::Error,
    sc: StatusCode,
}

impl IntoResponse for ItoError {
    fn into_response(self) -> Response {
        (self.sc, format!("Error: {}", self.err)).into_response()
    }
}

impl<E> From<E> for ItoError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self {
            err: err.into(),
            sc: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

type ItoPool = Pool<SqliteConnectionManager>;

#[derive(Template)]
#[template(path = "root.html")]
#[allow(dead_code)]
struct RootTemplate {
    links: Vec<Link>,
}

#[allow(dead_code)]
struct Link {
    id: i64,
    alias: String,
    target_url: Url,
}

struct HtmlTemplate<T>(T);

impl<T> IntoResponse for HtmlTemplate<T>
where
    T: Template,
{
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => Html(html).into_response(),
            Err(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to render template, error={}", err),
            )
                .into_response(),
        }
    }
}

async fn root_handler(State(pool): State<ItoPool>) -> Result<impl IntoResponse, ItoError> {
    let conn = pool.get()?;
    let mut statement = conn.prepare("SELECT id, alias, target_url from links")?;
    let links_rows = statement.query_map([], |row| {
        Ok(Link {
            id: row.get(0)?,
            alias: row.get(1)?,
            target_url: row.get(2)?,
        })
    })?;
    let mut links = Vec::new();
    for link in links_rows {
        links.push(link?);
    }
    let template = RootTemplate { links };
    Ok(HtmlTemplate(template))
}

#[derive(Deserialize, Debug)]
struct CreateLinkInput {
    alias: String,
    target_url: Url,
}

async fn create_link(
    State(pool): State<ItoPool>,
    Form(input): Form<CreateLinkInput>,
) -> Result<impl IntoResponse, ItoError> {
    let conn = pool.get()?;
    conn.execute(
        "INSERT INTO links (alias, target_url) VALUES (?1, ?2)",
        params![input.alias, input.target_url],
    )
    .map_err(handle_sqlite_err)?;
    return Ok(Redirect::to("/"));
}

async fn delete_link(
    State(pool): State<ItoPool>,
    Path(link_id): Path<i64>,
) -> Result<(), ItoError> {
    let conn = pool.get()?;
    conn.execute("DELETE FROM links WHERE id = ?", [link_id])?;
    Ok(())
}

async fn redirect_to_target(
    State(pool): State<ItoPool>,
    Path(link_alias): Path<String>,
) -> Result<impl IntoResponse, ItoError> {
    let conn = pool.get()?;
    let target_url: Url = conn
        .query_row_and_then(
            "SELECT target_url FROM links WHERE alias = ?",
            [link_alias],
            |row| row.get(0),
        )
        .map_err(handle_sqlite_err)?;
    Ok(Redirect::to(&target_url.to_string()))
}

fn handle_sqlite_err(err: rusqlite::Error) -> ItoError {
    match err {
        rusqlite::Error::SqliteFailure(inner_err, _) => {
            let sc = match inner_err.code {
                rusqlite::ErrorCode::ConstraintViolation => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            ItoError {
                err: err.into(),
                sc,
            }
        }
        rusqlite::Error::QueryReturnedNoRows => ItoError {
            err: err.into(),
            sc: StatusCode::NOT_FOUND,
        },
        _ => ItoError {
            err: err.into(),
            sc: StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}
