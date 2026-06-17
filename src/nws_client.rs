use anyhow::{Context, Result};
use serde::Deserialize;

pub const DEFAULT_NWS_API_BASE_URL: &str = "https://api.weather.gov";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwsClientConfig {
    pub base_url: String,
    pub user_agent: String,
    pub area: Option<String>,
    pub zone: Option<String>,
}

impl NwsClientConfig {
    pub fn active_alerts_url(&self) -> Result<String> {
        self.validate()?;

        let base_url = self.base_url.trim_end_matches('/');
        let mut url = format!("{}/alerts/active", base_url);

        if let Some(area) = &self.area {
            url.push_str("?area=");
            url.push_str(area);
        } else if let Some(zone) = &self.zone {
            url.push_str("?zone=");
            url.push_str(zone);
        }

        Ok(url)
    }

    pub fn validate(&self) -> Result<()> {
        if self.user_agent.trim().is_empty() {
            anyhow::bail!("NWS API User-Agent is required");
        }

        if self.area.is_some() && self.zone.is_some() {
            anyhow::bail!("configure either NWS area or NWS zone, not both");
        }

        Ok(())
    }
}

impl Default for NwsClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_NWS_API_BASE_URL.to_string(),
            user_agent: String::new(),
            area: None,
            zone: None,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct NwsAlertCollection {
    #[serde(default)]
    pub features: Vec<NwsAlertFeature>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct NwsAlertFeature {
    pub id: String,
    pub properties: NwsAlertProperties,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct NwsAlertProperties {
    pub id: Option<String>,
    pub event: Option<String>,
    pub headline: Option<String>,
    pub description: Option<String>,
    pub instruction: Option<String>,
    pub severity: Option<String>,
    pub urgency: Option<String>,
    pub certainty: Option<String>,
    pub effective: Option<String>,
    pub expires: Option<String>,
    pub ends: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "messageType")]
    pub message_type: Option<String>,
    #[serde(rename = "areaDesc")]
    pub area_desc: Option<String>,
    pub geocode: Option<NwsGeocode>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct NwsGeocode {
    #[serde(rename = "SAME")]
    pub same: Option<Vec<String>>,
    #[serde(rename = "UGC")]
    pub ugc: Option<Vec<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NwsHttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait NwsHttpClient {
    fn get(&self, url: &str, user_agent: &str) -> Result<NwsHttpResponse>;
}

pub struct UreqNwsHttpClient;

impl NwsHttpClient for UreqNwsHttpClient {
    fn get(&self, url: &str, user_agent: &str) -> Result<NwsHttpResponse> {
        let response = ureq::get(url)
            .set("Accept", "application/geo+json")
            .set("User-Agent", user_agent)
            .call();

        match response {
            Ok(response) => {
                let status = response.status();
                let body = response
                    .into_string()
                    .context("failed to read NWS API response body")?;
                Ok(NwsHttpResponse { status, body })
            }
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_else(|_| String::new());
                Ok(NwsHttpResponse { status, body })
            }
            Err(ureq::Error::Transport(error)) => {
                anyhow::bail!("NWS API transport failure: {}", error);
            }
        }
    }
}

pub struct NwsClient<T> {
    config: NwsClientConfig,
    http_client: T,
}

impl<T> NwsClient<T>
where
    T: NwsHttpClient,
{
    pub fn new(config: NwsClientConfig, http_client: T) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            http_client,
        })
    }

    pub fn fetch_active_alerts(&self) -> Result<NwsAlertCollection> {
        let url = self.config.active_alerts_url()?;
        let response = self.http_client.get(&url, &self.config.user_agent)?;

        if !(200..300).contains(&response.status) {
            anyhow::bail!("NWS API request failed with status {}", response.status);
        }

        parse_active_alerts(&response.body)
    }
}

