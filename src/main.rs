mod alert;
mod dashboard;
mod dashboard_view;
#[allow(dead_code)]
mod dedup;
mod discord_sender;
pub mod event_contracts;
mod event_emitter;
mod event_log_reader;
mod lxmf_sender;
mod meshcore_sender;
mod meshtastic_sender;
#[allow(dead_code)]
mod normalized_alert;
#[allow(dead_code)]
mod nws_client;
mod nws_polling;
mod replay;
mod sender;
mod source_health;
mod spool;

use alert::{
    alert_send_channel, format_alert_message, location_filter_allows, Alert, AlertMessageParts,
    AlertSignificance,
};
use anyhow::Result;
use byteorder::{NativeEndian, ReadBytesExt};
use clap::Parser;
use csv::ReaderBuilder;
use dashboard::{run_dashboard, DEFAULT_DASHBOARD_BIND};
use discord_sender::DiscordSender;
use event_contracts::{AlertRecord, EVENT_CONTRACT_SCHEMA_VERSION};
use event_emitter::{warn_event_write_failure, EventEmitter, FileEventEmitter};
use log::LevelFilter;
use lxmf_sender::{LxmfConfig, LxmfSender};
use meshcore_sender::{MeshCoreConfig, MeshCoreSender};
use meshtastic_sender::{MeshtasticConfig, MeshtasticSender};
use nws_client::{NwsClient, NwsClientConfig, UreqNwsHttpClient};
use nws_polling::{
    run_nws_polling_loop, FanOutNwsAlertDelivery, NwsPollingConfig, DEFAULT_NWS_POLL_SECONDS,
};
use replay::replay_spool_file;
use rust_embed::RustEmbed;
use sameold::{Message, SameReceiverBuilder, SignificanceLevel};
use sender::{FanOut, Sender};
use serde::Deserialize;
use simple_logger::SimpleLogger;
use source_health::{source_status_record, SourceHealthInput};
use spool::FileSpooler;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{self};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use strum::EnumMessage;

#[derive(RustEmbed)]
#[folder = "src"]
struct Asset;

#[derive(Debug, Deserialize)]
struct Record {
    code: String,
    county: String,
    state: String,
}

fn load_csv_into_hashmap() -> HashMap<String, (String, String)> {
    let mut map = HashMap::new();

    let csv_data = Asset::get("sameCodes.csv").unwrap();
    let csv_str = std::str::from_utf8(csv_data.data.as_ref()).unwrap();

    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .from_reader(csv_str.as_bytes());

    for result in rdr.deserialize() {
        let record: Record = result.unwrap();
        map.insert(record.code, (record.county, record.state));
    }

    map
}

fn search_by_code<'a>(
    map: &'a HashMap<String, (String, String)>,
    code: &str,
) -> Option<&'a (String, String)> {
    map.get(code)
}

fn alert_significance(significance: SignificanceLevel) -> AlertSignificance {
    match significance {
        SignificanceLevel::Test => AlertSignificance::Test,
        SignificanceLevel::Statement => AlertSignificance::Statement,
        SignificanceLevel::Emergency => AlertSignificance::Emergency,
        SignificanceLevel::Watch => AlertSignificance::Watch,
        SignificanceLevel::Warning => AlertSignificance::Warning,
        SignificanceLevel::Unknown => AlertSignificance::Unknown,
    }
}

fn location_name(map: &HashMap<String, (String, String)>, code: &str) -> Option<String> {
    search_by_code(map, &format!("0{}", &code[1..])).map(|(county, _state)| {
        let mut location = String::new();

        match code.chars().next().unwrap_or_default() {
            '0' => {}
            '1' => location.push_str("Northwest "),
            '2' => location.push_str("North "),
            '3' => location.push_str("Northeast "),
            '4' => location.push_str("West "),
            '5' => location.push_str("Central "),
            '6' => location.push_str("East "),
            '7' => location.push_str("Southwest "),
            '8' => location.push_str("South "),
            '9' => location.push_str("Southeast "),
            _ => {}
        }

        location.push_str(county);
        location
    })
}

#[derive(Parser, Debug, Clone)]
#[command(long_about = None)]
struct Args {
    /// Channel to which alerts are sent to, if not provided will default to channel 0
    #[arg(long, short)]
    alert_channel: Option<u32>,

    /// Channel to which tests are sent to, if not provided tests will be ignored
    #[arg(long, short)]
    test_channel: Option<u32>,

    /// Network address with port of device to connect to in the form of target.address:port
    #[arg(long)]
    host: Option<String>,

