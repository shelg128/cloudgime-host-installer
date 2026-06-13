use common::config::Config;
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use serde_json::{Map, Value};
use std::{
    fs::OpenOptions,
    io::{self, ErrorKind, IsTerminal},
    path::PathBuf,
    str::FromStr,
};
use tokio::fs::{self};
use tracing::{Level, Span, level_filters::LevelFilter, span};
use tracing_actix_web::{RootSpanBuilder, TracingLogger};
use tracing_appender::non_blocking;
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};
use venator::Venator;

use actix_web::{
    App as ActixApp, HttpResponse, HttpServer,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    http::header::{self, HeaderMap},
    middleware::{self},
    web::{Data, get, scope},
};
use tracing::{error, info, trace, warn};

use crate::{
    api::api_service,
    app::App,
    cli::{Cli, Command},
    human_json::preprocess_human_json,
    web::{assetlinks_json_service, web_config_js_service, web_service},
};

mod api;
mod app;
mod web;

mod cli;
mod human_json;

#[actix_web::main]
async fn main() {
    init_rustls_crypto_provider();

    let cli = Cli::load();

    // Load Config
    let config_path = PathBuf::from_str(&cli.config_path).expect("invalid config file path");
    let config = match fs::read_to_string(&config_path).await {
        Ok(mut value) => {
            value = preprocess_human_json(value);

            let mut config = serde_json::from_str(&value).expect("invalid file");
            cli.options.apply(&mut config);
            config
        }
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let mut new_config = Config::default();
            cli.options.apply(&mut new_config);

            let value_str =
                serde_json::to_string_pretty(&new_config).expect("failed to serialize file");

            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .expect("failed to create directories to file");
            }
            fs::write(&config_path, value_str)
                .await
                .expect("failed to write default file");

            new_config
        }
        Err(err) => panic!("failed to read file: {err}"),
    };
    match cli.command {
        Some(Command::PrintConfig) => {
            let json =
                serde_json::to_string_pretty(&config).expect("failed to serialize config to json");
            println!("{json}");
            return;
        }
        None | Some(Command::Run) => {
            // Fallthrough
        }
    }

    let guard = init_log(&config);

    if let Err(err) = start(config).await {
        error!("{err:?}");
    }

    drop(guard);
}

fn init_rustls_crypto_provider() {
    if CryptoProvider::get_default().is_none() {
        let _ = aws_lc_rs::default_provider().install_default();
    }
}

