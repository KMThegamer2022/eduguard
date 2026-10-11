//! Agent-side keyword detection (A1/A2). Matches window titles and browser
//! URL-ish text against the server-managed library and raises alerts *with
//! context* — never a raw keylog (GOAL guardrail #3).

use protocol::{AlertSource, KeywordAlert, KeywordRule};
#[cfg(test)]
use protocol::Severity;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Per-keyword cooldown so a persistent match doesn't spam the feed.
const COOLDOWN: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct Scanner {
    rules: Vec<KeywordRule>,
    enabled_categories: Vec<String>,
    last_alert: Mutex<HashMap<String, Instant>>,
}

impl Scanner {
    pub fn new() -> Self {
        Scanner::default()
    }

    pub fn set_rules(&mut self, rules: Vec<KeywordRule>, enabled_categories: Vec<String>) {
        self.rules = rules;
        self.enabled_categories = enabled_categories;
    }

    fn category_enabled(&self, cat: &str) -> bool {
        self.enabled_categories.is_empty() || self.enabled_categories.iter().any(|c| c == cat)
    }

    /// Inspect one sample of on-screen text; returns at most one alert.
    pub fn scan(&self, app: &str, title: &str, url: Option<&str>) -> Option<KeywordAlert> {
        let title_l = title.to_lowercase();
        let url_l = url.map(|u| u.to_lowercase());
        let now = Instant::now();
        let mut guard = self.last_alert.lock().ok()?;

        for rule in &self.rules {
            if !self.category_enabled(&rule.category) {
                continue;
            }
            let kw = rule.keyword.to_lowercase();
            if kw.is_empty() {
                continue;
            }
            let in_url = url_l.as_deref().map(|u| u.contains(&kw)).unwrap_or(false);
            let in_title = title_l.contains(&kw);
            if !(in_url || in_title) {
                continue;
            }
            let key = format!("{}|{}", rule.keyword, app);
            if let Some(prev) = guard.get(&key) {
                if now.duration_since(*prev) < COOLDOWN {
                    continue;
                }
            }
            guard.insert(key, now);
            guard.retain(|_, t| now.duration_since(*t) < COOLDOWN * 2);
            let context = if in_url {
                url_l.clone().unwrap_or_default()
            } else {
                title_l.clone()
            };
            return Some(KeywordAlert {
                ts: chrono::Utc::now().timestamp(),
                category: rule.category.clone(),
                keyword: rule.keyword.clone(),
                context: truncate(&context, 240),
                source: if in_url { AlertSource::Url } else { AlertSource::WindowTitle },
                app: app.to_string(),
                severity: rule.severity,
                evidence_ref: None,
            });
        }
        None
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

/// Heuristic: extract a URL-ish site token from browser window titles.
/// Browser titles look like "Page title - Site - Google Chrome"; the site part
/// is only useful when it contains a domain-shaped token.
pub fn url_from_title(title: &str) -> Option<String> {
    let browsers = [
        "google chrome",
        "chromium",
        "microsoft edge",
        "firefox",
        "safari",
        "brave",
    ];
    let lower = title.to_lowercase();
    if !browsers.iter().any(|b| lower.contains(b)) {
        return None;
    }
    // Look from the end for a token that looks like a domain.
    for part in title.rsplit(" - ").skip(1) {
        for token in part.split_whitespace() {
            let t = token.trim_matches(|c| c == '(' || c == ')' || c == '[' || c == ']');
            if t.contains('.') && !t.ends_with('.') && t.len() > 3 {
                let t = t.trim_end_matches('.');
                // Require a plausible TLD shape (letters after last dot).
                if let Some((_, tld)) = t.rsplit_once('.') {
                    if tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()) {
                        return Some(t.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(kw: &str, cat: &str, sev: Severity) -> KeywordRule {
        KeywordRule {
            keyword: kw.into(),
            category: cat.into(),
            severity: sev,
            note: String::new(),
        }
    }

    #[test]
    fn detects_and_cooldowns() {
        let mut s = Scanner::new();
        s.set_rules(vec![rule("suicide", "self_harm", Severity::High)], vec![]);
        let a = s.scan("Notes", "I want to talk about SUICIDE prevention", None).unwrap();
        assert_eq!(a.category, "self_harm");
        assert_eq!(a.source, AlertSource::WindowTitle);
        // Within cooldown: no second alert.
        assert!(s.scan("Notes", "suicide", None).is_none());
    }

    #[test]
    fn category_filter_respected() {
        let mut s = Scanner::new();
        s.set_rules(vec![rule("weed", "drugs", Severity::Low)], vec!["violence".into()]);
        assert!(s.scan("Browser", "weed dealer near school", None).is_none());
    }

    #[test]
    fn url_source_preferred() {
        let mut s = Scanner::new();
        s.set_rules(vec![rule("cocaine", "drugs", Severity::Medium)], vec![]);
        let a = s
            .scan("Chrome", "Cocaine wiki - Google Chrome", Some("https://drugs.example/cocaine"))
            .unwrap();
        assert_eq!(a.source, AlertSource::Url);
        assert!(a.context.contains("drugs.example"));
    }

    #[test]
    fn browser_url_heuristic() {
        assert_eq!(
            url_from_title("Download Paper - docs.example.sch.uk - Google Chrome").as_deref(),
            Some("docs.example.sch.uk")
        );
        // Plain browser title without domain-shaped text yields no URL.
        assert!(url_from_title("Funny Cats - YouTube - Google Chrome").is_none());
        assert!(url_from_title("my terminal window").is_none());
    }
}
