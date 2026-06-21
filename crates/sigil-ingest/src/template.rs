//! Online log template mining (DESIGN §6).
//!
//! A simplified, Drain-style miner: each message is tokenized and variable-like
//! tokens (numbers, IPs, hex, UUIDs, quoted strings) are masked to `<*>`. The
//! masked form is the *template*; its hash is a stable `template_id`, and the
//! masked-out tokens are the extracted *variables*. This captures Drain's core
//! idea (constant skeleton + variable slots) without the prefix-tree; the tree
//! optimization can replace [`TemplateMiner::mine`] later without changing the
//! [`Processor`] contract.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use regex::Regex;
use sigil_core::{ecs, Event, Plugin, PluginManifest, Processor, Result};

use crate::manifest;

const PLACEHOLDER: &str = "<*>";

/// Result of mining one message.
#[derive(Debug, Clone)]
pub struct Template {
    pub id: u64,
    pub template: String,
    pub variables: Vec<String>,
}

/// A [`Processor`] that mines `message` into a template id + variables.
pub struct TemplateMiner {
    var_token: Regex,
    manifest: PluginManifest,
}

impl Default for TemplateMiner {
    fn default() -> Self {
        // A token is "variable" if it is numeric, an IP, hex/uuid-ish, or quoted.
        let var_token = Regex::new(
            r"(?x)
            ^(?:
                [+-]?\d[\d.,:]*                    # numbers, durations, times
              | \d{1,3}(?:\.\d{1,3}){3}            # ipv4
              | 0x[0-9a-fA-F]+                     # hex
              | [0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}  # uuid
              | [0-9a-fA-F]{12,}                   # long hex blobs
            )$",
        )
        .expect("static var-token regex");
        TemplateMiner {
            var_token,
            manifest: manifest("drain", "process"),
        }
    }
}

impl TemplateMiner {
    /// Mine a single message into its template and variables.
    pub fn mine(&self, message: &str) -> Template {
        let mut template_tokens = Vec::new();
        let mut variables = Vec::new();
        for token in message.split_whitespace() {
            if self.is_variable(token) {
                template_tokens.push(PLACEHOLDER);
                variables.push(token.to_string());
            } else {
                template_tokens.push(token);
            }
        }
        let template = template_tokens.join(" ");
        let mut hasher = DefaultHasher::new();
        template.hash(&mut hasher);
        Template {
            id: hasher.finish(),
            template,
            variables,
        }
    }

    fn is_variable(&self, token: &str) -> bool {
        let trimmed = token.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | ',' | ';'));
        if trimmed.is_empty() {
            return false;
        }
        self.var_token.is_match(trimmed)
    }
}

impl Plugin for TemplateMiner {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }
}

impl Processor for TemplateMiner {
    fn process(&self, mut event: Event) -> Result<Vec<Event>> {
        if let Some(message) = event.get(ecs::MESSAGE).map(str::to_string) {
            let mined = self.mine(&message);
            event.template_id = Some(mined.id);
            event.set("log.template", mined.template);
            if !mined.variables.is_empty() {
                event.set("log.template.variables", mined.variables.join(" "));
            }
        }
        Ok(vec![event])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_skeleton_same_id() {
        let m = TemplateMiner::default();
        let a = m.mine("Failed password for root from 10.0.0.9 port 22");
        let b = m.mine("Failed password for root from 192.168.1.5 port 41122");
        assert_eq!(a.id, b.id, "same skeleton should share a template id");
        assert!(a.template.contains("Failed password for"));
        assert!(a.template.contains(PLACEHOLDER));
        assert!(a.variables.contains(&"10.0.0.9".to_string()));
    }

    #[test]
    fn different_skeleton_different_id() {
        let m = TemplateMiner::default();
        let a = m.mine("user alice logged in");
        let b = m.mine("disk full on /var");
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn processor_sets_template_id() {
        let mut ev = Event::default();
        ev.set(ecs::MESSAGE, "connection from 10.0.0.1 closed");
        let out = TemplateMiner::default().process(ev).unwrap();
        assert!(out[0].template_id.is_some());
        assert!(out[0].get("log.template").unwrap().contains("<*>"));
    }
}