fn init_log(config: &Config) -> Option<non_blocking::WorkerGuard> {
    let config_level_filter = match config.log.level_filter {
        log::LevelFilter::Off => LevelFilter::OFF,
        log::LevelFilter::Error => LevelFilter::ERROR,
        log::LevelFilter::Info => LevelFilter::INFO,
        log::LevelFilter::Warn => LevelFilter::WARN,
        log::LevelFilter::Debug => LevelFilter::DEBUG,
        log::LevelFilter::Trace => LevelFilter::TRACE,
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(config_level_filter.into())
        .from_env_lossy()
        // Add default directives
        .add_directive(
            "actix_http::h1=off"
                .parse()
                .expect("failed to add actix-web tracing directive"),
        )
        .add_directive(
            "mio::poll=off"
                .parse()
                .expect("failed to add mio tracing directive"),
        );

    #[cfg(windows)]
    enable_ansi_windows();

    let stdout_layer = fmt::layer()
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(io::stdout().is_terminal());

    let (file_layer, guard) = if let Some(log_file) = &config.log.file_path {
        let file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(log_file)
            .expect("failed to open log file");

        let (writer, guard) = non_blocking(file);

        let fmt_layer = fmt::layer()
            .with_span_events(FmtSpan::FULL)
            .with_writer(writer)
            .with_ansi(false);

        (Some(fmt_layer), Some(guard))
    } else {
        (None, None)
    };

    let venator = config.log.dev_venator.then(Venator::default);

    Registry::default()
        .with(venator)
        .with(env_filter.clone())
        .with(file_layer)
        .with(stdout_layer)
        .init();

    trace!("Using env_filter: {env_filter}");

    guard
}

#[cfg(windows)]
fn enable_ansi_windows() {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, SetConsoleMode,
    };

    unsafe {
        let handle = io::stdout().as_raw_handle();
        let mut mode = 0;
        if GetConsoleMode(handle as _, &mut mode) != 0 {
            SetConsoleMode(handle as _, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

struct ActixDebugSpan;

impl ActixDebugSpan {
    fn sanitize_headers(headers: &HeaderMap) -> Vec<(String, String)> {
        const SENSITIVE: &[&str] = &["authorization", "cookie", "set-cookie"];

        headers
            .iter()
            .map(|(name, value)| {
                let name_str = name.as_str().to_string();

                let value_str = if SENSITIVE.contains(&name_str.to_ascii_lowercase().as_str()) {
                    "<redacted>".to_string()
                } else {
                    value.to_str().unwrap_or("<binary>").to_string()
                };

                (name_str, value_str)
            })
            .collect()
    }
}

impl RootSpanBuilder for ActixDebugSpan {
    fn on_request_start(request: &ServiceRequest) -> Span {
        if tracing::enabled!(Level::TRACE) {
            span!(
                Level::TRACE,
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
                headers = ?Self::sanitize_headers(request.headers()),
                peer_addr = ?request.peer_addr(),
            )
        } else {
            span!(
                Level::DEBUG,
                "http_request",
                method = %request.method(),
                uri = %request.uri(),
            )
        }
    }
    fn on_request_end<B: MessageBody>(
        _span: Span,
        _outcome: &Result<ServiceResponse<B>, actix_web::Error>,
    ) {
    }
}

async fn start(config: Config) -> Result<(), anyhow::Error> {
    if let Err(err) = repair_shared_pair_store_filesystem(&config).await {
        warn!("failed to repair shared pair store on startup: {err}");
    }

    let runtime_dir = std::env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing runtime dir"))?;
    let _bundle_root = runtime_dir
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing bundle root"))?;
    info!(
        "Persistent display guard is managed by the host boot guard task; runtime will not spawn a second display helper."
    );

    let app = App::new(config.clone()).await?;
    let app = Data::new(app);

    let bind_address = app.config().web_server.bind_address;
    let server = HttpServer::new({
        let url_path_prefix = config.web_server.url_path_prefix.clone();
        let app = app.clone();

        move || {
            let root_redirect_target = if url_path_prefix.ends_with('/') {
                url_path_prefix.clone()
            } else {
                format!("{url_path_prefix}/")
            };
            ActixApp::new()
                .wrap(TracingLogger::<ActixDebugSpan>::new())
                .service(assetlinks_json_service())
                .route(
                    "/",
                    get().to({
                        let root_redirect_target = root_redirect_target.clone();
                        move || {
                            let root_redirect_target = root_redirect_target.clone();
                            async move {
                                HttpResponse::TemporaryRedirect()
                                    .append_header((header::LOCATION, root_redirect_target))
                                    .finish()
                            }
                        }
                    }),
                )
                .service(
                    scope(&url_path_prefix)
                        .app_data(app.clone())
                        .wrap(
                            // TODO: maybe only re cache when required?
                            middleware::DefaultHeaders::new()
                                .add((
                                    "Cache-Control",
                                    "no-store, no-cache, must-revalidate, private",
                                ))
                                .add(("Clear-Site-Data", "\"cache\""))
                                .add(("Pragma", "no-cache"))
                                .add(("Expires", "0")),
                        )
                        .service(api_service())
                        .service(web_config_js_service())
                        .service(web_service()),
                )
        }
    });

    if let Some(certificate) = app.config().web_server.certificate.as_ref() {
        info!("[Server]: Running Https Server with ssl tls");

        let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls())
            .expect("failed to create ssl tls acceptor");
        builder
            .set_private_key_file(&certificate.private_key_pem, SslFiletype::PEM)
            .expect("failed to set private key");
        builder
            .set_certificate_chain_file(&certificate.certificate_pem)
            .expect("failed to set certificate");

        server.bind_openssl(bind_address, builder)?.run().await?;
    } else {
        server.bind(bind_address)?.run().await?;
    }

    Ok(())
}

async fn repair_shared_pair_store_filesystem(config: &Config) -> Result<(), anyhow::Error> {
    let common::config::StorageConfig::Json { path, .. } = &config.data_storage;
    let relative_data_path = PathBuf::from(path);
    let runtime_dir = std::env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("missing runtime dir"))?;
    let data_path = if relative_data_path.is_absolute() {
        relative_data_path
    } else {
        runtime_dir.join(relative_data_path)
    };
    let Some(server_dir) = data_path.parent() else {
        return Ok(());
    };
    let shared_path = server_dir.join("shared_pair_info.json");

    if !fs::try_exists(&data_path).await.unwrap_or(false) {
        return Ok(());
    }

    let mut data_root = serde_json::from_str::<Value>(&fs::read_to_string(&data_path).await?)?;
    let mut shared_root = if fs::try_exists(&shared_path).await.unwrap_or(false) {
        serde_json::from_str::<Value>(&fs::read_to_string(&shared_path).await?)?
    } else {
        Value::Object(Map::from_iter([
            ("version".to_string(), Value::from(1)),
            ("hosts".to_string(), Value::Array(Vec::new())),
        ]))
    };

    let Some(data_hosts) = data_root.get_mut("hosts").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    let Some(shared_hosts) = shared_root.get_mut("hosts").and_then(Value::as_array_mut) else {
        return Ok(());
    };

    let mut shared_changed = false;
    let mut data_changed = false;
    let local_address = "127.0.0.1";
    let local_http_port = u64::from(config.moonlight.default_http_port);
    let default_owner = config.web_server.default_user_id.map(u64::from);
    let mut local_host_present = false;

    for (host_id, host_value) in data_hosts.iter_mut() {
        let Some(host_object) = host_value.as_object_mut() else {
            continue;
        };
        let Ok(host_id_numeric) = host_id.parse::<u64>() else {
            continue;
        };

        let address = host_object
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("127.0.0.1")
            .to_string();
        let mut http_port = host_object
            .get("http_port")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let local_loopback = matches!(address.as_str(), "127.0.0.1" | "localhost" | "::1");
        if local_loopback && http_port != local_http_port {
            host_object.insert("http_port".to_string(), Value::from(local_http_port));
            http_port = local_http_port;
            data_changed = true;
        }
        if local_loopback && http_port == local_http_port {
            local_host_present = true;
        }
        let pair_info = host_object.get("pair_info").cloned().unwrap_or(Value::Null);
        let shared_index = shared_hosts.iter().position(|entry| {
            entry
                .get("host_id")
                .and_then(Value::as_u64)
                .map(|value| value == host_id_numeric)
                .unwrap_or(false)
        });

        if !pair_info.is_null() {
            let replacement = Value::Object(Map::from_iter([
                ("host_id".to_string(), Value::from(host_id_numeric)),
                ("address".to_string(), Value::String(address)),
                ("http_port".to_string(), Value::from(http_port)),
                ("pair_info".to_string(), pair_info.clone()),
            ]));
            match shared_index {
                Some(index) if shared_hosts[index] != replacement => {
                    shared_hosts[index] = replacement;
                    shared_changed = true;
                }
                None => {
                    shared_hosts.push(replacement);
                    shared_changed = true;
                }
                _ => {}
            }
            continue;
        }

        if let Some(index) = shared_index {
            let shared_pair_info = shared_hosts[index]
                .get("pair_info")
                .cloned()
                .unwrap_or(Value::Null);
            if !shared_pair_info.is_null() {
                host_object.insert("pair_info".to_string(), shared_pair_info);
                data_changed = true;
            }
        }
    }

    if !local_host_present {
        let shared_local = shared_hosts.iter().find(|entry| {
            entry.get("address").and_then(Value::as_str) == Some(local_address)
                && entry.get("http_port").and_then(Value::as_u64) == Some(local_http_port)
        });

        let mut host_id = 4_100_364_999u64;
        while data_hosts.contains_key(&host_id.to_string()) {
            host_id = host_id.saturating_add(1);
        }

        let host_name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Local Host".to_string());
        let host_object = Value::Object(Map::from_iter([
            (
                "owner".to_string(),
                default_owner.map(Value::from).unwrap_or(Value::Null),
            ),
            (
                "address".to_string(),
                Value::String(local_address.to_string()),
            ),
            ("http_port".to_string(), Value::from(local_http_port)),
            (
                "pair_info".to_string(),
                shared_local
                    .and_then(|entry| entry.get("pair_info").cloned())
                    .unwrap_or(Value::Null),
            ),
            (
                "cache".to_string(),
                Value::Object(Map::from_iter([
                    ("name".to_string(), Value::String(host_name)),
                    ("mac".to_string(), Value::Null),
                ])),
            ),
        ]));

        data_hosts.insert(host_id.to_string(), host_object);
        data_changed = true;
        info!(
            "auto-registered bundled local host {}:{} as host_id={}",
            local_address, local_http_port, host_id
        );
    }

    if shared_changed || !fs::try_exists(&shared_path).await.unwrap_or(false) {
        fs::create_dir_all(server_dir).await?;
        fs::write(
            &shared_path,
            format!("{}\n", serde_json::to_string_pretty(&shared_root)?),
        )
        .await?;
    }
    if data_changed {
        fs::write(
            &data_path,
            format!("{}\n", serde_json::to_string_pretty(&data_root)?),
        )
        .await?;
        info!(
            "repaired host pair_info from shared pair store {}",
            shared_path.display()
        );
    }

    Ok(())
}
