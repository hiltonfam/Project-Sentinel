use crate::dedup::DedupGate;
use crate::normalized_alert::NormalizedAlert;
use crate::nws_client::{NwsAlertCollection, NwsClient, NwsHttpClient};
use anyhow::Result;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_NWS_POLL_SECONDS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwsPollingConfig {
    pub poll_seconds: u64,
}

impl NwsPollingConfig {
    pub fn new(poll_seconds: u64) -> Result<Self> {
        if poll_seconds == 0 {
            anyhow::bail!("--nws-poll-seconds must be greater than 0");
        }

        Ok(Self { poll_seconds })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NwsPollingSummary {
    pub fetched_alerts: usize,
    pub normalized_alerts: usize,
    pub accepted_alerts: usize,
    pub duplicate_alerts: usize,
    pub normalization_errors: usize,
}

pub trait ActiveAlertFetcher {
    fn fetch_active_alerts(&self) -> Result<NwsAlertCollection>;
}

impl<T> ActiveAlertFetcher for NwsClient<T>
where
    T: NwsHttpClient,
{
    fn fetch_active_alerts(&self) -> Result<NwsAlertCollection> {
        self.fetch_active_alerts()
    }
}

pub trait PollSleeper {
    fn sleep(&self, duration: Duration);
}

pub struct StdPollSleeper;

impl PollSleeper for StdPollSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub trait PollClock {
    fn now_unix_secs(&self) -> u64;
}

pub struct SystemPollClock;

impl PollClock for SystemPollClock {
    fn now_unix_secs(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

pub fn run_nws_polling_loop<F>(fetcher: &F, config: &NwsPollingConfig) -> Result<()>
where
    F: ActiveAlertFetcher,
{
    let mut dedup_gate = DedupGate::default();
    let clock = SystemPollClock;
    let sleeper = StdPollSleeper;

    loop {
        match run_nws_polling_once(fetcher, &mut dedup_gate, &clock) {
            Ok(summary) => log_nws_polling_summary(&summary),
            Err(e) => log::warn!("NOAA/NWS API polling failed: {}", e),
        }
        sleeper.sleep(Duration::from_secs(config.poll_seconds));
    }
}

#[cfg(test)]
pub fn run_nws_polling_iterations<F, C, S>(
    fetcher: &F,
    dedup_gate: &mut DedupGate,
    config: &NwsPollingConfig,
    clock: &C,
    sleeper: &S,
    iterations: usize,
) -> Result<Vec<NwsPollingSummary>>
where
    F: ActiveAlertFetcher,
    C: PollClock,
    S: PollSleeper,
{
    let mut summaries = Vec::new();

    for iteration in 0..iterations {
        summaries.push(run_nws_polling_once(fetcher, dedup_gate, clock)?);
        if iteration + 1 < iterations {
            sleeper.sleep(Duration::from_secs(config.poll_seconds));
        }
    }

    Ok(summaries)
}

pub fn run_nws_polling_once<F, C>(
    fetcher: &F,
    dedup_gate: &mut DedupGate,
    clock: &C,
) -> Result<NwsPollingSummary>
where
    F: ActiveAlertFetcher,
    C: PollClock,
{
    let collection = fetcher.fetch_active_alerts()?;
    Ok(process_nws_alert_collection(
        collection,
        dedup_gate,
        clock.now_unix_secs(),
    ))
}

pub fn process_nws_alert_collection(
    collection: NwsAlertCollection,
    dedup_gate: &mut DedupGate,
    now_unix_secs: u64,
) -> NwsPollingSummary {
    let mut summary = NwsPollingSummary {
        fetched_alerts: collection.features.len(),
        ..NwsPollingSummary::default()
    };

    for feature in &collection.features {
        match NormalizedAlert::from_nws_alert(feature) {
            Ok(alert) => {
                summary.normalized_alerts += 1;
                let decision = dedup_gate.check_and_record(&alert, now_unix_secs);
                if decision.is_duplicate {
                    summary.duplicate_alerts += 1;
                } else {
                    summary.accepted_alerts += 1;
                    log::info!(
                        "NOAA/NWS alert accepted for future delivery: source_id={}, event={}",
                        alert.source_id,
                        alert.event
                    );
                }
            }
            Err(e) => {
                summary.normalization_errors += 1;
                log::warn!("NOAA/NWS alert normalization failed: {}", e);
            }
        }
    }

    summary
}

fn log_nws_polling_summary(summary: &NwsPollingSummary) {
    log::info!(
        "NOAA/NWS polling complete: fetched={}, normalized={}, accepted={}, duplicates={}, normalization_errors={}",
        summary.fetched_alerts,
        summary.normalized_alerts,
        summary.accepted_alerts,
        summary.duplicate_alerts,
        summary.normalization_errors
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nws_client::parse_active_alerts;
    use anyhow::anyhow;
    use std::cell::RefCell;

    const MINIMAL_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_minimal.json");
    const EMPTY_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_empty.json");
    const MISSING_EVENT_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_missing_event.json");

    struct MockFetcher {
        responses: RefCell<Vec<Result<NwsAlertCollection>>>,
        calls: RefCell<usize>,
    }

    impl MockFetcher {
        fn new(responses: Vec<Result<NwsAlertCollection>>) -> Self {
            Self {
                responses: RefCell::new(responses),
                calls: RefCell::new(0),
            }
        }
    }

    impl ActiveAlertFetcher for MockFetcher {
        fn fetch_active_alerts(&self) -> Result<NwsAlertCollection> {
            *self.calls.borrow_mut() += 1;
            self.responses.borrow_mut().remove(0)
        }
    }

    struct FixedClock {
        now_unix_secs: u64,
    }

    impl PollClock for FixedClock {
        fn now_unix_secs(&self) -> u64 {
            self.now_unix_secs
        }
    }

    #[derive(Default)]
    struct RecordingSleeper {
        sleeps: RefCell<Vec<Duration>>,
    }

    impl PollSleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) {
            self.sleeps.borrow_mut().push(duration);
        }
    }

    fn collection(fixture: &str) -> NwsAlertCollection {
        parse_active_alerts(fixture).unwrap()
    }

    #[test]
    fn polling_config_rejects_zero_interval() {
        let error = NwsPollingConfig::new(0).unwrap_err();

        assert!(error
            .to_string()
            .contains("--nws-poll-seconds must be greater than 0"));
    }

    #[test]
    fn polling_config_accepts_default_interval() {
        let config = NwsPollingConfig::new(DEFAULT_NWS_POLL_SECONDS).unwrap();

        assert_eq!(config.poll_seconds, 60);
    }

    #[test]
    fn api_fetch_path_is_used() {
        let fetcher = MockFetcher::new(vec![Ok(collection(EMPTY_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();

        let summary = run_nws_polling_once(&fetcher, &mut dedup_gate, &clock).unwrap();

        assert_eq!(*fetcher.calls.borrow(), 1);
        assert_eq!(summary.fetched_alerts, 0);
    }

    #[test]
    fn poll_loop_processes_alerts_without_delivery() {
        let fetcher = MockFetcher::new(vec![Ok(collection(MINIMAL_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();

        let summary = run_nws_polling_once(&fetcher, &mut dedup_gate, &clock).unwrap();

        assert_eq!(
            summary,
            NwsPollingSummary {
                fetched_alerts: 1,
                normalized_alerts: 1,
                accepted_alerts: 1,
                duplicate_alerts: 0,
                normalization_errors: 0,
            }
        );
    }

    #[test]
    fn dedup_path_suppresses_duplicate_alerts() {
        let fetcher = MockFetcher::new(vec![
            Ok(collection(MINIMAL_ACTIVE_ALERTS)),
            Ok(collection(MINIMAL_ACTIVE_ALERTS)),
        ]);
        let clock = FixedClock { now_unix_secs: 100 };
        let sleeper = RecordingSleeper::default();
        let config = NwsPollingConfig::new(60).unwrap();
        let mut dedup_gate = DedupGate::default();

        let summaries =
            run_nws_polling_iterations(&fetcher, &mut dedup_gate, &config, &clock, &sleeper, 2)
                .unwrap();

        assert_eq!(summaries[0].accepted_alerts, 1);
        assert_eq!(summaries[0].duplicate_alerts, 0);
        assert_eq!(summaries[1].accepted_alerts, 0);
        assert_eq!(summaries[1].duplicate_alerts, 1);
    }

    #[test]
    fn poll_loop_uses_configured_sleep_between_iterations() {
        let fetcher = MockFetcher::new(vec![
            Ok(collection(EMPTY_ACTIVE_ALERTS)),
            Ok(collection(EMPTY_ACTIVE_ALERTS)),
        ]);
        let clock = FixedClock { now_unix_secs: 100 };
        let sleeper = RecordingSleeper::default();
        let config = NwsPollingConfig::new(12).unwrap();
        let mut dedup_gate = DedupGate::default();

        let summaries =
            run_nws_polling_iterations(&fetcher, &mut dedup_gate, &config, &clock, &sleeper, 2)
                .unwrap();

        assert_eq!(summaries.len(), 2);
        assert_eq!(
            sleeper.sleeps.borrow().as_slice(),
            &[Duration::from_secs(12)]
        );
    }

    #[test]
    fn normalization_errors_are_counted_without_stopping_poll_processing() {
        let fetcher = MockFetcher::new(vec![Ok(collection(MISSING_EVENT_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();

        let summary = run_nws_polling_once(&fetcher, &mut dedup_gate, &clock).unwrap();

        assert_eq!(summary.fetched_alerts, 1);
        assert_eq!(summary.normalized_alerts, 0);
        assert_eq!(summary.accepted_alerts, 0);
        assert_eq!(summary.normalization_errors, 1);
    }

    #[test]
    fn fetch_failure_returns_clear_error() {
        let fetcher = MockFetcher::new(vec![Err(anyhow!("network unavailable"))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();

        let error = run_nws_polling_once(&fetcher, &mut dedup_gate, &clock).unwrap_err();

        assert!(error.to_string().contains("network unavailable"));
    }

    #[test]
    fn no_sender_or_fanout_is_required_for_polling() {
        let fetcher = MockFetcher::new(vec![Ok(collection(MINIMAL_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();

        let summary = run_nws_polling_once(&fetcher, &mut dedup_gate, &clock).unwrap();

        assert_eq!(summary.accepted_alerts, 1);
    }
}
