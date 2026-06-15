mod alert;
mod discord_sender;
mod meshtastic_sender;
mod sender;

use alert::{
    alert_send_channel, format_alert_message, location_filter_allows, Alert, AlertMessageParts,
    AlertSignificance,
};
use anyhow::Result;
use byteorder::{NativeEndian, ReadBytesExt};
use clap::Parser;
use csv::ReaderBuilder;
use discord_sender::DiscordSender;
use log::LevelFilter;
use meshtastic_sender::{MeshtasticConfig, MeshtasticSender};
use rust_embed::RustEmbed;
use sameold::{Message, SameReceiverBuilder, SignificanceLevel};
use sender::{FanOut, Sender};
use serde::Deserialize;
use simple_logger::SimpleLogger;
use std::collections::HashMap;
use std::io::{self};
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
}

fn build_fanout(args: &Args) -> FanOut {
    let required_senders: Vec<Box<dyn Sender>> =
        vec![Box::new(MeshtasticSender::new(MeshtasticConfig {
            host: args.host.clone(),
            port: args.port.clone(),
        }))];
    let mut best_effort_senders: Vec<Box<dyn Sender>> = Vec::new();

    if let Some(webhook_url) = &args.discord_webhook_url {
        best_effort_senders.push(Box::new(DiscordSender::new(webhook_url.clone())));
        FanOut::with_best_effort(required_senders, best_effort_senders)
    } else {
        FanOut::new(required_senders)
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

    let mut fanout = build_fanout(&args);

    if let Err(e) = fanout.check_ready() {
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

    // Process messages from the audio source
    for msg in rx.iter_messages(audiosrc.map(|sa| sa as f32)) {
        match msg {
            Message::StartOfMessage(hdr) => {
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

                fanout
                    .send_alert(&alert, send_channel)
                    .expect("Failed sending msg");
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

    #[test]
    fn fanout_registers_only_meshtastic_without_discord_webhook() {
        let args = Args::try_parse_from(["alerter"]).unwrap();
        let fanout = build_fanout(&args);

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
        let fanout = build_fanout(&args);

        assert_eq!(fanout.required_sender_count(), 1);
        assert_eq!(fanout.best_effort_sender_count(), 1);
    }
}
