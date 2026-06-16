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

trait MeshCoreCommandRunner {
    fn run_with_stdin(
        &self,
        program: &Path,
        args: &[String],
        stdin_text: &str,
    ) -> io::Result<CommandStatus>;
}

struct RealMeshCoreCommandRunner;

impl MeshCoreCommandRunner for RealMeshCoreCommandRunner {
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
pub struct MeshCoreConfig {
    pub command: PathBuf,
    pub destination: String,
    pub config: Option<PathBuf>,
}

pub struct MeshCoreSender {
    config: MeshCoreConfig,
    runner: Box<dyn MeshCoreCommandRunner>,
}

impl MeshCoreSender {
    pub fn new(config: MeshCoreConfig) -> Self {
        Self::with_runner(config, Box::new(RealMeshCoreCommandRunner))
    }

    fn with_runner(config: MeshCoreConfig, runner: Box<dyn MeshCoreCommandRunner>) -> Self {
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

impl Sender for MeshCoreSender {
    fn name(&self) -> &'static str {
        "meshcore"
    }

    fn check_ready(&self) -> Result<()> {
        Ok(())
    }

    fn send_alert(&mut self, alert: &Alert, _channel: u32) -> Result<()> {
        let status = self
            .runner
            .run_with_stdin(&self.config.command, &self.send_args(), &alert.message_text)
            .map_err(|e| anyhow!("MeshCore helper execution failed: {}", e))?;

        if status.success {
            Ok(())
        } else if let Some(code) = status.code {
            Err(anyhow!("MeshCore helper failed with exit code {}", code))
        } else {
            Err(anyhow!("MeshCore helper failed"))
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
    struct RecordedMeshCoreCommand {
        program: PathBuf,
        args: Vec<String>,
        stdin_text: String,
    }

    struct RecordingMeshCoreCommandRunner {
        commands: Rc<RefCell<Vec<RecordedMeshCoreCommand>>>,
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

    impl MeshCoreCommandRunner for RecordingMeshCoreCommandRunner {
        fn run_with_stdin(
            &self,
            program: &Path,
            args: &[String],
            stdin_text: &str,
        ) -> io::Result<CommandStatus> {
            self.commands.borrow_mut().push(RecordedMeshCoreCommand {
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
        let sender = MeshCoreSender::new(MeshCoreConfig {
            command: PathBuf::from("meshcore-send"),
            destination: "room-123".to_string(),
            config: Some(PathBuf::from("meshcore-config")),
        });

        assert_eq!(
            sender.send_args(),
            vec!["--destination", "room-123", "--config", "meshcore-config"]
        );
    }

    #[test]
    fn alert_message_is_passed_to_helper_stdin() {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let mut sender = MeshCoreSender::with_runner(
            MeshCoreConfig {
                command: PathBuf::from("meshcore-send"),
                destination: "room-123".to_string(),
                config: None,
            },
            Box::new(RecordingMeshCoreCommandRunner {
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
            vec![RecordedMeshCoreCommand {
                program: PathBuf::from("meshcore-send"),
                args: vec!["--destination", "room-123"]
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
        let mut sender = MeshCoreSender::with_runner(
            MeshCoreConfig {
                command: PathBuf::from("meshcore-send"),
                destination: "room-123".to_string(),
                config: None,
            },
            Box::new(RecordingMeshCoreCommandRunner {
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

        assert_eq!(error, "MeshCore helper failed with exit code 2");
        assert_eq!(commands.borrow().len(), 1);
    }

    #[test]
    fn meshcore_failure_does_not_block_required_sender() {
        let required_calls = Rc::new(RefCell::new(Vec::new()));
        let meshcore_commands = Rc::new(RefCell::new(Vec::new()));
        let meshcore_sender = MeshCoreSender::with_runner(
            MeshCoreConfig {
                command: PathBuf::from("meshcore-send"),
                destination: "room-123".to_string(),
                config: None,
            },
            Box::new(RecordingMeshCoreCommandRunner {
                commands: Rc::clone(&meshcore_commands),
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
            vec![Box::new(meshcore_sender)],
        );

        assert!(fanout
            .send_alert(&test_alert("alert body".to_string()), 0)
            .is_ok());
        assert_eq!(*required_calls.borrow(), vec!["required"]);
        assert_eq!(meshcore_commands.borrow().len(), 1);
    }

    #[test]
    fn meshcore_failure_is_spooled_when_spooler_is_configured() {
        let required_calls = Rc::new(RefCell::new(Vec::new()));
        let meshcore_commands = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let meshcore_sender = MeshCoreSender::with_runner(
            MeshCoreConfig {
                command: PathBuf::from("meshcore-send"),
                destination: "room-123".to_string(),
                config: None,
            },
            Box::new(RecordingMeshCoreCommandRunner {
                commands: Rc::clone(&meshcore_commands),
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
            vec![Box::new(meshcore_sender)],
        )
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
        }));

        assert!(fanout
            .send_alert(&test_alert("alert body".to_string()), 3)
            .is_ok());

        let records = spooled.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sender, "meshcore");
        assert_eq!(records[0].channel, 3);
        assert_eq!(records[0].message_text, "alert body");
        assert_eq!(records[0].error, "MeshCore helper failed with exit code 1");
    }
}
