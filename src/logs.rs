use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Local, TimeZone, Utc};
use serde::Serialize;
use syslog::SyslogMessage;
use syslog_rfc5424 as syslog;
use tokio::io::AsyncBufReadExt;

use crate::LINE_BUF_SIZE;

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
pub enum LogEvent {
    Connecting,
    Connected,
    Disconnected,
}

impl FromStr for LogEvent {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "connecting" => Ok(LogEvent::Connecting),
            "connected" => Ok(LogEvent::Connected),
            "disconnected" => Ok(LogEvent::Disconnected),
            _ => Err(anyhow!("unknown event `{}`", s)),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub event: LogEvent,
    pub time: DateTime<Local>,
    pub ip: IpAddr,
    pub port: u16,
    pub user: String,
    pub server: String,
}

impl TryFrom<&SyslogMessage> for LogEntry {
    type Error = anyhow::Error;
    fn try_from(log: &SyslogMessage) -> Result<Self, Self::Error> {
        // openvpn server 'ovpns1' user 'rust' address '1.1.1.1:11111' - connected
        const SPLITS: usize = 9;

        let msg = &log.msg;

        let time = Utc
            .timestamp_opt(
                log.timestamp
                    .ok_or_else(|| anyhow!("syslog message missing timestamp"))?,
                log.timestamp_nanos
                    .ok_or_else(|| anyhow!("syslog message missing timestamp nanos"))?,
            )
            .single()
            .ok_or_else(|| anyhow!("ambiguous timestamp"))?
            .with_timezone(&Local);

        let mut splitter = msg.split(' ');
        let mut splits = [""; SPLITS];
        for split in splits.iter_mut() {
            *split = splitter
                .next()
                .ok_or_else(|| anyhow!("msg doesn't split into enough parts"))?;
        }
        if splitter.next().is_some() {
            anyhow::bail!("msg splits into too many parts");
        }

        let sock_addr = SocketAddr::from_str(splits[6].trim_matches('\''))
            .context("invalid socket address in message")?;

        Ok(LogEntry {
            event: LogEvent::from_str(splits[8])?,
            time,
            ip: sock_addr.ip(),
            port: sock_addr.port(),
            user: splits[4].trim_matches('\'').to_string(),
            server: splits[2].trim_matches('\'').to_string(),
        })
    }
}

pub async fn filter_from_logs() -> anyhow::Result<Vec<LogEntry>> {
    let mut logs = tokio::io::BufReader::new(
        tokio::fs::OpenOptions::default()
            .read(true)
            .open("/var/log/openvpn.log")
            .await
            .context("couldn't open openvpn log file")?,
    );

    let mut line_buffer = String::with_capacity(LINE_BUF_SIZE);
    let mut relevant = Vec::new();
    loop {
        // check for end of file
        if logs.read_line(&mut line_buffer).await.unwrap() == 0 {
            break;
        }
        let line = line_buffer.trim();

        if !line.ends_with("connected")
            && !line.ends_with("disconnected")
            && !line.ends_with("connecting")
        {
            line_buffer.clear();
            continue;
        }

        let syslog = syslog::parse_message(line).context("couldn't parse syslog msg")?;
        relevant.push(LogEntry::try_from(&syslog)?);
        line_buffer.clear();
    }

    Ok(relevant)
}
