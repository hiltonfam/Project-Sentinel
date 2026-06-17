use crate::alert::{Alert, AlertSignificance};
use crate::dedup::DedupGate;
use crate::event_contracts::{AlertRecord, EVENT_CONTRACT_SCHEMA_VERSION};
use crate::event_emitter::{warn_event_write_failure, EventEmitter};
use crate::normalized_alert::NormalizedAlert;
use crate::nws_client::{NwsAlertCollection, NwsClient, NwsHttpClient};
use crate::sender::FanOut;
use crate::source_health::{source_status_record, SourceHealthInput, SourceKind};
use anyhow::Result;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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
    pub delivery_failures: usize,
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

pub trait NwsAlertDelivery {
    fn deliver_alert(&mut self, alert: &NormalizedAlert, timestamp_unix_secs: u64) -> Result<()>;

    fn record_source_status(&mut self, _record: &crate::event_contracts::SourceStatusRecord) {}
}

pub struct FanOutNwsAlertDelivery<'a> {
    fanout: &'a mut FanOut,
    channel: u32,
    event_emitter: Option<Box<dyn EventEmitter>>,
}

impl<'a> FanOutNwsAlertDelivery<'a> {
    pub fn new(
        fanout: &'a mut FanOut,
        channel: u32,
        event_emitter: Option<Box<dyn EventEmitter>>,
    ) -> Self {
        Self {
            fanout,
            channel,
            event_emitter,
        }
    }
}