    /// The port of the device to connect to using serial, e.g. /dev/ttyUSB0. (defaults to trying to detect a port)
    #[arg(long)]
    port: Option<String>,

    /// Sample rate.
    #[arg(long, short, default_value_t = 48000)]
    rate: u32,

    /// Location codes that must be present to send an alert
    #[arg(long, short, value_delimiter = ',', default_value = None, required = false)]
    locations: Vec<String>,

    /// Optional Discord webhook URL for best-effort alert delivery
    #[arg(long)]
    discord_webhook_url: Option<String>,

    /// Optional JSONL path for spooling best-effort sender failures
    #[arg(long)]
    spool_path: Option<PathBuf>,

    /// Optional LXMF send helper command for best-effort alert delivery
    #[arg(long)]
    lxmf_command: Option<PathBuf>,

    /// Optional LXMF destination for best-effort alert delivery
    #[arg(long)]
    lxmf_destination: Option<String>,

    /// Optional LXMF helper configuration path
    #[arg(long)]
    lxmf_config: Option<PathBuf>,

    /// Optional MeshCore send helper command for best-effort alert delivery
    #[arg(long)]
    meshcore_command: Option<PathBuf>,

    /// Optional MeshCore destination for best-effort alert delivery
    #[arg(long)]
    meshcore_destination: Option<String>,

    /// Optional MeshCore helper configuration path
    #[arg(long)]
    meshcore_config: Option<PathBuf>,

    /// Run one-shot replay for best-effort sender failures from this spool path, then exit
    #[arg(long)]
    replay_spool: Option<PathBuf>,

    /// Optional path for records that fail during one-shot replay
    #[arg(long)]
    replay_failed_output: Option<PathBuf>,

    /// Optional JSONL path for local dashboard event records
    #[arg(long)]
    event_log_path: Option<PathBuf>,

    /// Run the read-only local dashboard against this event log path
    #[arg(long)]
    dashboard_event_log: Option<PathBuf>,

    /// Address and port for the read-only local dashboard
    #[arg(long, default_value = DEFAULT_DASHBOARD_BIND)]
    dashboard_bind: String,

    /// Run opt-in NOAA/NWS API polling mode without alert delivery
    #[arg(long)]
    nws_api: bool,

    /// Required User-Agent for NOAA/NWS API requests when --nws-api is enabled
    #[arg(long)]
    nws_user_agent: Option<String>,

    /// Optional NOAA/NWS API state/territory area filter
    #[arg(long)]
    nws_area: Option<String>,

    /// Optional NOAA/NWS API zone filter
    #[arg(long)]
    nws_zone: Option<String>,

    /// NOAA/NWS API poll interval in seconds
    #[arg(long, default_value_t = DEFAULT_NWS_POLL_SECONDS)]
    nws_poll_seconds: u64,
}

fn build_best_effort_senders(args: &Args) -> Result<Vec<Box<dyn Sender>>> {
    let mut best_effort_senders: Vec<Box<dyn Sender>> = Vec::new();

    if let Some(webhook_url) = &args.discord_webhook_url {
        best_effort_senders.push(Box::new(DiscordSender::new(webhook_url.clone())));
    }

    match (&args.lxmf_command, &args.lxmf_destination) {
        (Some(command), Some(destination)) => {
            best_effort_senders.push(Box::new(LxmfSender::new(LxmfConfig {
                command: command.clone(),
                destination: destination.clone(),
                config: args.lxmf_config.clone(),
            })));
        }
        (None, None) => {
            if args.lxmf_config.is_some() {
                return Err(anyhow::anyhow!(
                    "--lxmf-config requires --lxmf-command and --lxmf-destination"
                ));
            }
        }
        (Some(_), None) => {
            return Err(anyhow::anyhow!(
                "--lxmf-command requires --lxmf-destination"
            ));
        }
        (None, Some(_)) => {
            return Err(anyhow::anyhow!(
                "--lxmf-destination requires --lxmf-command"
            ));
        }
    }

    match (&args.meshcore_command, &args.meshcore_destination) {
        (Some(command), Some(destination)) => {
            best_effort_senders.push(Box::new(MeshCoreSender::new(MeshCoreConfig {
                command: command.clone(),
                destination: destination.clone(),
                config: args.meshcore_config.clone(),
            })));
        }
        (None, None) => {
            if args.meshcore_config.is_some() {
                return Err(anyhow::anyhow!(
                    "--meshcore-config requires --meshcore-command and --meshcore-destination"
                ));
            }
        }
        (Some(_), None) => {
            return Err(anyhow::anyhow!(
                "--meshcore-command requires --meshcore-destination"
            ));
        }
        (None, Some(_)) => {
            return Err(anyhow::anyhow!(
                "--meshcore-destination requires --meshcore-command"
            ));
        }
    }

    Ok(best_effort_senders)
}

