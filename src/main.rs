#![allow(dead_code)]

use std::{collections::HashMap, ops::ControlFlow};

use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

const BIND_IP: &str = "0.0.0.0:7505";
const LINE_BUF_SIZE: usize = 1024;
const SOCK_BUF_SIZE: usize = 2048;

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
    fn is_bad_line(line: &str) -> bool {
        line.contains("END") || line.contains("ERROR")
    }
    async fn next_line<'a>(&mut self, line_buf: &'a mut String) -> anyhow::Result<&'a str> {
        line_buf.clear();
        let _ = self
            .sock_read
            .read_line(line_buf)
            .await
            .context("couldn't read next line from socket")?;
        Ok(line_buf.trim())
    }

    pub async fn open(server_name: &str) -> anyhow::Result<OpenVpnMgmnt> {
        if !lazy_regex::regex_is_match!("^[a-z0-9]+$", server_name) {
            anyhow::bail!("openvpn server name can only contain lowercase letters and numbers");
        }

        let sock_path = format!("/var/etc/openvpn/{}/sock", server_name);
        let (sock_read, sock_write) = tokio::net::UnixStream::connect(&sock_path)
            .await
            .context("couldn't connect to openvpn socket")?
            .into_split();

        let sock_read = tokio::io::BufReader::with_capacity(SOCK_BUF_SIZE, sock_read);
        let sock_write = tokio::io::BufWriter::with_capacity(SOCK_BUF_SIZE, sock_write);

        let mut this = OpenVpnMgmnt {
            sock_read,
            sock_write,
        };

        // read and discard the welcome message
        let mut welcome = String::with_capacity(LINE_BUF_SIZE);
        let _ = this.next_line(&mut welcome).await?;

        Ok(this)
    }

    async fn write_command(&mut self, cmd: &str) -> anyhow::Result<()> {
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
            .context("couldn't flush command to socket")
    }

    pub async fn execute<F, T>(&mut self, cmd: &str, mut f: F) -> anyhow::Result<T>
    where
        F: FnMut(&str) -> std::ops::ControlFlow<T>,
    {
        let mut line_buf = String::with_capacity(LINE_BUF_SIZE);
        self.write_command(cmd).await?;

        let val = loop {
            let line = self.next_line(&mut line_buf).await?;
            if let ControlFlow::Break(v) = f(line) {
                break v;
            }
        };

        Ok(val)
    }
    pub async fn execute_to_map(
        &mut self,
        cmd: &str,
        key: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mut line_buf = String::with_capacity(LINE_BUF_SIZE);
        self.write_command(cmd).await?;

        // find the header that tells us the name of each field
        let keys = loop {
            let line = self.next_line(&mut line_buf).await?;

            if Self::is_bad_line(line) {
                anyhow::bail!("unexpected line `{}`", line)
            }

            // look for the header line for the given key,
            // skip the first two parts
            if line.contains("HEADER") && line.contains(key) {
                break line
                    .split(',')
                    .skip(2)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
            }
        };

        // parse the values, one row per connection
        let mut objs = Vec::new();
        loop {
            let line = self.next_line(&mut line_buf).await?;

            if Self::is_bad_line(line) {
                anyhow::bail!("unexpected line `{}`", line_buf.trim())
            }

            // lines are contiguous, if one line doesn't contain the key,
            // all following lines won't containt it either
            if !line.contains(key) {
                break;
            }

            // skip the header
            let values_ = line
                .split(',')
                .skip(1)
                .map(|s| s.to_string())
                .collect::<Vec<_>>();

            if values_.len() != keys.len() {
                anyhow::bail!(
                    "length of keys and values are different ({} != {})",
                    values_.len(),
                    keys.len()
                )
            }

            // collect key-value pairs into a hashmap
            objs.push(
                keys.clone()
                    .into_iter()
                    .zip(values_)
                    .collect::<HashMap<_, _>>(),
            );
        }

        serde_json::to_value(objs).context("couldn't convert maps to json value")
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

    let val = match openvpn.execute_to_map("status 2", "CLIENT_LIST").await {
        Err(err) => {
            log::error!("{}", err);
            return HttpResponse::InternalServerError().finish();
        }
        Ok(v) => v,
    };
    HttpResponse::Ok().json(&val)
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
