use crate::alert::{chunk_message, Alert};
use crate::sender::Sender;
use anyhow::{anyhow, Result};
use std::io;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

struct CommandOutput {
    stdout: Vec<u8>,
}

trait CommandRunner {
    fn output(&self, program: &str, args: &[String]) -> io::Result<CommandOutput>;
    fn spawn(&self, program: &str, args: &[String]) -> io::Result<()>;
}

struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn output(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
        Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .output()
            .map(|output| CommandOutput {
                stdout: output.stdout,
            })
    }

    fn spawn(&self, program: &str, args: &[String]) -> io::Result<()> {
        Command::new(program).args(args).spawn().map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshtasticConfig {
    pub host: Option<String>,
    pub port: Option<String>,
}

pub struct MeshtasticSender {
    config: MeshtasticConfig,
    last_message_time: Option<Instant>,
    runner: Box<dyn CommandRunner>,
}

impl MeshtasticSender {
    pub fn new(config: MeshtasticConfig) -> Self {
        Self::with_runner(config, Box::new(RealCommandRunner))
    }

    fn with_runner(config: MeshtasticConfig, runner: Box<dyn CommandRunner>) -> Self {
        Self {
            config,
            last_message_time: None,
            runner,
        }
    }

    fn info_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(host) = &self.config.host {
            args.push("--host".to_string());
            args.push(host.clone());
        }

        if let Some(port) = &self.config.port {
            args.push("--port".to_string());
            args.push(port.clone());
        }

        args.push("--info".to_string());
        args
    }

    fn send_args(&self, chan: u32, message: &str) -> Vec<String> {
        let mut args = vec![
            "--no-nodes".to_string(),
            "--no-time".to_string(),
            "--ch-index".to_string(),
            chan.to_string(),
            "--sendtext".to_string(),
            message.to_string(),
            "--ack".to_string(),
        ];

        if let Some(host) = &self.config.host {
            args.push("--host".to_string());
            args.push(host.clone());
        }

        if let Some(port) = &self.config.port {
            args.push("--port".to_string());
            args.push(port.clone());
        }

        args
    }

    fn send_message_with_retry(
        &mut self,
        chan: u32,
        message: &str,
        retries: u32,
        delay: Duration,
    ) -> Result<()> {
        if let Some(last_time) = self.last_message_time {
            let elapsed = last_time.elapsed();
            if elapsed < Duration::from_secs(20) {
                sleep(Duration::from_secs(20) - elapsed);
            }
        }

        for attempt in 0..=retries {
            let result = self
                .runner
                .spawn("meshtastic", &self.send_args(chan, message));

            match result {
                Ok(_) => {
                    self.last_message_time = Some(Instant::now());
                    return Ok(());
                }
                Err(e) => {
                    if attempt < retries {
                        log::warn!("Error sending message: {}. Retrying in {:?}...", e, delay);
                        sleep(delay);
                    } else {
                        log::error!("Error sending message after {} attempts: {}", retries, e);
                        return Err(anyhow!("Failed to send message: {}", e));
                    }
                }
            }
        }

        Ok(())
    }
}

