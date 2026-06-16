use crate::alert::Alert;
use crate::sender::Sender;
use anyhow::{anyhow, Result};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct CommandStatus {
    success: bool,
    code: Option<i32>,
}

trait LxmfCommandRunner {
    fn run_with_stdin(
        &self,
        program: &Path,
        args: &[String],
        stdin_text: &str,
    ) -> io::Result<CommandStatus>;
}

struct RealLxmfCommandRunner;

impl LxmfCommandRunner for RealLxmfCommandRunner {
    fn run_with_stdin(
        &self,
        program: &Path,
        args: &[String],
        stdin_text: &str,
    ) -> io::Result<CommandStatus> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(stdin_text.as_bytes())?;
        }

        let status = child.wait()?;

        Ok(CommandStatus {
            success: status.success(),
            code: status.code(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LxmfConfig {
    pub command: PathBuf,
    pub destination: String,
    pub config: Option<PathBuf>,
}

pub struct LxmfSender {
    config: LxmfConfig,
    runner: Box<dyn LxmfCommandRunner>,
}

impl LxmfSender {
    pub fn new(config: LxmfConfig) -> Self {
        Self::with_runner(config, Box::new(RealLxmfCommandRunner))
    }

    fn with_runner(config: LxmfConfig, runner: Box<dyn LxmfCommandRunner>) -> Self {
        Self { config, runner }
    }

    fn send_args(&self) -> Vec<String> {
        let mut args = vec!["--destination".to_string(), self.config.destination.clone()];

        if let Some(config) = &self.config.config {
            args.push("--config".to_string());
            args.push(config.to_string_lossy().to_string());
        }

        args
    }
}

impl Sender for LxmfSender {
    fn name(&self) -> &'static str {
        "lxmf"
    }

    fn check_ready(&self) -> Result<()> {
        Ok(())
    }

    fn send_alert(&mut self, alert: &Alert, _channel: u32) -> Result<()> {
        let status = self
            .runner
            .run_with_stdin(&self.config.command, &self.send_args(), &alert.message_text)
            .map_err(|e| anyhow!("LXMF helper execution failed: {}", e))?;

        if status.success {
            Ok(())
        } else if let Some(code) = status.code {
            Err(anyhow!("LXMF helper failed with exit code {}", code))
        } else {
            Err(anyhow!("LXMF helper failed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::{Alert, AlertSignificance};
    use crate::sender::FanOut;
    use crate::spool::{SpoolRecord, Spooler};
    use anyhow::Result;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, PartialEq, Eq)]
    struct RecordedLxmfCommand {
        program: PathBuf,
        args: Vec<String>,
        stdin_text: String,
    }

    struct RecordingLxmfCommandRunner {
        commands: Rc<RefCell<Vec<RecordedLxmfCommand>>>,
        status: CommandStatus,
    }

    struct RequiredSender {
        calls: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Sender for RequiredSender {
        fn name(&self) -> &'static str {
            "required"
        }

        fn check_ready(&self) -> Result<()> {
            Ok(())
        }

        fn send_alert(&mut self, _alert: &Alert, _channel: u32) -> Result<()> {
            self.calls.borrow_mut().push("required");
            Ok(())
        }
    }

    struct RecordingSpooler {
        records: Rc<RefCell<Vec<SpoolRecord>>>,
    }

    impl Spooler for RecordingSpooler {
        fn spool(&mut self, record: &SpoolRecord) -> Result<()> {
            self.records.borrow_mut().push(record.clone());
            Ok(())
        }
    }

    impl LxmfCommandRunner for RecordingLxmfCommandRunner {
        fn run_with_stdin(
            &self,
            program: &Path,
            args: &[String],
            stdin_text: &str,
        ) -> io::Result<CommandStatus> {
            self.commands.borrow_mut().push(RecordedLxmfCommand {
                program: program.to_path_buf(),
                args: args.to_vec(),
                stdin_text: stdin_text.to_string(),
            });

            Ok(CommandStatus {
                success: self.status.success,
                code: self.status.code,
            })
        }
    }

    fn test_alert(message_text: String) -> Alert {
        Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            Vec::new(),
            Vec::new(),
            message_text,
        )
    }

    #[test]
    fn send_args_include_destination_and_optional_config() {
        let sender = LxmfSender::new(LxmfConfig {
            command: PathBuf::from("lxmf-send"),
            destination: "dest-123".to_string(),
            config: Some(PathBuf::from("reticulum-config")),
        });

        assert_eq!(
            sender.send_args(),
            vec!["--destination", "dest-123", "--config", "reticulum-config"]
        );
    }

    #[test]
    fn alert_message_is_passed_to_helper_stdin() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut sender = LxmfSender::with_runner(
            LxmfConfig {
                command: PathBuf::from("lxmf-send"),
                destination: "dest-123".to_string(),
                config: None,
            },
            Box::new(RecordingLxmfCommandRunner {
                commands: Rc::clone(&commands),
                status: CommandStatus {
                    success: true,
                    code: Some(0),
                },
            }),
        );

        sender
            .send_alert(&test_alert("alert body\nsecond line".to_string()), 0)
            .unwrap();

        assert_eq!(
            *commands.borrow(),
            vec![RecordedLxmfCommand {
                program: PathBuf::from("lxmf-send"),
                args: vec!["--destination", "dest-123"]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                stdin_text: "alert body\nsecond line".to_string(),
            }]
        );
    }

    #[test]
    fn non_zero_helper_exit_is_a_send_failure() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut sender = LxmfSender::with_runner(
            LxmfConfig {
                command: PathBuf::from("lxmf-send"),
                destination: "dest-123".to_string(),
                config: None,
            },
            Box::new(RecordingLxmfCommandRunner {
                commands: Rc::clone(&commands),
                status: CommandStatus {
                    success: false,
                    code: Some(2),
                },
            }),
        );

        let error = sender
            .send_alert(&test_alert("alert body".to_string()), 0)
            .unwrap_err()
            .to_string();

        assert_eq!(error, "LXMF helper failed with exit code 2");
        assert_eq!(commands.borrow().len(), 1);
    }

    #[test]
    fn lxmf_failure_does_not_block_required_sender() {
        let required_calls = Rc::new(RefCell::new(Vec::new()));
        let lxmf_commands = Rc::new(RefCell::new(Vec::new()));
        let lxmf_sender = LxmfSender::with_runner(
            LxmfConfig {
                command: PathBuf::from("lxmf-send"),
                destination: "dest-123".to_string(),
                config: None,
            },
            Box::new(RecordingLxmfCommandRunner {
                commands: Rc::clone(&lxmf_commands),
                status: CommandStatus {
                    success: false,
                    code: Some(1),
                },
            }),
        );
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RequiredSender {
                calls: Rc::clone(&required_calls),
            })],
            vec![Box::new(lxmf_sender)],
        );

        assert!(fanout
            .send_alert(&test_alert("alert body".to_string()), 0)
            .is_ok());
        assert_eq!(*required_calls.borrow(), vec!["required"]);
        assert_eq!(lxmf_commands.borrow().len(), 1);
    }

    #[test]
    fn lxmf_failure_is_spooled_when_spooler_is_configured() {
        let required_calls = Rc::new(RefCell::new(Vec::new()));
        let lxmf_commands = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let lxmf_sender = LxmfSender::with_runner(
            LxmfConfig {
                command: PathBuf::from("lxmf-send"),
                destination: "dest-123".to_string(),
                config: None,
            },
            Box::new(RecordingLxmfCommandRunner {
                commands: Rc::clone(&lxmf_commands),
                status: CommandStatus {
                    success: false,
                    code: Some(1),
                },
            }),
        );
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RequiredSender {
                calls: Rc::clone(&required_calls),
            })],
            vec![Box::new(lxmf_sender)],
        )
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
        }));

        assert!(fanout
            .send_alert(&test_alert("alert body".to_string()), 3)
            .is_ok());

        let records = spooled.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sender, "lxmf");
        assert_eq!(records[0].channel, 3);
        assert_eq!(records[0].message_text, "alert body");
        assert_eq!(records[0].error, "LXMF helper failed with exit code 1");
    }
}
