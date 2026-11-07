use openidea_plugin::{Error, HttpRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Settings {
    pub active: String,
    pub environments: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            active: "local".into(),
            environments: BTreeMap::from([(
                "local".into(),
                BTreeMap::from([("baseUrl".into(), "http://localhost:8080".into())]),
            )]),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), Error> {
        if !self.environments.contains_key(&self.active) {
            return Err(Error::Invalid("environment_missing".into()));
        }
        if serde_json::to_vec(self).map_err(|e| Error::Invalid(e.to_string()))?.len() > 128 << 10 {
            return Err(Error::Invalid("environment_size".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Draft {
    pub method: String,
    pub url: String,
    pub headers: String,
    pub body: String,
}

pub fn substitute(text: &str, variables: &BTreeMap<String, String>) -> Result<String, Error> {
    let mut value = text.to_owned();
    for _ in 0..16 {
        let mut output = String::new();
        let mut rest = value.as_str();
        while let Some(start) = rest.find("{{") {
            output.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find("}}").ok_or_else(|| Error::Invalid("variable_syntax".into()))?;
            let name = after[..end].trim();
            let replacement = variables.get(name).ok_or_else(|| Error::Invalid(format!("variable:{name}")))?;
            output.push_str(replacement);
            if output.len() > 512 << 10 {
                return Err(Error::Invalid("request_size".into()));
            }
            rest = &after[end + 2..];
        }
        output.push_str(rest);
        if output.len() > 512 << 10 {
            return Err(Error::Invalid("request_size".into()));
        }
        if !output.contains("{{") {
            return Ok(output);
        }
        value = output;
    }
    Err(Error::Invalid("variable_cycle".into()))
}

pub fn prepare(draft: &Draft, settings: &Settings) -> Result<HttpRequest, Error> {
    settings.validate()?;
    let variables = &settings.environments[&settings.active];
    let method = draft.method.trim().to_ascii_uppercase();
    if !["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"].contains(&method.as_str()) {
        return Err(Error::Invalid("invalid_method".into()));
    }
    let raw_url = substitute(draft.url.trim(), variables)?;
    let url = url::Url::parse(&raw_url).map_err(|_| Error::Invalid("url".into()))?;
    match url.scheme() {
        "https" => {}
        "http" if matches!(url.host_str(), Some("localhost" | "127.0.0.1")) => {}
        "http" => return Err(Error::Invalid("http_local_only".into())),
        _ => return Err(Error::Invalid("url".into())),
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Invalid("url".into()));
    }
    let mut headers = Vec::new();
    for line in draft.headers.lines().filter(|line| !line.trim().is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| Error::Invalid("invalid_headers".into()))?;
        let name = name.trim();
        let value = substitute(value.trim(), variables)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || value.contains(['\r', '\n', '\0'])
        {
            return Err(Error::Invalid("invalid_headers".into()));
        }
        headers.push((name.to_owned(), value));
    }
    let body = substitute(&draft.body, variables)?;
    Ok(HttpRequest {
        method,
        url: url.into(),
        headers,
        body: (!body.is_empty()).then(|| body.into_bytes()),
        timeout_ms: Some(30_000),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(url: &str) -> Draft {
        Draft { method: "get".into(), url: url.into(), headers: String::new(), body: String::new() }
    }

    #[test]
    fn resolves_selected_environment_in_url_headers_and_body() {
        let settings = Settings {
            active: "dev".into(),
            environments: BTreeMap::from([(
                "dev".into(),
                BTreeMap::from([
                    ("baseUrl".into(), "http://localhost:9010".into()),
                    ("token".into(), "demo".into()),
                    ("api".into(), "{{baseUrl}}/api".into()),
                ]),
            )]),
        };
        let mut input = draft("{{ api }}/users?q=a");
        input.headers = "Authorization: Bearer {{token}}\nX-Time: 12:30".into();
        input.body = "{\"token\":\"{{token}}\"}".into();
        let result = prepare(&input, &settings).unwrap();
        assert_eq!(result.url, "http://localhost:9010/api/users?q=a");
        assert_eq!(result.headers[0].1, "Bearer demo");
        assert_eq!(result.headers[1].1, "12:30");
        assert_eq!(result.body.unwrap(), br#"{"token":"demo"}"#);
    }

    #[test]
    fn rejects_unresolved_cycles_and_unsupported_network_targets() {
        let variables = BTreeMap::from([("a".into(), "{{b}}".into()), ("b".into(), "{{a}}".into())]);
        assert!(substitute("{{missing}}", &variables).is_err());
        assert!(substitute("{{a}}", &variables).is_err());
        assert!(substitute("{{a", &variables).is_err());
        for url in
            ["http://192.168.1.1/", "http://dev.company.local/", "ftp://localhost/", "https://user:pass@example.com/"]
        {
            assert!(prepare(&draft(url), &Settings::default()).is_err(), "{url}");
        }
        assert!(prepare(&draft("http://127.0.0.1:9010/"), &Settings::default()).is_ok());
        assert!(prepare(&draft("https://example.com/"), &Settings::default()).is_ok());
    }

    #[test]
    fn rejects_header_injection_from_variables() {
        let mut settings = Settings::default();
        settings.environments.get_mut("local").unwrap().insert("bad".into(), "a\r\nInjected: yes".into());
        let mut input = draft("http://localhost/");
        input.headers = "X-Test: {{bad}}".into();
        assert!(prepare(&input, &settings).is_err());
    }
}