fn build_fanout(args: &Args) -> Result<FanOut> {
    let required_senders: Vec<Box<dyn Sender>> =
        vec![Box::new(MeshtasticSender::new(MeshtasticConfig {
            host: args.host.clone(),
            port: args.port.clone(),
        }))];
    let best_effort_senders = build_best_effort_senders(args)?;

    let mut fanout = if best_effort_senders.is_empty() {
        FanOut::new(required_senders)
    } else {
        FanOut::with_best_effort(required_senders, best_effort_senders)
    };

    if let Some(spool_path) = &args.spool_path {
        fanout = fanout.with_spooler(Box::new(FileSpooler::new(spool_path.clone())));
    }

    Ok(fanout)
}

fn build_event_emitter(args: &Args) -> Option<Box<dyn EventEmitter>> {
    args.event_log_path
        .as_ref()
        .map(|path| Box::new(FileEventEmitter::new(path.clone())) as Box<dyn EventEmitter>)
}

fn replay_mode_enabled(args: &Args) -> bool {
    args.replay_spool.is_some()
}

fn dashboard_mode_enabled(args: &Args) -> bool {
    args.dashboard_event_log.is_some()
}

fn nws_mode_enabled(args: &Args) -> bool {
    args.nws_api
}

fn build_nws_client_config(args: &Args) -> Result<NwsClientConfig> {
    if !args.nws_api {
        return Err(anyhow::anyhow!(
            "--nws-api is required for NOAA polling mode"
        ));
    }

    let user_agent = args
        .nws_user_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("--nws-user-agent is required when --nws-api is enabled"))?;

    let config = NwsClientConfig {
        user_agent: user_agent.to_string(),
        area: args.nws_area.clone(),
        zone: args.nws_zone.clone(),
        ..NwsClientConfig::default()
    };
    config.validate()?;

    Ok(config)
}

fn build_nws_polling_config(args: &Args) -> Result<NwsPollingConfig> {
    NwsPollingConfig::new(args.nws_poll_seconds)
}

fn run_nws_mode(args: &Args) -> Result<()> {
    let client_config = build_nws_client_config(args)?;
    let polling_config = build_nws_polling_config(args)?;
    let client = NwsClient::new(client_config, UreqNwsHttpClient)?;
    let mut fanout = build_fanout(args)?;
    let mut event_emitter = build_event_emitter(args);
    let channel = args.alert_channel.unwrap_or(0);
    if !(0..=7).contains(&channel) {
        return Err(anyhow::anyhow!("alertChannel must be between 0 and 7"));
    }

    let readiness_result = if let Some(event_emitter) = event_emitter.as_deref_mut() {
        fanout.check_ready_with_events(event_emitter)
    } else {
        fanout.check_ready()
    };
    readiness_result?;

    let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, channel, event_emitter);
    run_nws_polling_loop(&client, &polling_config, &mut delivery)
}

fn run_dashboard_mode(args: &Args) -> Result<()> {
    let Some(event_log_path) = &args.dashboard_event_log else {
        return Ok(());
    };

    run_dashboard(event_log_path.clone(), &args.dashboard_bind)
}

