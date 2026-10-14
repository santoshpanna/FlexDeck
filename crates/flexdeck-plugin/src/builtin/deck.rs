//! The `deck` plugin: what a step can do to the deck itself.
//!
//! "Go to page" is the first: a key that opens the Stream page, a scheduled
//! automation that shows the meeting page at nine. The tablet does the turning;
//! this only says which page, through the injected [`PageControl`], so a test
//! can see what was asked without a tablet.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use flexdeck_actions::{ActionSpec, ArgumentSpec};
use flexdeck_vars::VariableSpec;

use crate::{argument, Ctx, Outcome, Plugin, PluginError, PluginManifest};

pub const DECK_PLUGIN: &str = "deck";

/// Which page to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageTarget {
    /// The page with this id.
    Page(String),
    /// So many pages on from the one showing (−1: back); wraps around.
    Step(i32),
}

/// Where page requests go: every connected tablet.
pub trait PageControl: Send + Sync + std::fmt::Debug {
    fn show(&self, target: PageTarget) -> Result<(), PluginError>;
    /// The deck's pages as (id, name), in order.
    fn pages(&self) -> Vec<(String, String)>;
}

static DECK_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        plugin: DECK_PLUGIN,
        action: "next_page",
        description: "Show the next page",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: DECK_PLUGIN,
        action: "previous_page",
        description: "Show the previous page",
        arguments: &[],
        slider: None,
        active: None,
    },
    ActionSpec {
        plugin: DECK_PLUGIN,
        action: "go_to_page",
        description: "Show a page",
        arguments: &[ArgumentSpec::text("page", "The page's name", "Stream")],
        slider: None,
        active: None,
    },
];

static DECK_MANIFEST: PluginManifest = PluginManifest {
    id: DECK_PLUGIN,
    name: "Deck",
    version: "1.0.0",
    description: "Turn the deck's pages.",
    actions: DECK_ACTIONS,
    settings: &[],
};

#[derive(Debug)]
pub struct Deck {
    control: Option<Arc<dyn PageControl>>,
}

impl Deck {
    pub fn new(control: Option<Arc<dyn PageControl>>) -> Self {
        Self { control }
    }

    fn control(&self) -> Result<&Arc<dyn PageControl>, PluginError> {
        self.control
            .as_ref()
            .ok_or_else(|| PluginError::Failed("pages cannot be turned from here".into()))
    }

    /// The page `wanted` names, by name (any case) or by id.
    fn find(&self, wanted: &str) -> Result<String, PluginError> {
        let wanted = wanted.trim();
        let pages = self.control()?.pages();
        pages
            .iter()
            .find(|(id, _)| id == wanted)
            .or_else(|| pages.iter().find(|(_, name)| name.trim().eq_ignore_ascii_case(wanted)))
            .map(|(id, _)| id.clone())
            .ok_or_else(|| PluginError::Failed(format!("the deck has no page called '{wanted}'")))
    }
}

impl Plugin for Deck {
    fn manifest(&self) -> crate::abi::ManifestDto {
        DECK_MANIFEST.to_dto()
    }

    fn variables(&self) -> Vec<VariableSpec> {
        Vec::new()
    }

    fn choices(
        &self,
        _action: &str,
        _args: &HashMap<String, String>,
    ) -> Result<crate::abi::ChoicesResponse, PluginError> {
        let mut answer = crate::abi::ChoicesResponse::default();
        let pages = self.control()?.pages();
        answer.arguments.insert(
            "page".into(),
            pages
                .into_iter()
                .enumerate()
                .map(|(index, (_, name))| crate::abi::ChoiceDto {
                    value: if name.trim().is_empty() { format!("Page {}", index + 1) } else { name },
                    label: None,
                })
                .collect(),
        );
        Ok(answer)
    }

    fn sample(&mut self, _wanted: &[&str], _ctx: &mut Ctx<'_>) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke(
        &mut self,
        action: &str,
        args: &HashMap<String, String>,
        _ctx: &mut Ctx<'_>,
    ) -> Result<Outcome, PluginError> {
        let target = match action {
            "next_page" => PageTarget::Step(1),
            "previous_page" => PageTarget::Step(-1),
            "go_to_page" => {
                let wanted = argument(args, "page");
                // "Page 3", as the choices offer an unnamed page.
                let numbered = wanted
                    .trim()
                    .strip_prefix("Page ")
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .and_then(|n| self.control().ok()?.pages().get(n.checked_sub(1)?).cloned())
                    .map(|(id, _)| id);
                PageTarget::Page(match numbered {
                    Some(id) => id,
                    None => self.find(wanted)?,
                })
            }
            other => {
                return Err(PluginError::UnknownAction {
                    plugin: DECK_PLUGIN.to_string(),
                    action: other.to_string(),
                })
            }
        };
        self.control()?.show(target)?;
        Ok(Outcome::default())
    }
}

/// A [`PageControl`] that writes down what it was asked. For tests.
#[derive(Debug, Default)]
pub struct RecordingPages {
    pub pages: Vec<(String, String)>,
    pub shown: Mutex<Vec<PageTarget>>,
}

impl PageControl for RecordingPages {
    fn show(&self, target: PageTarget) -> Result<(), PluginError> {
        self.shown.lock().unwrap_or_else(PoisonError::into_inner).push(target);
        Ok(())
    }

    fn pages(&self) -> Vec<(String, String)> {
        self.pages.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RecordingNotifier;
    use flexdeck_actions::RecordingExecutor;
    use flexdeck_vars::VariableStore;
    use std::collections::BTreeMap;

    fn run(deck: &mut Deck, action: &str, page: &str) -> Result<(), PluginError> {
        let vars = VariableStore::new();
        let mut input = RecordingExecutor::new();
        let mut notifier = RecordingNotifier::new();
        let settings = BTreeMap::new();
        let mut ctx = Ctx { input: &mut input, vars: &vars, notifier: &mut notifier, settings: &settings, now_ms: 0 };
        let args = HashMap::from([("page".to_string(), page.to_string())]);
        deck.invoke(action, &args, &mut ctx).map(drop)
    }

    #[test]
    fn pages_are_turned_by_step_by_name_by_id_and_by_number() {
        let control = Arc::new(RecordingPages {
            pages: vec![("home".into(), "Home".into()), ("p2".into(), "Stream".into()), ("p3".into(), String::new())],
            ..Default::default()
        });
        let mut deck = Deck::new(Some(control.clone() as Arc<dyn PageControl>));
        run(&mut deck, "next_page", "").unwrap();
        run(&mut deck, "previous_page", "").unwrap();
        run(&mut deck, "go_to_page", "stream").unwrap();
        run(&mut deck, "go_to_page", "home").unwrap();
        run(&mut deck, "go_to_page", "Page 3").unwrap();
        assert_eq!(
            *control.shown.lock().unwrap(),
            [
                PageTarget::Step(1),
                PageTarget::Step(-1),
                PageTarget::Page("p2".into()),
                PageTarget::Page("home".into()),
                PageTarget::Page("p3".into())
            ]
        );
        assert!(run(&mut deck, "go_to_page", "Nowhere").unwrap_err().to_string().contains("no page called 'Nowhere'"));

        let choices = deck.choices("go_to_page", &HashMap::new()).unwrap();
        let offered: Vec<_> = choices.arguments["page"].iter().map(|c| c.value.as_str()).collect();
        assert_eq!(offered, ["Home", "Stream", "Page 3"]);
    }

    #[test]
    fn without_a_deck_to_turn_it_says_so() {
        let mut deck = Deck::new(None);
        assert!(run(&mut deck, "next_page", "").is_err());
    }
}
