#![allow(dead_code)]

mod logs;
mod socket;

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};

use crate::socket::OpenVpnMgmnt;

pub const BIND_IP: &str = "0.0.0.0:7505";
pub const LINE_BUF_SIZE: usize = 1024;
pub const SOCK_BUF_SIZE: usize = 2048;

type BufSockRead = tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>;
type BufSockWrite = tokio::io::BufWriter<tokio::net::unix::OwnedWriteHalf>;

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
    pub fn _from_output(server: &'a str, welcome: &'a str, output: &'a str) -> OpenVpn<'a> {
        OpenVpn {
            server,
            welcome,
            output: output.split("\r\n").collect(),
        }
    }
}

#[actix_web::get("/openvpn/status/{server}")]
async fn get_server(path: web::Path<String>) -> HttpResponse {
    let mut openvpn = match OpenVpnMgmnt::connect(&path).await {
        Err(err) => {
            log::error!("{}", err);
            return HttpResponse::InternalServerError().finish();
        }
        Ok(v) => v,
    };

    let start = std::time::Instant::now();
    let val = match openvpn.execute_to_map("status 2", "CLIENT_LIST").await {
        Err(err) => {
            log::error!("{}", err);
            return HttpResponse::InternalServerError().finish();
        }
        Ok(v) => v,
    };
    let elapsed_ms = start.elapsed().as_secs_f64() * 1e3;

    HttpResponse::Ok().json(serde_json::json!({
        "elapsed_ms": elapsed_ms,
        "data": val,
    }))
}

#[actix_web::get("/openvpn/auth/{server}")]
async fn get_auth(path: web::Path<String>) -> HttpResponse {
    if !lazy_regex::regex_is_match!("^[a-z0-9]+$", &path) {
        log::error!("invalid server name `{}`", path);
        return HttpResponse::InternalServerError().finish();
    }

    let start = std::time::Instant::now();
    let logs = match logs::filter_from_logs().await {
        Err(err) => {
            log::error!("{}", err);
            return HttpResponse::InternalServerError().finish();
        }
        Ok(v) => v,
    };
    let elapsed_ms = start.elapsed().as_secs_f64() * 1e3;

    HttpResponse::Ok().json(serde_json::json!({
        "elapsed_ms": elapsed_ms,
        "data": logs,
    }))
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    init_logger().unwrap();

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .service(web::scope("/api").service(get_server).service(get_auth))
    });

    server
        .bind(BIND_IP)
        .unwrap()
        .workers(1)
        .run()
        .await
        .unwrap();
}