impl Sender for MeshtasticSender {
    fn check_ready(&self) -> Result<()> {
        let output = self
            .runner
            .output("meshtastic", &self.info_args())
            .map_err(|e| anyhow!("Failed to execute meshtastic --info: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        if stdout.contains("Error") {
            return Err(anyhow!("Received error output: {}", stdout));
        }

        if let Some(first_line) = stdout.lines().next() {
            if first_line == "Connected to radio" {
                log::info!("Successfully connected to the node.");
                Ok(())
            } else {
                Err(anyhow!(
                    "Failed to connect to the radio. First line: {}",
                    first_line
                ))
            }
        } else {
            Err(anyhow!("Output from meshtastic --info was empty."))
        }
    }

    fn send_alert(&mut self, alert: &Alert, channel: u32) -> Result<()> {
        for chunk in chunk_message(&alert.message_text, 75) {
            self.send_message_with_retry(channel, &chunk, 3, Duration::from_secs(5))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, PartialEq, Eq)]
    struct RecordedCommand {
        program: String,
        args: Vec<String>,
    }

    struct RecordingCommandRunner {
        commands: Rc<RefCell<Vec<RecordedCommand>>>,
        stdout: Vec<u8>,
    }

    impl CommandRunner for RecordingCommandRunner {
        fn output(&self, program: &str, args: &[String]) -> io::Result<CommandOutput> {
            self.commands.borrow_mut().push(RecordedCommand {
                program: program.to_string(),
                args: args.to_vec(),
            });

            Ok(CommandOutput {
                stdout: self.stdout.clone(),
            })
        }

        fn spawn(&self, program: &str, args: &[String]) -> io::Result<()> {
            self.commands.borrow_mut().push(RecordedCommand {
                program: program.to_string(),
                args: args.to_vec(),
            });

            Ok(())
        }
    }

    #[test]
    fn info_args_include_host_and_port_when_configured() {
        let sender = MeshtasticSender::new(MeshtasticConfig {
            host: Some("192.0.2.1:4403".to_string()),
            port: Some("/dev/ttyUSB0".to_string()),
        });

        assert_eq!(
            sender.info_args(),
            vec![
                "--host",
                "192.0.2.1:4403",
                "--port",
                "/dev/ttyUSB0",
                "--info"
            ]
        );
    }

    #[test]
    fn send_args_match_existing_meshtastic_cli_flags() {
        let sender = MeshtasticSender::new(MeshtasticConfig {
            host: None,
            port: None,
        });

        assert_eq!(
            sender.send_args(2, "alert text"),
            vec![
                "--no-nodes",
                "--no-time",
                "--ch-index",
                "2",
                "--sendtext",
                "alert text",
                "--ack"
            ]
        );
    }

    #[test]
    fn send_args_append_optional_host_and_port() {
        let sender = MeshtasticSender::new(MeshtasticConfig {
            host: Some("mesh.local:4403".to_string()),
            port: Some("/dev/ttyUSB0".to_string()),
        });

        assert_eq!(
            sender.send_args(1, "alert text"),
            vec![
                "--no-nodes",
                "--no-time",
                "--ch-index",
                "1",
                "--sendtext",
                "alert text",
                "--ack",
                "--host",
                "mesh.local:4403",
                "--port",
                "/dev/ttyUSB0"
            ]
        );
    }

    #[test]
    fn check_ready_runs_meshtastic_info_without_real_cli() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let sender = MeshtasticSender::with_runner(
            MeshtasticConfig {
                host: Some("mesh.local:4403".to_string()),
                port: None,
            },
            Box::new(RecordingCommandRunner {
                commands: Rc::clone(&commands),
                stdout: b"Connected to radio\n".to_vec(),
            }),
        );

        sender.check_ready().unwrap();

        assert_eq!(
            *commands.borrow(),
            vec![RecordedCommand {
                program: "meshtastic".to_string(),
                args: vec!["--host", "mesh.local:4403", "--info"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
            }]
        );
    }

    #[test]
    fn send_alert_runs_meshtastic_send_without_real_cli() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut sender = MeshtasticSender::with_runner(
            MeshtasticConfig {
                host: None,
                port: Some("/dev/ttyUSB0".to_string()),
            },
            Box::new(RecordingCommandRunner {
                commands: Rc::clone(&commands),
                stdout: Vec::new(),
            }),
        );
        let alert = Alert::new(
            "Tornado Warning".to_string(),
            crate::alert::AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            vec!["006085".to_string()],
            vec!["Central Santa Clara".to_string()],
            "short alert".to_string(),
        );

        sender.send_alert(&alert, 3).unwrap();

        assert_eq!(
            *commands.borrow(),
            vec![RecordedCommand {
                program: "meshtastic".to_string(),
                args: vec![
                    "--no-nodes",
                    "--no-time",
                    "--ch-index",
                    "3",
                    "--sendtext",
                    "short alert",
                    "--ack",
                    "--port",
                    "/dev/ttyUSB0",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            }]
        );
    }
}