pub fn parse_active_alerts(body: &str) -> Result<NwsAlertCollection> {
    serde_json::from_str(body).context("failed to parse NWS active alerts JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use std::cell::RefCell;

    const MINIMAL_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_minimal.json");
    const EMPTY_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_empty.json");
    const MISSING_OPTIONAL_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_missing_optional.json");
    const MALFORMED_ACTIVE_ALERTS: &str =
        include_str!("../tests/fixtures/nws_active_alerts_malformed.json");

    #[derive(Debug)]
    struct MockHttpClient {
        response: Result<NwsHttpResponse>,
        seen_requests: RefCell<Vec<(String, String)>>,
    }

    impl MockHttpClient {
        fn ok(body: &str) -> Self {
            Self {
                response: Ok(NwsHttpResponse {
                    status: 200,
                    body: body.to_string(),
                }),
                seen_requests: RefCell::new(Vec::new()),
            }
        }

        fn status(status: u16) -> Self {
            Self {
                response: Ok(NwsHttpResponse {
                    status,
                    body: String::new(),
                }),
                seen_requests: RefCell::new(Vec::new()),
            }
        }

        fn transport_failure() -> Self {
            Self {
                response: Err(anyhow!("network unavailable")),
                seen_requests: RefCell::new(Vec::new()),
            }
        }
    }

    impl NwsHttpClient for MockHttpClient {
        fn get(&self, url: &str, user_agent: &str) -> Result<NwsHttpResponse> {
            self.seen_requests
                .borrow_mut()
                .push((url.to_string(), user_agent.to_string()));
            match &self.response {
                Ok(response) => Ok(NwsHttpResponse {
                    status: response.status,
                    body: response.body.clone(),
                }),
                Err(error) => Err(anyhow!(error.to_string())),
            }
        }
    }

    fn config() -> NwsClientConfig {
        NwsClientConfig {
            base_url: "https://api.weather.gov".to_string(),
            user_agent: "Project-Sentinel test@example.com".to_string(),
            area: None,
            zone: None,
        }
    }

    #[test]
    fn parses_active_alert_collection() {
        let collection = parse_active_alerts(MINIMAL_ACTIVE_ALERTS).unwrap();

        assert_eq!(collection.features.len(), 1);
        let alert = &collection.features[0];
        assert_eq!(alert.id, "https://api.weather.gov/alerts/urn:oid:1");
        assert_eq!(alert.properties.event.as_deref(), Some("Tornado Warning"));
        assert_eq!(
            alert.properties.headline.as_deref(),
            Some("Tornado Warning issued June 16 at 1:00PM CDT")
        );
        assert_eq!(
            alert.properties.geocode.as_ref().unwrap().same.as_ref(),
            Some(&vec!["048201".to_string()])
        );
        assert_eq!(
            alert.properties.geocode.as_ref().unwrap().ugc.as_ref(),
            Some(&vec!["TXC201".to_string()])
        );
    }

    #[test]
    fn parses_empty_active_alerts() {
        let collection = parse_active_alerts(EMPTY_ACTIVE_ALERTS).unwrap();

        assert!(collection.features.is_empty());
    }

    #[test]
    fn missing_optional_fields_do_not_fail_parsing() {
        let collection = parse_active_alerts(MISSING_OPTIONAL_ACTIVE_ALERTS).unwrap();

        assert_eq!(collection.features.len(), 1);
        let properties = &collection.features[0].properties;
        assert_eq!(
            properties.event.as_deref(),
            Some("Severe Thunderstorm Warning")
        );
        assert_eq!(properties.headline, None);
        assert_eq!(properties.geocode, None);
    }

    #[test]
    fn malformed_json_returns_clear_error() {
        let error = parse_active_alerts(MALFORMED_ACTIVE_ALERTS).unwrap_err();

        assert!(error
            .to_string()
            .contains("failed to parse NWS active alerts JSON"));
    }

    #[test]
    fn user_agent_is_required() {
        let config = NwsClientConfig {
            user_agent: " ".to_string(),
            ..config()
        };

        let error = config.validate().unwrap_err();

        assert!(error.to_string().contains("User-Agent is required"));
    }

    #[test]
    fn active_alerts_url_without_filters_is_deterministic() {
        let config = config();

        assert_eq!(
            config.active_alerts_url().unwrap(),
            "https://api.weather.gov/alerts/active"
        );
    }

    #[test]
    fn active_alerts_url_with_area_filter_is_deterministic() {
        let config = NwsClientConfig {
            area: Some("TX".to_string()),
            ..config()
        };

        assert_eq!(
            config.active_alerts_url().unwrap(),
            "https://api.weather.gov/alerts/active?area=TX"
        );
    }

    #[test]
    fn active_alerts_url_with_zone_filter_is_deterministic() {
        let config = NwsClientConfig {
            zone: Some("TXC201".to_string()),
            ..config()
        };

        assert_eq!(
            config.active_alerts_url().unwrap(),
            "https://api.weather.gov/alerts/active?zone=TXC201"
        );
    }

    #[test]
    fn area_and_zone_together_are_rejected() {
        let config = NwsClientConfig {
            area: Some("TX".to_string()),
            zone: Some("TXC201".to_string()),
            ..config()
        };

        let error = config.active_alerts_url().unwrap_err();

        assert!(error
            .to_string()
            .contains("configure either NWS area or NWS zone, not both"));
    }

    #[test]
    fn fetch_active_alerts_uses_url_and_user_agent() {
        let http_client = MockHttpClient::ok(EMPTY_ACTIVE_ALERTS);
        let client = NwsClient::new(config(), http_client).unwrap();

        let collection = client.fetch_active_alerts().unwrap();

        assert!(collection.features.is_empty());
        assert_eq!(
            client.http_client.seen_requests.borrow().as_slice(),
            &[(
                "https://api.weather.gov/alerts/active".to_string(),
                "Project-Sentinel test@example.com".to_string()
            )]
        );
    }

    #[test]
    fn http_non_200_becomes_clear_error() {
        let client = NwsClient::new(config(), MockHttpClient::status(503)).unwrap();

        let error = client.fetch_active_alerts().unwrap_err();

        assert!(error
            .to_string()
            .contains("NWS API request failed with status 503"));
    }

    #[test]
    fn transport_failure_becomes_clear_error() {
        let client = NwsClient::new(config(), MockHttpClient::transport_failure()).unwrap();

        let error = client.fetch_active_alerts().unwrap_err();

        assert!(error.to_string().contains("network unavailable"));
    }
}
