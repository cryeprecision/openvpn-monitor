use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BIND_IP: &str = "0.0.0.0:7505";
const SOCKETS: &[(&str, &str)] = &[
    ("server1", "/var/etc/openvpn/server1/sock"),
    ("server2", "/var/etc/openvpn/server2/sock"),
];

type Socket = tokio::net::UnixStream;
// type Socket = std::sync::Arc<std::sync::RwLock<tokio::net::UnixStream>>;

pub struct State {
    pub socket: Socket,
}

pub fn init_logger() -> Result<()> {
    use log::LevelFilter;
    use simplelog::{ColorChoice, ConfigBuilder, TermLogger, TerminalMode};

    TermLogger::init(
        LevelFilter::Info,
        ConfigBuilder::default()
            .set_time_format_rfc2822()
            .set_target_level(LevelFilter::Info)
            .set_time_offset_to_local()
            .unwrap()
            .build(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .context("couldn't init logger")
}

#[derive(serde::Serialize)]
struct OpenVpn<'a> {
    server: &'a str,
    welcome: &'a str,
    output: Vec<&'a str>,
}

impl<'a> OpenVpn<'a> {
    pub fn from_output(server: &'a str, welcome: &'a str, output: &'a str) -> OpenVpn<'a> {
        OpenVpn {
            server,
            welcome,
            output: output.split("\r\n").collect(),
        }
    }
}

#[actix_web::get("/openvpn/{server}")]
async fn get_server(path: web::Path<String>) -> actix_web::Result<HttpResponse> {
    let path = path.as_str();
    let socket_path = match SOCKETS.iter().find(|(name, _)| *name == path) {
        Some((_, path)) => *path,
        None => return Ok(HttpResponse::NotFound().finish()),
    };

    log::info!("connecting to socket");
    let mut socket = tokio::net::UnixStream::connect(socket_path).await.unwrap();
    let (mut sr, mut sw) = socket.split();

    let mut buf = vec![0u8; 4096];
    let mut out = String::with_capacity(2048);
    let mut out_pre = String::with_capacity(2048);

    log::info!("reading welcome message");
    let count = sr.read(&mut buf).await.unwrap();
    out_pre.push_str(
        std::str::from_utf8(&buf[..count])
            .unwrap_or("invalid-utf8")
            .trim(),
    );

    log::info!("writing to socket");
    sw.write_all(b"status 2\n").await.unwrap();

    log::info!("reading command output");
    let count = sr.read(&mut buf).await.unwrap();
    out.push_str(
        std::str::from_utf8(&buf[..count])
            .unwrap_or("invalid-utf8")
            .trim(),
    );

    log::info!("returning result");
    let resp = OpenVpn::from_output(path, &out_pre, &out);
    Ok(HttpResponse::Ok().json(&resp))
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    init_logger().unwrap();

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .service(web::scope("/api").service(get_server))
    });

    server
        .bind(BIND_IP)
        .unwrap()
        .workers(1)
        .run()
        .await
        .unwrap();
}