impl NwsAlertDelivery for FanOutNwsAlertDelivery<'_> {
    fn deliver_alert(&mut self, alert: &NormalizedAlert, timestamp_unix_secs: u64) -> Result<()> {
        let runtime_alert = runtime_alert_from_normalized(alert);
        if let Some(event_emitter) = self.event_emitter.as_deref_mut() {
            let record = alert_record_from_normalized_at(
                alert,
                &runtime_alert,
                self.channel,
                timestamp_unix_secs,
            );
            emit_nws_alert_record(event_emitter, &record);
            self.fanout.send_alert_with_events(
                &runtime_alert,
                self.channel,
                &record.alert_id,
                event_emitter,
            )
        } else {
            self.fanout.send_alert(&runtime_alert, self.channel)
        }
    }

    fn record_source_status(&mut self, record: &crate::event_contracts::SourceStatusRecord) {
        if let Some(event_emitter) = self.event_emitter.as_deref_mut() {
            if let Err(e) = event_emitter.emit_source_status(record) {
                warn_event_write_failure(e);
            }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NwsSourceHealth {
    last_success_unix_secs: Option<u64>,
    last_failure_unix_secs: Option<u64>,
    last_error: Option<String>,
}

impl NwsSourceHealth {
    fn record_success(&mut self, now_unix_secs: u64) -> crate::event_contracts::SourceStatusRecord {
        self.last_success_unix_secs = Some(now_unix_secs);
        self.last_error = None;
        source_status_record(SourceHealthInput {
            source: SourceKind::NwsApi,
            now_unix_secs,
            last_success_unix_secs: self.last_success_unix_secs,
            last_failure_unix_secs: self.last_failure_unix_secs,
            last_decoded_message_unix_secs: None,
            last_accepted_alert_unix_secs: None,
            error: None,
        })
    }

    fn record_failure(
        &mut self,
        now_unix_secs: u64,
        error: String,
    ) -> crate::event_contracts::SourceStatusRecord {
        self.last_failure_unix_secs = Some(now_unix_secs);
        self.last_error = Some(error);
        source_status_record(SourceHealthInput {
            source: SourceKind::NwsApi,
            now_unix_secs,
            last_success_unix_secs: self.last_success_unix_secs,
            last_failure_unix_secs: self.last_failure_unix_secs,
            last_decoded_message_unix_secs: None,
            last_accepted_alert_unix_secs: None,
            error: self.last_error.clone(),
        })
    }
}

pub fn run_nws_polling_loop<F, D>(
    fetcher: &F,
    config: &NwsPollingConfig,
    delivery: &mut D,
) -> Result<()>
where
    F: ActiveAlertFetcher,
    D: NwsAlertDelivery,
{
    let mut dedup_gate = DedupGate::default();
    let mut source_health = NwsSourceHealth::default();
    let clock = SystemPollClock;
    let sleeper = StdPollSleeper;

    loop {
        match run_nws_polling_once(
            fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            delivery,
        ) {
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
    source_health: &mut NwsSourceHealth,
    config: &NwsPollingConfig,
    clock: &C,
    sleeper: &S,
    delivery: &mut impl NwsAlertDelivery,
    iterations: usize,
) -> Result<Vec<NwsPollingSummary>>
where
    F: ActiveAlertFetcher,
    C: PollClock,
    S: PollSleeper,
{
    let mut summaries = Vec::new();

    for iteration in 0..iterations {
        summaries.push(run_nws_polling_once(
            fetcher,
            dedup_gate,
            source_health,
            clock,
            delivery,
        )?);
        if iteration + 1 < iterations {
            sleeper.sleep(Duration::from_secs(config.poll_seconds));
        }
    }

    Ok(summaries)
}

pub fn run_nws_polling_once<F, C>(
    fetcher: &F,
    dedup_gate: &mut DedupGate,
    source_health: &mut NwsSourceHealth,
    clock: &C,
    delivery: &mut impl NwsAlertDelivery,
) -> Result<NwsPollingSummary>
where
    F: ActiveAlertFetcher,
    C: PollClock,
{
    let now_unix_secs = clock.now_unix_secs();
    let collection = match fetcher.fetch_active_alerts() {
        Ok(collection) => {
            let record = source_health.record_success(now_unix_secs);
            delivery.record_source_status(&record);
            collection
        }
        Err(e) => {
            let record = source_health.record_failure(now_unix_secs, e.to_string());
            delivery.record_source_status(&record);
            return Err(e);
        }
    };
    Ok(process_nws_alert_collection(
        collection,
        dedup_gate,
        now_unix_secs,
        delivery,
    ))
}

pub fn process_nws_alert_collection(
    collection: NwsAlertCollection,
    dedup_gate: &mut DedupGate,
    now_unix_secs: u64,
    delivery: &mut impl NwsAlertDelivery,
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
                    if let Err(e) = delivery.deliver_alert(&alert, now_unix_secs) {
                        summary.delivery_failures += 1;
                        log::error!("NOAA/NWS alert delivery failed: {}", e);
                    }
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
        "NOAA/NWS polling complete: fetched={}, normalized={}, accepted={}, duplicates={}, normalization_errors={}, delivery_failures={}",
        summary.fetched_alerts,
        summary.normalized_alerts,
        summary.accepted_alerts,
        summary.duplicate_alerts,
        summary.normalization_errors,
        summary.delivery_failures
    );
}

pub fn runtime_alert_from_normalized(alert: &NormalizedAlert) -> Alert {
    Alert::new(
        alert.event.clone(),
        significance_from_nws_severity(alert.severity.as_deref()),
        "National Weather Service".to_string(),
        "NWS API".to_string(),
        false,
        alert.same_codes.clone(),
        alert
            .area_desc
            .as_ref()
            .map(|area| vec![area.clone()])
            .unwrap_or_default(),
        alert.message_text.clone(),
    )
}

fn significance_from_nws_severity(severity: Option<&str>) -> AlertSignificance {
    match severity.unwrap_or_default().to_ascii_lowercase().as_str() {
        "minor" => AlertSignificance::Statement,
        "moderate" => AlertSignificance::Watch,
        "severe" | "extreme" => AlertSignificance::Warning,
        _ => AlertSignificance::Unknown,
    }
}

fn alert_record_from_normalized_at(
    normalized: &NormalizedAlert,
    runtime_alert: &Alert,
    channel: u32,
    timestamp_unix_secs: u64,
) -> AlertRecord {
    AlertRecord {
        schema_version: EVENT_CONTRACT_SCHEMA_VERSION,
        record_type: "alert".to_string(),
        alert_id: nws_alert_id_for(normalized, channel, timestamp_unix_secs),
        timestamp_unix_secs,
        source: "nws_api".to_string(),
        event: runtime_alert.event.clone(),
        significance: runtime_alert.significance,
        originator: runtime_alert.originator.clone(),
        callsign: runtime_alert.callsign.clone(),
        is_national: runtime_alert.is_national,
        is_test: runtime_alert.is_test,
        location_codes: runtime_alert.location_codes.clone(),
        location_names: runtime_alert.location_names.clone(),
        message_text: runtime_alert.message_text.clone(),
    }
}

fn nws_alert_id_for(alert: &NormalizedAlert, channel: u32, timestamp_unix_secs: u64) -> String {
    let mut hasher = DefaultHasher::new();
    alert.source_id.hash(&mut hasher);
    alert.event.hash(&mut hasher);
    alert.same_codes.hash(&mut hasher);
    alert.ugc_codes.hash(&mut hasher);
    channel.hash(&mut hasher);
    format!(
        "nws-api-{}-{}-{:016x}",
        timestamp_unix_secs,
        channel,
        hasher.finish()
    )
}

fn emit_nws_alert_record(event_emitter: &mut dyn EventEmitter, record: &AlertRecord) {
    if let Err(e) = event_emitter.emit_alert(record) {
        warn_event_write_failure(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_contracts::{
        DeliveryAttemptRecord, DeliveryAttemptStatus, SenderStatusRecord,
    };
    use crate::nws_client::parse_active_alerts;
    use crate::sender::Sender;
    use crate::spool::{SpoolRecord, Spooler};
    use anyhow::anyhow;
    use std::cell::RefCell;
    use std::rc::Rc;

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

    #[derive(Default)]
    struct RecordingDelivery {
        alerts: RefCell<Vec<NormalizedAlert>>,
        error: Option<&'static str>,
    }

    impl RecordingDelivery {
        fn with_error(error: &'static str) -> Self {
            Self {
                alerts: RefCell::new(Vec::new()),
                error: Some(error),
            }
        }
    }

    impl NwsAlertDelivery for RecordingDelivery {
        fn deliver_alert(
            &mut self,
            alert: &NormalizedAlert,
            _timestamp_unix_secs: u64,
        ) -> Result<()> {
            self.alerts.borrow_mut().push(alert.clone());

            if let Some(error) = self.error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
        }
    }

    struct RecordingSender {
        label: &'static str,
        calls: Rc<RefCell<Vec<&'static str>>>,
        send_error: Option<&'static str>,
    }

    impl RecordingSender {
        fn new(label: &'static str, calls: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                label,
                calls,
                send_error: None,
            }
        }

        fn with_send_error(mut self, error: &'static str) -> Self {
            self.send_error = Some(error);
            self
        }
    }

    impl Sender for RecordingSender {
        fn name(&self) -> &'static str {
            self.label
        }

        fn check_ready(&self) -> Result<()> {
            Ok(())
        }

        fn send_alert(&mut self, _alert: &Alert, _channel: u32) -> Result<()> {
            self.calls.borrow_mut().push(self.label);

            if let Some(error) = self.send_error {
                Err(anyhow!(error))
            } else {
                Ok(())
            }
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

    #[derive(Default)]
    struct RecordingEventEmitter {
        alerts: Rc<RefCell<Vec<AlertRecord>>>,
        delivery_attempts: Rc<RefCell<Vec<DeliveryAttemptRecord>>>,
        source_statuses: Rc<RefCell<Vec<crate::event_contracts::SourceStatusRecord>>>,
    }

    impl EventEmitter for RecordingEventEmitter {
        fn emit_alert(&mut self, record: &AlertRecord) -> Result<()> {
            self.alerts.borrow_mut().push(record.clone());
            Ok(())
        }

        fn emit_delivery_attempt(&mut self, record: &DeliveryAttemptRecord) -> Result<()> {
            self.delivery_attempts.borrow_mut().push(record.clone());
            Ok(())
        }

        fn emit_sender_status(&mut self, _record: &SenderStatusRecord) -> Result<()> {
            Ok(())
        }

        fn emit_source_status(
            &mut self,
            record: &crate::event_contracts::SourceStatusRecord,
        ) -> Result<()> {
            self.source_statuses.borrow_mut().push(record.clone());
            Ok(())
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
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let summary = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap();

        assert_eq!(*fetcher.calls.borrow(), 1);
        assert_eq!(summary.fetched_alerts, 0);
    }

    #[test]
    fn poll_loop_delivers_accepted_alerts() {
        let fetcher = MockFetcher::new(vec![Ok(collection(MINIMAL_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let summary = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap();

        assert_eq!(
            summary,
            NwsPollingSummary {
                fetched_alerts: 1,
                normalized_alerts: 1,
                accepted_alerts: 1,
                duplicate_alerts: 0,
                normalization_errors: 0,
                delivery_failures: 0,
            }
        );
        assert_eq!(delivery.alerts.borrow().len(), 1);
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
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let summaries = run_nws_polling_iterations(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &config,
            &clock,
            &sleeper,
            &mut delivery,
            2,
        )
        .unwrap();

        assert_eq!(summaries[0].accepted_alerts, 1);
        assert_eq!(summaries[0].duplicate_alerts, 0);
        assert_eq!(summaries[1].accepted_alerts, 0);
        assert_eq!(summaries[1].duplicate_alerts, 1);
        assert_eq!(delivery.alerts.borrow().len(), 1);
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
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let summaries = run_nws_polling_iterations(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &config,
            &clock,
            &sleeper,
            &mut delivery,
            2,
        )
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
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let summary = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap();

        assert_eq!(summary.fetched_alerts, 1);
        assert_eq!(summary.normalized_alerts, 0);
        assert_eq!(summary.accepted_alerts, 0);
        assert_eq!(summary.normalization_errors, 1);
        assert!(delivery.alerts.borrow().is_empty());
    }

    #[test]
    fn fetch_failure_returns_clear_error() {
        let fetcher = MockFetcher::new(vec![Err(anyhow!("network unavailable"))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::default();

        let error = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap_err();

        assert!(error.to_string().contains("network unavailable"));
        assert!(delivery.alerts.borrow().is_empty());
    }

    #[test]
    fn api_success_emits_healthy_source_status_when_events_are_enabled() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let source_statuses = Rc::new(RefCell::new(Vec::new()));
        let emitter = RecordingEventEmitter {
            source_statuses: Rc::clone(&source_statuses),
            ..RecordingEventEmitter::default()
        };
        let fetcher = MockFetcher::new(vec![Ok(collection(EMPTY_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();
        let mut source_health = NwsSourceHealth::default();
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, Some(Box::new(emitter)));

        let summary = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap();

        assert_eq!(summary.fetched_alerts, 0);
        let statuses = source_statuses.borrow();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].source, "nws_api");
        assert_eq!(
            statuses[0].state,
            crate::event_contracts::SourceState::Healthy
        );
        assert_eq!(statuses[0].last_success_unix_secs, Some(100));
    }

    #[test]
    fn api_failure_emits_source_status_without_delivery() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let source_statuses = Rc::new(RefCell::new(Vec::new()));
        let emitter = RecordingEventEmitter {
            source_statuses: Rc::clone(&source_statuses),
            ..RecordingEventEmitter::default()
        };
        let fetcher = MockFetcher::new(vec![Err(anyhow!("network unavailable"))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();
        let mut source_health = NwsSourceHealth::default();
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, Some(Box::new(emitter)));

        let error = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap_err();

        assert!(error.to_string().contains("network unavailable"));
        assert!(calls.borrow().is_empty());
        let statuses = source_statuses.borrow();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].source, "nws_api");
        assert_eq!(statuses[0].last_failure_unix_secs, Some(100));
        assert_eq!(statuses[0].error.as_deref(), Some("network unavailable"));
    }

    #[test]
    fn delivery_failure_is_counted_without_stopping_poll_processing() {
        let fetcher = MockFetcher::new(vec![Ok(collection(MINIMAL_ACTIVE_ALERTS))]);
        let clock = FixedClock { now_unix_secs: 100 };
        let mut dedup_gate = DedupGate::default();
        let mut source_health = NwsSourceHealth::default();
        let mut delivery = RecordingDelivery::with_error("delivery unavailable");

        let summary = run_nws_polling_once(
            &fetcher,
            &mut dedup_gate,
            &mut source_health,
            &clock,
            &mut delivery,
        )
        .unwrap();

        assert_eq!(summary.accepted_alerts, 1);
        assert_eq!(summary.delivery_failures, 1);
        assert_eq!(delivery.alerts.borrow().len(), 1);
    }

    #[test]
    fn api_accepted_alert_sends_through_fanout() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, None);
        let mut dedup_gate = DedupGate::default();

        let summary = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );

        assert_eq!(summary.accepted_alerts, 1);
        assert_eq!(summary.delivery_failures, 0);
        assert_eq!(*calls.borrow(), vec!["meshtastic"]);
    }

    #[test]
    fn duplicate_api_alert_is_suppressed_before_fanout() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, None);
        let mut dedup_gate = DedupGate::default();

        let first = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );
        let second = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            101,
            &mut delivery,
        );

        assert_eq!(first.accepted_alerts, 1);
        assert_eq!(second.accepted_alerts, 0);
        assert_eq!(second.duplicate_alerts, 1);
        assert_eq!(*calls.borrow(), vec!["meshtastic"]);
    }

    #[test]
    fn required_sender_failure_is_reported_as_delivery_failure() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(
            RecordingSender::new("meshtastic", Rc::clone(&calls))
                .with_send_error("required unavailable"),
        )]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, None);
        let mut dedup_gate = DedupGate::default();

        let summary = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );

        assert_eq!(summary.accepted_alerts, 1);
        assert_eq!(summary.delivery_failures, 1);
        assert_eq!(*calls.borrow(), vec!["meshtastic"]);
    }

    #[test]
    fn best_effort_sender_failure_does_not_fail_api_delivery_and_is_spooled() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let spooled = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::with_best_effort(
            vec![Box::new(RecordingSender::new(
                "meshtastic",
                Rc::clone(&calls),
            ))],
            vec![Box::new(
                RecordingSender::new("discord", Rc::clone(&calls))
                    .with_send_error("webhook unavailable"),
            )],
        )
        .with_spooler(Box::new(RecordingSpooler {
            records: Rc::clone(&spooled),
        }));
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, None);
        let mut dedup_gate = DedupGate::default();

        let summary = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );

        assert_eq!(summary.delivery_failures, 0);
        assert_eq!(*calls.borrow(), vec!["meshtastic", "discord"]);
        let records = spooled.borrow();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].sender, "discord");
        assert_eq!(records[0].channel, 2);
        assert_eq!(records[0].error, "webhook unavailable");
    }

    #[test]
    fn event_emission_records_nws_api_source_when_enabled() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let alerts = Rc::new(RefCell::new(Vec::new()));
        let attempts = Rc::new(RefCell::new(Vec::new()));
        let emitter = RecordingEventEmitter {
            alerts: Rc::clone(&alerts),
            delivery_attempts: Rc::clone(&attempts),
            source_statuses: Rc::new(RefCell::new(Vec::new())),
        };
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, Some(Box::new(emitter)));
        let mut dedup_gate = DedupGate::default();

        let summary = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );

        assert_eq!(summary.delivery_failures, 0);
        let alerts = alerts.borrow();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].source, "nws_api");
        assert!(alerts[0].alert_id.starts_with("nws-api-100-2-"));
        let attempts = attempts.borrow();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].sender, "meshtastic");
        assert!(attempts[0].required);
        assert_eq!(attempts[0].status, DeliveryAttemptStatus::Success);
        assert_eq!(attempts[0].alert_id, alerts[0].alert_id);
    }

    #[test]
    fn api_delivery_without_event_emitter_emits_no_event_records() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut fanout = FanOut::new(vec![Box::new(RecordingSender::new(
            "meshtastic",
            Rc::clone(&calls),
        ))]);
        let mut delivery = FanOutNwsAlertDelivery::new(&mut fanout, 2, None);
        let mut dedup_gate = DedupGate::default();

        let summary = process_nws_alert_collection(
            collection(MINIMAL_ACTIVE_ALERTS),
            &mut dedup_gate,
            100,
            &mut delivery,
        );

        assert_eq!(summary.delivery_failures, 0);
        assert_eq!(*calls.borrow(), vec!["meshtastic"]);
    }
}
