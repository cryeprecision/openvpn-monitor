use std::ops::ControlFlow;

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const BIND_IP: &str = "0.0.0.0:7505";

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

struct OpenVpnMgmnt {
    sock_read: BufSockRead,
    sock_write: BufSockWrite,
}

impl OpenVpnMgmnt {
    pub async fn open(server_name: &str) -> anyhow::Result<OpenVpnMgmnt> {
        if !lazy_regex::regex_is_match!("^[a-z0-9]+$", server_name) {
            anyhow::bail!("openvpn server name can only contain lowercase letters and numbers");
        }

        let sock_path = format!("/var/etc/openvpn/{}/sock", server_name);
        let (sock_read, sock_write) = tokio::net::UnixStream::connect(&sock_path)
            .await
            .context("couldn't connect to openvpn socket")?
            .into_split();
        let mut sock_read = tokio::io::BufReader::with_capacity(2048, sock_read);
        let sock_write = tokio::io::BufWriter::with_capacity(2048, sock_write);

        let mut welcome = String::with_capacity(256);
        let _ = sock_read
            .read_line(&mut welcome)
            .await
            .context("couldn't read welcome message from socket")?;
        log::info!("read welcome message: `{}`", welcome);

        Ok(OpenVpnMgmnt {
            sock_read,
            sock_write,
        })
    }
    pub async fn execute<F>(&mut self, cmd: &str, mut f: F) -> anyhow::Result<()>
    where
        F: FnMut(&str) -> std::ops::ControlFlow<()>,
    {
        self.sock_write
            .write_all(cmd.as_bytes())
            .await
            .context("couldn't write command to socket")?;
        self.sock_write
            .write_u8(b'\n')
            .await
            .context("couldn't write newline after command to socket")?;
        self.sock_write
            .flush()
            .await
            .context("couldn't flush command to socket")?;

        let mut line_buf = String::with_capacity(1024);
        loop {
            let _ = self
                .sock_read
                .read_line(&mut line_buf)
                .await
                .context("couldn't read next line from socket")?;
            if f(&line_buf).is_break() {
                break;
            }
            line_buf.clear();
        }

        Ok(())
    }
}

#[actix_web::get("/openvpn/{server}")]
async fn get_server(path: web::Path<String>) -> HttpResponse {
    log::info!("connecting to socket");
    let mut openvpn = match OpenVpnMgmnt::open(&path).await {
        Err(err) => {
            log::error!("{}", err);
            return HttpResponse::InternalServerError().finish();
        }
        Ok(v) => v,
    };

    log::info!("executing command on socket");
    if let Err(err) = openvpn
        .execute("status 2", |line| {
            log::info!("line: `{}`", line);
            if line.contains("END") || line.contains("ERROR") {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .await
    {
        log::error!("{}", err);
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().finish()
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
