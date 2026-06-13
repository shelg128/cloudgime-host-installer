use actix_files::{Files, NamedFile};
use actix_web::{
    HttpResponse, Result as ActixResult, dev::HttpServiceFactory, get, services, web::Data,
};
use common::{api_bindings::ConfigJs, api_bindings_ext::TsAny};
use log::warn;

use crate::app::App;

pub fn web_service() -> impl HttpServiceFactory {
    #[cfg(debug_assertions)]
    let files = Files::new("/", "dist").index_file("index.html");

    #[cfg(not(debug_assertions))]
    let files = Files::new("/", "static").index_file("index.html");

    services![host_selector_index_no_slash, host_selector_index, files]
}

pub fn web_config_js_service() -> impl HttpServiceFactory {
    services![config_js]
}
pub fn assetlinks_json_service() -> impl HttpServiceFactory {
    services![assetlinks_json]
}

#[cfg(debug_assertions)]
const WEB_INDEX_PATH: &str = "dist/index.html";

#[cfg(not(debug_assertions))]
const WEB_INDEX_PATH: &str = "static/index.html";

#[get("/h/{host_id}")]
async fn host_selector_index_no_slash() -> ActixResult<NamedFile> {
    Ok(NamedFile::open_async(WEB_INDEX_PATH).await?)
}

#[get("/h/{host_id}/")]
async fn host_selector_index() -> ActixResult<NamedFile> {
    Ok(NamedFile::open_async(WEB_INDEX_PATH).await?)
}

#[get("/.well-known/assetlinks.json")]
async fn assetlinks_json() -> HttpResponse {
    HttpResponse::Ok()
        .append_header(("Content-Type", "application/json"))
        .append_header((
            "Cache-Control",
            "no-store, no-cache, must-revalidate, private",
        ))
        .body(include_str!("../web/.well-known/assetlinks.json"))
}

#[get("/config.js")]
async fn config_js(app: Data<App>) -> HttpResponse {
    let config_json = match serde_json::to_string(&ConfigJs {
        path_prefix: app.config().web_server.url_path_prefix.clone(),
        default_settings: app.config().default_settings.clone().map(TsAny::from),
    }) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                "failed to create the web config.js. The Web Interface might fail to load! {err:?}"
            );

            return HttpResponse::InternalServerError().finish();
        }
    };
    let config_js = format!("export default {config_json}");

    HttpResponse::Ok()
        .append_header(("Content-Type", "text/javascript"))
        .body(config_js)
}