fn run_replay_mode(args: &Args) -> Result<()> {
    let Some(replay_spool) = &args.replay_spool else {
        return Ok(());
    };

    let summary = replay_spool_file(
        replay_spool,
        build_best_effort_senders(args)?,
        args.replay_failed_output.as_deref(),
    )?;

    log::info!(
        "Replay complete: parsed={}, replayed={}, failed={}, malformed={}, skipped_meshtastic={}, skipped_unconfigured={}, skipped_unknown={}",
        summary.parsed_records,
        summary.replayed_records,
        summary.failed_records,
        summary.malformed_lines,
        summary.skipped_meshtastic_records,
        summary.skipped_unconfigured_records,
        summary.skipped_unknown_sender_records
    );

    Ok(())
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn alert_id_for(alert: &Alert, channel: u32, timestamp_unix_secs: u64) -> String {
    let mut hasher = DefaultHasher::new();
    alert.event.hash(&mut hasher);
    alert.originator.hash(&mut hasher);
    alert.callsign.hash(&mut hasher);
    alert.is_national.hash(&mut hasher);
    alert.is_test.hash(&mut hasher);
    alert.location_codes.hash(&mut hasher);
    alert.message_text.hash(&mut hasher);
    channel.hash(&mut hasher);
    format!(
        "same-{}-{}-{:016x}",
        timestamp_unix_secs,
        channel,
        hasher.finish()
    )
}

fn alert_record_from_alert_at(
    alert: &Alert,
    channel: u32,
    timestamp_unix_secs: u64,
) -> AlertRecord {
    AlertRecord {
        schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
        record_type: "alert".to_string(),
        alert_id: alert_id_for(alert, channel, timestamp_unix_secs),
        timestamp_unix_secs,
        source: "same".to_string(),
        event: alert.event.clone(),
        significance: alert.significance,
        originator: alert.originator.clone(),
        callsign: alert.callsign.clone(),
        is_national: alert.is_national,
        is_test: alert.is_test,
        location_codes: alert.location_codes.clone(),
        location_names: alert.location_names.clone(),
        message_text: alert.message_text.clone(),
    }
}

fn emit_alert_record(event_emitter: &mut dyn EventEmitter, record: &AlertRecord) {
    if let Err(e) = event_emitter.emit_alert(record) {
        warn_event_write_failure(e);
    }
}

fn emit_same_source_status(
    event_emitter: &mut dyn EventEmitter,
    timestamp_unix_secs: u64,
    last_decoded_message_unix_secs: Option<u64>,
    last_accepted_alert_unix_secs: Option<u64>,
) {
    let record = source_status_record(SourceHealthInput::same_radio(
        timestamp_unix_secs,
        last_decoded_message_unix_secs,
        last_accepted_alert_unix_secs,
    ));
    if let Err(e) = event_emitter.emit_source_status(&record) {
        warn_event_write_failure(e);
    }
}

fn main() -> Result<()> {
    // Initialize logging
    SimpleLogger::new()
        .with_level(LevelFilter::Off)
        .with_module_level("Meshtastic_SAME_EAS_Alerter", LevelFilter::Info)
        .init()
        .unwrap();

    // Default channel for alerts
    let mut alert_channel: u32 = 0;

    // Default value of test_channel
    // 10 means that tests will not be logged
    let mut test_channel: u32 = 10;

    // Parse the command line arguments
    let args = Args::parse();

    if dashboard_mode_enabled(&args) {
        return run_dashboard_mode(&args);
    }

    if nws_mode_enabled(&args) {
        return run_nws_mode(&args);
    }

    if replay_mode_enabled(&args) {
        return run_replay_mode(&args);
    }

    // Handle alertChannel argument
    if let Some(alert_channel_arg) = args.alert_channel {
        if !(0..=7).contains(&alert_channel_arg) {
            // https://meshtastic.org/docs/configuration/radio/channels/
            return Err(anyhow::anyhow!("alertChannel must be between 0 and 7"));
        } else {
            alert_channel = alert_channel_arg;
        }
    }

    // Handle testChannel argument
    if let Some(test_channel_arg) = args.test_channel {
        if !(0..=7).contains(&test_channel_arg) {
            // https://meshtastic.org/docs/configuration/radio/channels/
            return Err(anyhow::anyhow!("testChannel must be between 0 and 7"));
        } else {
            test_channel = test_channel_arg;
        }
    }

    let mut fanout = build_fanout(&args)?;
    let mut event_emitter = build_event_emitter(&args);

    let readiness_result = if let Some(event_emitter) = event_emitter.as_deref_mut() {
        fanout.check_ready_with_events(event_emitter)
    } else {
        fanout.check_ready()
    };

    if let Err(e) = readiness_result {
        log::error!("{}", e);
        std::process::exit(1);
    }

    // Create a SameReceiver.
    let mut rx = SameReceiverBuilder::new(args.rate)
        .with_agc_gain_limits(1.0f32 / (i16::MAX as f32), 1.0 / 200.0)
        .with_agc_bandwidth(0.05) // AGC bandwidth at symbol rate, < 1.0
        .with_squelch_power(0.10, 0.05) // squelch open/close power, 0.0 < power < 1.0
        .with_preamble_max_errors(2) // bit error limit when detecting sync sequence
        .build();

    // Set up stdin as the input source
    let stdin = io::stdin();
    // Check if there is any input from stdin
    if atty::is(atty::Stream::Stdin) {
        log::error!("Error: No input provided to stdin. Please provide RTL FM input.");
        std::process::exit(1);
    }

    let map = load_csv_into_hashmap();
    log::info!("Loaded locations CSV");

    let stdin_handle = stdin.lock();
    let mut inbuf = Box::new(io::BufReader::new(stdin_handle));

    // Create an iterator for audio source from stdin, reading i16 and converting to f32
    let audiosrc = std::iter::from_fn(|| inbuf.read_i16::<NativeEndian>().ok());

    log::info!("Monitoring for alerts");
    log::info!("Alerts will be sent to channel: {}", alert_channel);
    if test_channel == 10 {
        log::info!("Tests alerts will be ignored (test-channel argument was not provided)")
    } else {
        log::info!("Test alerts will be sent to channel: {}", test_channel)
    }

    let mut last_same_decoded_message_unix_secs;
    let mut last_same_accepted_alert_unix_secs = None;
    if let Some(event_emitter) = event_emitter.as_deref_mut() {
        emit_same_source_status(event_emitter, unix_timestamp_secs(), None, None);
    }

    // Process messages from the audio source
    for msg in rx.iter_messages(audiosrc.map(|sa| sa as f32)) {
        match msg {
            Message::StartOfMessage(hdr) => {
                let decoded_timestamp = unix_timestamp_secs();
                last_same_decoded_message_unix_secs = Some(decoded_timestamp);
                if let Some(event_emitter) = event_emitter.as_deref_mut() {
                    emit_same_source_status(
                        event_emitter,
                        decoded_timestamp,
                        last_same_decoded_message_unix_secs,
                        last_same_accepted_alert_unix_secs,
                    );
                }

                let evt = hdr.event();
                let significance = alert_significance(evt.significance());
                log::info!("Begin SAME voice message: {:?}", hdr);
                let codes: Vec<String> = hdr.location_str_iter().map(|s| s.to_string()).collect();
                let mut locations_found = Vec::new();

                let Some(send_channel) =
                    alert_send_channel(significance, alert_channel, test_channel)
                else {
                    log::info!("Ignoring test alert");
                    continue;
                };

                if !hdr.is_national() {
                    if !args.locations.is_empty() && !codes.is_empty() {
                        // Log the values for debugging
                        log::debug!("Provided locations: {:?}", args.locations);
                        log::debug!("Alert locations: {:?}", codes);

                        for code in &codes {
                            let matches = args.locations.contains(code);
                            log::debug!(
                                "Comparing alert code '{}' with provided locations: {}",
                                code,
                                matches
                            );
                        }

                        if !location_filter_allows(hdr.is_national(), &args.locations, &codes) {
                            log::info!("Ignoring alert with no matching locations in filter");
                            continue;
                        } else {
                            log::info!("Alert has matching locations, proceeding to send");
                        }
                    } else {
                        log::info!(
                            "No location filter applied (locations empty) or no locations in alert"
                        );
                    }

                    // Pass each code into the function and collect the results
                    for code in &codes {
                        if let Some(location) = location_name(&map, code) {
                            locations_found.push(location);
                        } else {
                            log::debug!("Location Code: {} not found", code);
                        }
                    }
                }

                let message = format_alert_message(AlertMessageParts {
                    event: &evt.to_string(),
                    significance,
                    originator: hdr.originator().get_detailed_message().unwrap(),
                    callsign: hdr.callsign(),
                    is_national: hdr.is_national(),
                    location_names: &locations_found,
                });

                log::info!("Attempting to send message over the mesh: {}", message);

                let alert = Alert::new(
                    evt.to_string(),
                    significance,
                    hdr.originator().get_detailed_message().unwrap().to_string(),
                    hdr.callsign().to_string(),
                    hdr.is_national(),
                    codes,
                    locations_found,
                    message,
                );

                let alert_record =
                    alert_record_from_alert_at(&alert, send_channel, unix_timestamp_secs());
                let alert_id = alert_record.alert_id.clone();
                last_same_accepted_alert_unix_secs = Some(alert_record.timestamp_unix_secs);
                if let Some(event_emitter) = event_emitter.as_deref_mut() {
                    emit_same_source_status(
                        event_emitter,
                        alert_record.timestamp_unix_secs,
                        last_same_decoded_message_unix_secs,
                        last_same_accepted_alert_unix_secs,
                    );
                    emit_alert_record(event_emitter, &alert_record);
                    fanout
                        .send_alert_with_events(&alert, send_channel, &alert_id, event_emitter)
                        .expect("Failed sending msg");
                } else {
                    fanout
                        .send_alert(&alert, send_channel)
                        .expect("Failed sending msg");
                }
            }
            Message::EndOfMessage => {
                log::info!("End SAME voice message");
            }
        }
    }

    log::warn!("Program stopped, no longer monitoring");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use event_contracts::JsonLineRecord;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct RecordingEventEmitter {
        alerts: Rc<RefCell<Vec<AlertRecord>>>,
        error: Option<&'static str>,
    }

    impl RecordingEventEmitter {
        fn new(alerts: Rc<RefCell<Vec<AlertRecord>>>) -> Self {
            Self {
                alerts,
                error: None,
            }
        }

        fn with_error(mut self, error: &'static str) -> Self {
            self.error = Some(error);
            self
        }
    }

    impl EventEmitter for RecordingEventEmitter {
        fn emit_alert(&mut self, record: &AlertRecord) -> Result<()> {
            self.alerts.borrow_mut().push(record.clone());

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }

        fn emit_delivery_attempt(
            &mut self,
            _record: &event_contracts::DeliveryAttemptRecord,
        ) -> Result<()> {
            Ok(())
        }

        fn emit_sender_status(
            &mut self,
            _record: &event_contracts::SenderStatusRecord,
        ) -> Result<()> {
            Ok(())
        }

        fn emit_source_status(
            &mut self,
            _record: &event_contracts::SourceStatusRecord,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fanout_registers_only_meshtastic_without_discord_webhook() {
        let args = Args::try_parse_from(["alerter"]).unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 0);
    }

    #[test]
    fn fanout_registers_discord_as_best_effort_when_webhook_is_provided() {
        let args = Args::try_parse_from([
            "alerter",
            "--discord-webhook-url",
            "https://discord.example/webhook",
        ])
        .unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 1);
    }

    #[test]
    fn replay_mode_is_disabled_without_replay_spool() {
        let args = Args::try_parse_from(["alerter"]).unwrap();

        assert!(!replay_mode_enabled(&args));
    }

    #[test]
    fn replay_mode_is_enabled_with_replay_spool() {
        let args = Args::try_parse_from(["alerter", "--replay-spool", "failures.jsonl"]).unwrap();

        assert!(replay_mode_enabled(&args));
    }

    #[test]
    fn dashboard_mode_is_disabled_without_dashboard_event_log() {
        let args = Args::try_parse_from(["alerter"]).unwrap();

        assert!(!dashboard_mode_enabled(&args));
        assert_eq!(args.dashboard_bind, DEFAULT_DASHBOARD_BIND);
    }

    #[test]
    fn dashboard_mode_is_enabled_with_dashboard_event_log() {
        let args =
            Args::try_parse_from(["alerter", "--dashboard-event-log", "events.jsonl"]).unwrap();

        assert!(dashboard_mode_enabled(&args));
        assert_eq!(args.dashboard_bind, DEFAULT_DASHBOARD_BIND);
    }

    #[test]
    fn dashboard_bind_can_be_overridden() {
        let args = Args::try_parse_from([
            "alerter",
            "--dashboard-event-log",
            "events.jsonl",
            "--dashboard-bind",
            "127.0.0.1:9090",
        ])
        .unwrap();

        assert!(dashboard_mode_enabled(&args));
        assert_eq!(args.dashboard_bind, "127.0.0.1:9090");
    }

    #[test]
    fn nws_mode_is_disabled_without_nws_api_flag() {
        let args = Args::try_parse_from(["alerter"]).unwrap();

        assert!(!nws_mode_enabled(&args));
    }

    #[test]
    fn nws_mode_is_enabled_with_nws_api_flag() {
        let args = Args::try_parse_from([
            "alerter",
            "--nws-api",
            "--nws-user-agent",
            "Project-Sentinel test@example.com",
        ])
        .unwrap();

        assert!(nws_mode_enabled(&args));
    }

    #[test]
    fn nws_user_agent_is_required_when_nws_api_is_enabled() {
        let args = Args::try_parse_from(["alerter", "--nws-api"]).unwrap();

        let error = build_nws_client_config(&args).unwrap_err();

        assert_eq!(
            error.to_string(),
            "--nws-user-agent is required when --nws-api is enabled"
        );
    }

    #[test]
    fn nws_area_and_zone_are_mutually_exclusive() {
        let args = Args::try_parse_from([
            "alerter",
            "--nws-api",
            "--nws-user-agent",
            "Project-Sentinel test@example.com",
            "--nws-area",
            "TX",
            "--nws-zone",
            "TXC201",
        ])
        .unwrap();

        let error = build_nws_client_config(&args).unwrap_err();

        assert_eq!(
            error.to_string(),
            "configure either NWS area or NWS zone, not both"
        );
    }

    #[test]
    fn nws_client_config_uses_area_filter() {
        let args = Args::try_parse_from([
            "alerter",
            "--nws-api",
            "--nws-user-agent",
            "Project-Sentinel test@example.com",
            "--nws-area",
            "TX",
        ])
        .unwrap();

        let config = build_nws_client_config(&args).unwrap();

        assert_eq!(config.user_agent, "Project-Sentinel test@example.com");
        assert_eq!(config.area.as_deref(), Some("TX"));
        assert_eq!(config.zone, None);
    }

    #[test]
    fn nws_client_config_uses_zone_filter() {
        let args = Args::try_parse_from([
            "alerter",
            "--nws-api",
            "--nws-user-agent",
            "Project-Sentinel test@example.com",
            "--nws-zone",
            "TXC201",
        ])
        .unwrap();

        let config = build_nws_client_config(&args).unwrap();

        assert_eq!(config.user_agent, "Project-Sentinel test@example.com");
        assert_eq!(config.area, None);
        assert_eq!(config.zone.as_deref(), Some("TXC201"));
    }

    #[test]
    fn nws_polling_config_defaults_to_sixty_seconds() {
        let args = Args::try_parse_from(["alerter"]).unwrap();

        let config = build_nws_polling_config(&args).unwrap();

        assert_eq!(config.poll_seconds, 60);
    }

    #[test]
    fn nws_polling_config_can_be_overridden() {
        let args = Args::try_parse_from(["alerter", "--nws-poll-seconds", "30"]).unwrap();

        let config = build_nws_polling_config(&args).unwrap();

        assert_eq!(config.poll_seconds, 30);
    }

    #[test]
    fn nws_polling_config_rejects_zero_seconds() {
        let args = Args::try_parse_from(["alerter", "--nws-poll-seconds", "0"]).unwrap();

        let error = build_nws_polling_config(&args).unwrap_err();

        assert_eq!(
            error.to_string(),
            "--nws-poll-seconds must be greater than 0"
        );
    }

    #[test]
    fn event_emitter_is_not_configured_without_event_log_path() {
        let args = Args::try_parse_from(["alerter"]).unwrap();

        assert!(build_event_emitter(&args).is_none());
    }

    #[test]
    fn event_log_path_configures_event_emitter() {
        let args = Args::try_parse_from(["alerter", "--event-log-path", "events.jsonl"]).unwrap();

        assert!(build_event_emitter(&args).is_some());
    }

    #[test]
    fn alert_record_is_built_for_accepted_alerts() {
        let alert = Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            vec!["006085".to_string()],
            vec!["Central Santa Clara".to_string()],
            "test alert".to_string(),
        );

        let record = alert_record_from_alert_at(&alert, 2, 123);

        assert_eq!(record.schema_version, EVENT_CONTRACT_SCHEMA_VERSION);
        assert_eq!(record.record_type, "alert");
        assert_eq!(record.alert_id, alert_id_for(&alert, 2, 123));
        assert_eq!(record.timestamp_unix_secs, 123);
        assert_eq!(record.source, "same");
        assert_eq!(record.event, "Tornado Warning");
        assert_eq!(record.significance, AlertSignificance::Warning);
        assert_eq!(record.originator, "National Weather Service");
        assert_eq!(record.callsign, "KXYZ");
        assert!(!record.is_national);
        assert!(!record.is_test);
        assert_eq!(record.location_codes, vec!["006085"]);
        assert_eq!(record.location_names, vec!["Central Santa Clara"]);
        assert_eq!(record.message_text, "test alert");
        assert!(!record.to_json_line().contains('\n'));
    }

    #[test]
    fn alert_record_is_emitted_for_accepted_alerts() {
        let alerts = Rc::new(RefCell::new(Vec::new()));
        let mut emitter = RecordingEventEmitter::new(Rc::clone(&alerts));
        let alert = Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            vec!["006085".to_string()],
            vec!["Central Santa Clara".to_string()],
            "test alert".to_string(),
        );
        let record = alert_record_from_alert_at(&alert, 2, 123);

        emit_alert_record(&mut emitter, &record);

        assert_eq!(*alerts.borrow(), vec![record]);
    }

    #[test]
    fn alert_event_write_failure_does_not_panic() {
        let alerts = Rc::new(RefCell::new(Vec::new()));
        let mut emitter =
            RecordingEventEmitter::new(Rc::clone(&alerts)).with_error("disk unavailable");
        let alert = Alert::new(
            "Tornado Warning".to_string(),
            AlertSignificance::Warning,
            "National Weather Service".to_string(),
            "KXYZ".to_string(),
            false,
            Vec::new(),
            Vec::new(),
            "test alert".to_string(),
        );
        let record = alert_record_from_alert_at(&alert, 0, 123);

        emit_alert_record(&mut emitter, &record);

        assert_eq!(*alerts.borrow(), vec![record]);
    }

    #[test]
    fn fanout_has_no_spooler_without_spool_path() {
        let args = Args::try_parse_from(["alerter"]).unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert!(!fanout.has_spooler());
    }

    #[test]
    fn fanout_configures_spooler_when_spool_path_is_provided() {
        let args = Args::try_parse_from(["alerter", "--spool-path", "failed-sends.jsonl"]).unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert!(fanout.has_spooler());
        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 0);
    }

    #[test]
    fn fanout_registers_no_lxmf_sender_without_lxmf_flags() {
        let args = Args::try_parse_from(["alerter"]).unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 0);
    }

    #[test]
    fn fanout_registers_lxmf_as_best_effort_when_required_flags_are_provided() {
        let args = Args::try_parse_from([
            "alerter",
            "--lxmf-command",
            "lxmf-send",
            "--lxmf-destination",
            "dest-123",
        ])
        .unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 1);
    }

    #[test]
    fn fanout_registers_discord_and_lxmf_as_best_effort_when_both_are_configured() {
        let args = Args::try_parse_from([
            "alerter",
            "--discord-webhook-url",
            "https://discord.example/webhook",
            "--lxmf-command",
            "lxmf-send",
            "--lxmf-destination",
            "dest-123",
        ])
        .unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 2);
    }

    #[test]
    fn fanout_registers_no_meshcore_sender_without_meshcore_flags() {
        let args = Args::try_parse_from(["alerter"]).unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 0);
    }

    #[test]
    fn fanout_registers_meshcore_as_best_effort_when_required_flags_are_provided() {
        let args = Args::try_parse_from([
            "alerter",
            "--meshcore-command",
            "meshcore-send",
            "--meshcore-destination",
            "room-123",
        ])
        .unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 1);
    }

    #[test]
    fn fanout_registers_discord_lxmf_and_meshcore_as_best_effort_when_configured() {
        let args = Args::try_parse_from([
            "alerter",
            "--discord-webhook-url",
            "https://discord.example/webhook",
            "--lxmf-command",
            "lxmf-send",
            "--lxmf-destination",
            "dest-123",
            "--meshcore-command",
            "meshcore-send",
            "--meshcore-destination",
            "room-123",
        ])
        .unwrap();
        let fanout = build_fanout(&args).unwrap();

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 3);
    }

    #[test]
    fn partial_lxmf_config_returns_clear_error() {
        let command_only =
            Args::try_parse_from(["alerter", "--lxmf-command", "lxmf-send"]).unwrap();
        assert_eq!(
            build_fanout(&command_only).err().unwrap().to_string(),
            "--lxmf-command requires --lxmf-destination"
        );

        let destination_only =
            Args::try_parse_from(["alerter", "--lxmf-destination", "dest-123"]).unwrap();
        assert_eq!(
            build_fanout(&destination_only).err().unwrap().to_string(),
            "--lxmf-destination requires --lxmf-command"
        );

        let config_only =
            Args::try_parse_from(["alerter", "--lxmf-config", "reticulum-config"]).unwrap();
        assert_eq!(
            build_fanout(&config_only).err().unwrap().to_string(),
            "--lxmf-config requires --lxmf-command and --lxmf-destination"
        );
    }

    #[test]
    fn partial_meshcore_config_returns_clear_error() {
        let command_only =
            Args::try_parse_from(["alerter", "--meshcore-command", "meshcore-send"]).unwrap();
        assert_eq!(
            build_fanout(&command_only).err().unwrap().to_string(),
            "--meshcore-command requires --meshcore-destination"
        );

        let destination_only =
            Args::try_parse_from(["alerter", "--meshcore-destination", "room-123"]).unwrap();
        assert_eq!(
            build_fanout(&destination_only).err().unwrap().to_string(),
            "--meshcore-destination requires --meshcore-command"
        );

        let config_only =
            Args::try_parse_from(["alerter", "--meshcore-config", "meshcore-config"]).unwrap();
        assert_eq!(
            build_fanout(&config_only).err().unwrap().to_string(),
            "--meshcore-config requires --meshcore-command and --meshcore-destination"
        );
    }
}
