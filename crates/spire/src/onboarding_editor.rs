//! The terminal adapter for the push-based onboarding editor.
//!
//! All onboarding decisions are made by `spire_application::OnboardingModel`.
//! This module only translates key events into model mutations and renders the
//! current document. Linear calls are delivered through the discovery channels,
//! so a slow provider cannot hold a terminal frame hostage.

use std::{
    collections::BTreeSet,
    env,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Widget},
};
use serde::Serialize;
use spire_adapters::linear::LinearReadAdapter;
use spire_application::{
    DiscoveryRequest, DiscoveryResponse, Editable, LinearOnboardingDiscoveryPort, LinearStateKind,
    LinearTeamConfiguration, ModelCatalog, MutationOutcome, OnboardingDiscovery,
    OnboardingEditorPort, OnboardingEditorResult, OnboardingModel, OnboardingRole,
    OnboardingSection, SectionStatus, rank_states, suggest_complexity_mapping,
};
use spire_domain::{HarnessId, ModelId};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[cfg(test)]
use std::collections::BTreeMap;

#[cfg(test)]
use ratatui::{backend::TestBackend, buffer::Buffer};
#[cfg(test)]
use spire_application::HeadlessEvent;

pub const MIN_TERMINAL_COLUMNS: u16 = 80;
pub const MIN_TERMINAL_ROWS: u16 = 24;
pub const DEFAULT_CATALOG_FILE: &str = "model-catalog.yaml";

pub fn load_model_catalog(path: &Path) -> Result<ModelCatalog> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("unable to read model catalog {}", path.display()))?;
    let catalog: ModelCatalog = serde_yaml::from_str(&contents)
        .with_context(|| format!("model catalog {} is malformed", path.display()))?;
    if catalog.version.trim().is_empty() || catalog.providers.is_empty() {
        bail!(
            "model catalog {} must name a version and provider",
            path.display()
        )
    }
    for (provider, models) in &catalog.providers {
        if HarnessId::new(provider).is_err() || models.is_empty() {
            bail!(
                "model catalog {} has an invalid provider entry",
                path.display()
            )
        }
        for entry in models {
            ModelId::new(entry.id.clone()).map_err(|error| {
                anyhow::anyhow!(
                    "model catalog {} contains an invalid model for {}: {error}",
                    path.display(),
                    provider
                )
            })?;
            if !entry.efforts.contains(&entry.default_effort) {
                bail!(
                    "model catalog {} gives {} a default effort outside its own effort list",
                    path.display(),
                    entry.id
                )
            }
        }
    }
    Ok(catalog)
}

pub fn default_model_catalog_path(paths: &spire_application::ResolvedPaths) -> PathBuf {
    if let Some(path) = env::var_os("SPIRE_MODEL_CATALOG") {
        return PathBuf::from(path);
    }
    paths.config_root.join(DEFAULT_CATALOG_FILE)
}

/// Loads the operator override first. During development and tests the checked
/// in catalog is a useful packaged fallback; release assembly copies the same
/// file beside the installed configuration template.
pub fn load_default_model_catalog(
    paths: &spire_application::ResolvedPaths,
) -> Result<ModelCatalog> {
    let configured = default_model_catalog_path(paths);
    if configured.exists() {
        return load_model_catalog(&configured);
    }
    if env::var_os("SPIRE_MODEL_CATALOG").is_some() {
        return load_model_catalog(&configured);
    }
    if let Ok(executable) = env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let installed = directory.join(DEFAULT_CATALOG_FILE);
        if installed.exists() {
            return load_model_catalog(&installed);
        }
    }
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/model-catalog.yaml");
    load_model_catalog(&configured).or_else(|_| load_model_catalog(&bundled))
}

pub fn spawn_discovery(
    adapter: LinearReadAdapter,
) -> (
    UnboundedSender<DiscoveryRequest>,
    UnboundedReceiver<DiscoveryResponse>,
) {
    let (request_tx, mut request_rx) = unbounded_channel();
    let (response_tx, response_rx) = unbounded_channel();
    tokio::spawn(async move {
        while let Some(request) = request_rx.recv().await {
            match request {
                DiscoveryRequest::ListTeams => {
                    let response = adapter
                        .list_teams()
                        .await
                        .map_err(|error| error.to_string());
                    let _ = response_tx.send(DiscoveryResponse::Teams(response));
                }
                DiscoveryRequest::TeamConfiguration { team_id } => {
                    let response = adapter
                        .team_configuration(&team_id)
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| match result {
                            spire_application::ExternalResult::Confirmed(config) => Ok(config),
                            spire_application::ExternalResult::NotFound => {
                                Err("Linear no longer reports the selected team".to_owned())
                            }
                            spire_application::ExternalResult::Ambiguous { detail } => Err(detail),
                        });
                    let _ = response_tx.send(DiscoveryResponse::TeamConfiguration {
                        team_id,
                        result: response,
                    });
                }
            }
        }
    });
    (request_tx, response_rx)
}

pub fn append_write_trace(path: &Path, destination: &Path, backup: Option<&Path>) -> Result<()> {
    let mut trace = TraceWriter::open(path.to_owned())?;
    trace.record(TraceEvent {
        event: "configuration_write",
        section: Some(OnboardingSection::ReviewAndWrite.as_str()),
        field: None,
        value: None,
        suggested_default_replaced: false,
        invalidated: Vec::new(),
        destination: Some(destination.to_string_lossy().into_owned()),
        backup: backup.map(|path| path.to_string_lossy().into_owned()),
    })
}

#[derive(Debug, Serialize)]
struct TraceEvent<'a> {
    event: &'a str,
    section: Option<&'a str>,
    field: Option<&'a str>,
    value: Option<serde_json::Value>,
    suggested_default_replaced: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    invalidated: Vec<&'a str>,
    destination: Option<String>,
    backup: Option<String>,
}

struct TraceWriter {
    file: fs::File,
    path: PathBuf,
}

impl TraceWriter {
    fn open(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .context("onboarding trace must have a parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create trace directory {}", parent.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("unable to open onboarding trace {}", path.display()))?;
        Ok(Self { file, path })
    }

    fn record(&mut self, event: TraceEvent<'_>) -> Result<()> {
        serde_json::to_writer(&mut self.file, &event)?;
        self.file.write_all(b"\n")?;
        self.file.sync_data()?;
        Ok(())
    }

    fn mutation(
        &mut self,
        section: &str,
        field: &str,
        value: serde_json::Value,
        suggested: bool,
    ) -> Result<()> {
        self.record(TraceEvent {
            event: "model_mutation",
            section: Some(section),
            field: Some(field),
            value: Some(value),
            suggested_default_replaced: suggested,
            invalidated: Vec::new(),
            destination: None,
            backup: None,
        })
    }

    fn invalidation(&mut self, outcome: &MutationOutcome) -> Result<()> {
        if outcome.invalidated.is_empty() {
            return Ok(());
        }
        let names = outcome
            .invalidated
            .iter()
            .map(|section| section.as_str())
            .collect::<Vec<_>>();
        self.record(TraceEvent {
            event: "section_invalidation",
            section: None,
            field: Some(outcome.mutation.as_str()),
            value: None,
            suggested_default_replaced: false,
            invalidated: names,
            destination: None,
            backup: None,
        })
    }
}

pub struct TerminalOnboardingEditor {
    paths: spire_application::ResolvedPaths,
    catalog: ModelCatalog,
    request_tx: UnboundedSender<DiscoveryRequest>,
    response_rx: UnboundedReceiver<DiscoveryResponse>,
}

impl TerminalOnboardingEditor {
    pub fn new(
        paths: spire_application::ResolvedPaths,
        catalog: ModelCatalog,
        request_tx: UnboundedSender<DiscoveryRequest>,
        response_rx: UnboundedReceiver<DiscoveryResponse>,
    ) -> Self {
        Self {
            paths,
            catalog,
            request_tx,
            response_rx,
        }
    }

    fn edit_terminal(
        &mut self,
        model: OnboardingModel,
        discovery: OnboardingDiscovery,
    ) -> Result<OnboardingEditorResult> {
        validate_terminal()?;
        let mut guard = TerminalGuard::enter()?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        let trace_path = self.paths.state_root.join("onboarding-trace.jsonl");
        let mut session = EditorSession::new(model, discovery, self.catalog.clone(), trace_path)?;
        let _ = self.request_tx.send(DiscoveryRequest::ListTeams);
        let result = run_session(
            &mut terminal,
            &mut session,
            &self.request_tx,
            &mut self.response_rx,
        )?;
        guard.restore()?;
        Ok(result)
    }
}

impl OnboardingEditorPort for TerminalOnboardingEditor {
    type Error = anyhow::Error;

    fn edit(
        &mut self,
        model: OnboardingModel,
        discovery: OnboardingDiscovery,
    ) -> Result<OnboardingEditorResult, Self::Error> {
        self.edit_terminal(model, discovery)
    }
}

pub fn validate_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("spire init requires a terminal: stdin and stdout must be TTYs")
    }
    match env::var("TERM") {
        Ok(term) if !term.is_empty() && term != "dumb" => {}
        _ => bail!("spire init requires TERM to name a usable terminal (TERM=dumb is unsupported)"),
    }
    let (columns, rows) = terminal::size().context("unable to determine terminal size")?;
    if columns < MIN_TERMINAL_COLUMNS || rows < MIN_TERMINAL_ROWS {
        bail!(
            "spire init needs at least {MIN_TERMINAL_COLUMNS}x{MIN_TERMINAL_ROWS}; terminal is {columns}x{rows}"
        )
    }
    Ok(())
}

struct TerminalGuard {
    restored: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("unable to enable raw terminal mode")?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = terminal::disable_raw_mode();
            return Err(error.into());
        }
        install_panic_cleanup();
        Ok(Self { restored: false })
    }

    fn restore(&mut self) -> Result<()> {
        if !self.restored {
            terminal::disable_raw_mode().context("unable to restore terminal mode")?;
            execute!(io::stdout(), LeaveAlternateScreen)?;
            self.restored = true;
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn install_panic_cleanup() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        previous(panic);
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Section(OnboardingSection),
    QuitConfirmation,
}

struct EditorSession {
    model: OnboardingModel,
    discovery: OnboardingDiscovery,
    catalog: ModelCatalog,
    screen: Screen,
    home_index: usize,
    section_index: usize,
    multi_selected: BTreeSet<String>,
    error: Option<String>,
    trace: TraceWriter,
}

impl EditorSession {
    fn new(
        model: OnboardingModel,
        discovery: OnboardingDiscovery,
        catalog: ModelCatalog,
        trace_path: PathBuf,
    ) -> Result<Self> {
        let multi_selected = model
            .type_labels
            .value
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let mut trace = TraceWriter::open(trace_path)?;
        trace.mutation(
            OnboardingSection::Linear.as_str(),
            "credential_verified",
            serde_json::json!(model.credential_verified),
            false,
        )?;
        trace.mutation(
            OnboardingSection::Maker.as_str(),
            "catalog_version",
            serde_json::json!(catalog.version),
            false,
        )?;
        Ok(Self {
            model,
            discovery,
            catalog,
            screen: Screen::Home,
            home_index: 0,
            section_index: 0,
            multi_selected,
            error: None,
            trace,
        })
    }

    fn apply_response(&mut self, response: DiscoveryResponse) {
        match response {
            DiscoveryResponse::Teams(result) => match result {
                Ok(teams) => {
                    self.discovery.teams = teams;
                    self.error = None;
                }
                Err(error) => {
                    self.discovery
                        .failures
                        .insert("teams".to_owned(), error.clone());
                    self.error = Some(format!(
                        "Linear discovery failed: {error}; press r to retry"
                    ));
                }
            },
            DiscoveryResponse::TeamConfiguration { team_id, result } => match result {
                Ok(configuration) => {
                    self.discovery
                        .team_configurations
                        .insert(team_id.clone(), configuration.clone());
                    self.prefill_team(configuration);
                    self.discovery.failures.remove(&team_id);
                    self.error = None;
                }
                Err(error) => {
                    self.discovery.failures.insert(team_id, error.clone());
                    self.error = Some(format!(
                        "Linear team discovery failed: {error}; press r to retry"
                    ));
                }
            },
        }
    }

    fn prefill_team(&mut self, configuration: LinearTeamConfiguration) {
        for kind in LinearStateKind::ALL {
            let previous = self
                .model
                .workflow_states
                .get(&kind)
                .cloned()
                .unwrap_or_default();
            let retained_is_legal = previous
                .value
                .as_deref()
                .is_some_and(|value| configuration.states.iter().any(|state| state.id == value));
            if previous.is_stale() && retained_is_legal {
                self.model.workflow_states.insert(kind, previous);
            } else if let Some(index) = rank_states(kind, &configuration.states).first().copied() {
                self.model.workflow_states.insert(
                    kind,
                    Editable::complete(configuration.states[index].id.clone()),
                );
            }
        }
        let retain_complexity = self.model.complexity.is_stale();
        if retain_complexity {
            // Keep the old mapping visible and stale so reopening the section can
            // offer it for confirmation against the newly selected team.
        } else if let Ok(mapping) = suggest_complexity_mapping(&configuration.estimates) {
            self.model.complexity = Editable::complete(spire_application::ComplexitySelection {
                scale: configuration.estimates.clone(),
                mapping,
            });
        } else {
            self.model.complexity = Editable::default();
        }
        if !self.model.type_labels.is_stale() {
            self.model.type_labels =
                Editable::complete(self.model.type_labels.value.clone().unwrap_or_else(|| {
                    spire_application::DEFAULT_TYPE_LABELS
                        .iter()
                        .map(|label| (*label).to_owned())
                        .collect()
                }));
        }
        let _ = self.trace.record(TraceEvent {
            event: "derived_value",
            section: Some(OnboardingSection::Complexity.as_str()),
            field: Some("mapping"),
            value: self.model.complexity.value.as_ref().map(|complexity| {
                serde_json::json!({
                    "estimate_scale": complexity.scale,
                    "mapping": complexity.mapping,
                })
            }),
            suggested_default_replaced: false,
            invalidated: Vec::new(),
            destination: None,
            backup: None,
        });
    }

    fn select_team(&mut self, team_id: String, request_tx: &UnboundedSender<DiscoveryRequest>) {
        let outcome = self.model.set_team(team_id.clone());
        let _ = self.trace.mutation(
            OnboardingSection::Linear.as_str(),
            "team_id",
            serde_json::json!(team_id),
            false,
        );
        let _ = self.trace.invalidation(&outcome);
        if !self.discovery.team_configurations.contains_key(&team_id) {
            let _ = request_tx.send(DiscoveryRequest::TeamConfiguration { team_id });
            self.error = Some("Loading team configuration…".to_owned());
        } else if let Some(config) = self.discovery.team_configurations.get(&team_id).cloned() {
            self.prefill_team(config);
        }
    }

    fn section_items(&self, section: OnboardingSection) -> Vec<String> {
        match section {
            OnboardingSection::Linear => self
                .discovery
                .teams
                .iter()
                .map(|team| format!("{} ({})", team.name, team.key))
                .collect(),
            OnboardingSection::WorkflowStates => self
                .discovery
                .team_configurations
                .get(self.model.team_id.value.as_deref().unwrap_or_default())
                .map(|configuration| {
                    configuration
                        .states
                        .iter()
                        .map(|state| format!("{} [{:?}]", state.name, state.category))
                        .collect()
                })
                .unwrap_or_default(),
            OnboardingSection::Maker | OnboardingSection::Reviewer => {
                let role = if section == OnboardingSection::Maker {
                    OnboardingRole::Maker
                } else {
                    OnboardingRole::Reviewer
                };
                let provider = match role {
                    OnboardingRole::Maker => self.model.maker.value.as_ref().map(|v| &v.provider),
                    OnboardingRole::Reviewer => {
                        self.model.reviewer.value.as_ref().map(|v| &v.provider)
                    }
                };
                provider
                    .map(|provider| {
                        self.catalog
                            .models_for(provider)
                            .into_iter()
                            .map(|model| model.to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            }
            OnboardingSection::TypeLabels => spire_application::DEFAULT_TYPE_LABELS
                .iter()
                .map(|label| (*label).to_owned())
                .collect(),
            OnboardingSection::Rollout => self
                .discovery
                .teams
                .iter()
                .map(|team| format!("{} ({})", team.name, team.id))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        request_tx: &UnboundedSender<DiscoveryRequest>,
    ) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.screen = Screen::QuitConfirmation;
            return true;
        }
        match self.screen {
            Screen::Home => self.handle_home_key(key),
            Screen::QuitConfirmation => {
                if matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y')
                ) {
                    return true;
                }
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N')
                ) {
                    self.screen = Screen::Home;
                }
                false
            }
            Screen::Section(section) => self.handle_section_key(section, key, request_tx),
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Up => self.home_index = self.home_index.saturating_sub(1),
            KeyCode::Down => {
                self.home_index = (self.home_index + 1).min(OnboardingSection::ALL.len() - 1)
            }
            KeyCode::Enter => {
                self.section_index = 0;
                self.screen = Screen::Section(OnboardingSection::ALL[self.home_index]);
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.home_index = OnboardingSection::ReviewAndWrite as usize;
                self.screen = Screen::Section(OnboardingSection::ReviewAndWrite);
            }
            KeyCode::Char('q') | KeyCode::Esc => self.screen = Screen::QuitConfirmation,
            _ => {}
        }
        false
    }

    fn handle_section_key(
        &mut self,
        section: OnboardingSection,
        key: KeyEvent,
        request_tx: &UnboundedSender<DiscoveryRequest>,
    ) -> bool {
        if key.code == KeyCode::Esc {
            self.screen = Screen::Home;
            return false;
        }
        if section == OnboardingSection::Linear
            && matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R'))
        {
            let request = self
                .model
                .team_id
                .value
                .clone()
                .map(|team_id| DiscoveryRequest::TeamConfiguration { team_id })
                .unwrap_or(DiscoveryRequest::ListTeams);
            let _ = request_tx.send(request);
            return false;
        }
        if key.code == KeyCode::Char('r') || key.code == KeyCode::Char('R') {
            self.screen = Screen::Section(OnboardingSection::ReviewAndWrite);
            return false;
        }
        match section {
            OnboardingSection::Linear => {
                let items = self.section_items(section);
                match key.code {
                    KeyCode::Up => self.section_index = self.section_index.saturating_sub(1),
                    KeyCode::Down => {
                        self.section_index =
                            (self.section_index + 1).min(items.len().saturating_sub(1))
                    }
                    KeyCode::Enter if !items.is_empty() => {
                        let team_id = self.discovery.teams[self.section_index].id.clone();
                        self.select_team(team_id, request_tx);
                    }
                    KeyCode::Char('r') => {
                        let team_id = self.model.team_id.value.clone();
                        if let Some(team_id) = team_id {
                            let _ =
                                request_tx.send(DiscoveryRequest::TeamConfiguration { team_id });
                        } else {
                            let _ = request_tx.send(DiscoveryRequest::ListTeams);
                        }
                    }
                    _ => {}
                }
            }
            OnboardingSection::WorkflowStates => self.handle_workflow_key(key),
            OnboardingSection::Complexity => {
                if key.code == KeyCode::Enter {
                    if let Some(configuration) = self
                        .model
                        .team_id
                        .value
                        .as_deref()
                        .and_then(|team_id| self.discovery.team_configurations.get(team_id))
                        && let Ok(mapping) = suggest_complexity_mapping(&configuration.estimates)
                    {
                        self.model.complexity =
                            Editable::complete(spire_application::ComplexitySelection {
                                scale: configuration.estimates.clone(),
                                mapping,
                            });
                    }
                    self.model.confirm_section(section);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "mapping",
                        self.model
                            .complexity
                            .value
                            .as_ref()
                            .map(|complexity| {
                                serde_json::json!({
                                    "estimate_scale": complexity.scale,
                                    "mapping": complexity.mapping,
                                })
                            })
                            .unwrap_or(serde_json::Value::Null),
                        true,
                    );
                }
            }
            OnboardingSection::Maker | OnboardingSection::Reviewer => {
                self.handle_harness_key(section, key)
            }
            OnboardingSection::TypeLabels | OnboardingSection::Rollout => {
                self.handle_multi_key(section, key)
            }
            OnboardingSection::Paths => {}
            OnboardingSection::ReviewAndWrite => {
                if key.code == KeyCode::Enter {
                    match self.model.validate() {
                        Ok(()) => return true,
                        Err(error) => self.error = Some(format!("write refused: {error}")),
                    }
                }
            }
        }
        false
    }

    fn handle_workflow_key(&mut self, key: KeyEvent) {
        let Some(configuration) = self
            .model
            .team_id
            .value
            .as_deref()
            .and_then(|team_id| self.discovery.team_configurations.get(team_id))
        else {
            return;
        };
        match key.code {
            KeyCode::Up => self.section_index = self.section_index.saturating_sub(1),
            KeyCode::Down => {
                self.section_index = (self.section_index + 1).min(LinearStateKind::ALL.len() - 1)
            }
            KeyCode::Enter => {
                let kind = LinearStateKind::ALL[self.section_index];
                let candidates = rank_states(kind, &configuration.states);
                let current = self
                    .model
                    .workflow_states
                    .get(&kind)
                    .and_then(|state| state.value.as_deref());
                let candidate_index = current
                    .and_then(|value| {
                        candidates
                            .iter()
                            .position(|index| configuration.states[*index].id == value)
                    })
                    .map(|index| (index + 1) % candidates.len().max(1))
                    .unwrap_or(0);
                if let Some(state_index) = candidates.get(candidate_index)
                    && let Some(state) = configuration.states.get(*state_index)
                {
                    self.model
                        .workflow_states
                        .insert(kind, Editable::complete(state.id.clone()));
                    self.model
                        .confirm_section(OnboardingSection::WorkflowStates);
                    let _ = self.trace.mutation(
                        OnboardingSection::WorkflowStates.as_str(),
                        kind.as_str(),
                        serde_json::json!(state.id),
                        false,
                    );
                }
            }
            // Space is deliberately ignored for this single-select screen.
            KeyCode::Char(' ') => {}
            _ => {}
        }
    }

    fn handle_harness_key(&mut self, section: OnboardingSection, key: KeyEvent) {
        let role = if section == OnboardingSection::Maker {
            OnboardingRole::Maker
        } else {
            OnboardingRole::Reviewer
        };
        let selection = match role {
            OnboardingRole::Maker => self.model.maker.value.clone(),
            OnboardingRole::Reviewer => self.model.reviewer.value.clone(),
        };
        let Some(mut selection) = selection else {
            return;
        };
        match key.code {
            KeyCode::Up => {
                self.section_index = self.section_index.saturating_sub(1);
            }
            KeyCode::Down => {
                self.section_index = (self.section_index + 1).min(2);
            }
            KeyCode::Char('o') | KeyCode::Char('O') if self.section_index == 1 => {
                match prompt_off_catalog_model() {
                    Ok(model) => {
                        self.model.set_model_catalog_state(role, true);
                        let chosen = model.to_string();
                        let outcome = self.model.set_model(role, model, &self.catalog);
                        let _ = self.trace.invalidation(&outcome);
                        let _ = self.trace.mutation(
                            section.as_str(),
                            "model",
                            serde_json::json!({
                                "model": chosen,
                                "catalog_resolution": "off_catalog",
                            }),
                            false,
                        );
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            KeyCode::Enter if self.section_index == 0 => {
                let providers = ["codex", "claude-code"];
                let current = providers
                    .iter()
                    .position(|p| *p == selection.provider.as_str())
                    .unwrap_or(0);
                let next = (current + 1) % providers.len();
                if let Ok(provider) = HarnessId::new(providers[next]) {
                    selection.provider = provider;
                    selection.model = ModelId::new("unselected").expect("literal model is valid");
                    if role == OnboardingRole::Maker {
                        let outcome = self.model.set_maker_provider(selection.provider.clone());
                        let _ = self.trace.invalidation(&outcome);
                    } else {
                        let outcome = self.model.set_reviewer_provider(selection.provider.clone());
                        let _ = self.trace.invalidation(&outcome);
                    }
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "provider",
                        serde_json::json!(selection.provider.as_str()),
                        false,
                    );
                }
            }
            KeyCode::Enter if self.section_index == 1 => {
                if let Some(model) = self
                    .catalog
                    .next_model(&selection.provider, &selection.model)
                {
                    self.model.set_model_catalog_state(role, false);
                    let chosen = model.to_string();
                    let outcome = self.model.set_model(role, model, &self.catalog);
                    let effort_reset = !outcome.invalidated.is_empty();
                    let _ = self.trace.invalidation(&outcome);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "model",
                        serde_json::json!({
                            "model": chosen,
                            "effort_reset_to_model_default": effort_reset,
                        }),
                        true,
                    );
                }
            }
            KeyCode::Enter if self.section_index == 2 => {
                selection.effort = self.catalog.next_effort(
                    &selection.provider,
                    &selection.model,
                    selection.effort,
                );
                self.model.set_effort(role, selection.effort);
                let _ = self.trace.mutation(
                    section.as_str(),
                    "effort",
                    serde_json::json!(format!("{:?}", selection.effort).to_ascii_lowercase()),
                    false,
                );
            }
            // Space has no meaning in provider/model/effort lists.
            KeyCode::Char(' ') => {}
            _ => {}
        }
    }

    fn handle_multi_key(&mut self, section: OnboardingSection, key: KeyEvent) {
        let items = self.section_items(section);
        match key.code {
            KeyCode::Up => self.section_index = self.section_index.saturating_sub(1),
            KeyCode::Down => {
                self.section_index = (self.section_index + 1).min(items.len().saturating_sub(1))
            }
            KeyCode::Char(' ') if !items.is_empty() => {
                let item = if section == OnboardingSection::TypeLabels {
                    items[self.section_index].clone()
                } else {
                    self.discovery.teams[self.section_index].id.clone()
                };
                if !self.multi_selected.insert(item.clone()) {
                    self.multi_selected.remove(&item);
                }
            }
            KeyCode::Enter => {
                let selected = self.multi_selected.iter().cloned().collect::<Vec<_>>();
                if section == OnboardingSection::TypeLabels {
                    self.model.type_labels = Editable::complete(selected);
                    self.model.confirm_section(section);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "labels",
                        serde_json::json!(self.model.type_labels.value),
                        false,
                    );
                } else {
                    self.model.rollout_allowed_team_ids = Editable::complete(selected);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "allowed_team_ids",
                        serde_json::json!(self.model.rollout_allowed_team_ids.value),
                        false,
                    );
                }
            }
            _ => {}
        }
    }
}

fn prompt_off_catalog_model() -> Result<ModelId> {
    terminal::disable_raw_mode().context("unable to leave raw mode for model entry")?;
    execute!(io::stdout(), LeaveAlternateScreen)
        .context("unable to leave the alternate screen for model entry")?;
    let input_result = (|| -> Result<String> {
        print!("\nModel ID outside the catalog (recorded as unverified): ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(input)
    })();
    let enter_result = execute!(io::stdout(), EnterAlternateScreen)
        .context("unable to restore the alternate screen after model entry");
    let raw_result =
        terminal::enable_raw_mode().context("unable to restore raw mode after model entry");
    let input = input_result?;
    enter_result?;
    raw_result?;
    ModelId::new(input.trim()).map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn run_session<B: Backend>(
    terminal: &mut Terminal<B>,
    session: &mut EditorSession,
    request_tx: &UnboundedSender<DiscoveryRequest>,
    response_rx: &mut UnboundedReceiver<DiscoveryResponse>,
) -> Result<OnboardingEditorResult> {
    loop {
        while let Ok(response) = response_rx.try_recv() {
            session.apply_response(response);
        }
        terminal.draw(|frame| render_session(frame, session))?;
        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && session.handle_key(key, request_tx)
        {
            if matches!(session.screen, Screen::QuitConfirmation) {
                return Ok(OnboardingEditorResult::Cancelled);
            }
            session.record_committed_values()?;
            return Ok(OnboardingEditorResult::Committed(Box::new(
                session.model.clone(),
            )));
        }
    }
}

fn render_session(frame: &mut ratatui::Frame<'_>, session: &EditorSession) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(area);
    match session.screen {
        Screen::Home => frame.render_widget(home_widget(session, chunks[0]), chunks[0]),
        Screen::Section(section) => {
            frame.render_widget(section_widget(session, section, chunks[0]), chunks[0])
        }
        Screen::QuitConfirmation => frame.render_widget(quit_widget(chunks[0]), chunks[0]),
    }
    let footer = if let Some(error) = &session.error {
        Line::from(Span::styled(
            error.clone(),
            Style::default().fg(Color::Yellow),
        ))
    } else {
        footer_for(session.screen)
    };
    frame.render_widget(
        Paragraph::new(footer).block(Block::default().borders(Borders::TOP)),
        chunks[1],
    );
}

fn home_widget<'a>(session: &'a EditorSession, _area: Rect) -> impl Widget + 'a {
    let statuses = session.model.statuses();
    let items = OnboardingSection::ALL
        .iter()
        .map(|section| {
            let status = statuses.get(section);
            let (marker, style) = match status {
                Some(SectionStatus::Complete) => ("✓", Style::default().fg(Color::Green)),
                Some(SectionStatus::Incomplete { .. }) | None => {
                    ("○", Style::default().fg(Color::DarkGray))
                }
                Some(SectionStatus::Stale { .. }) => (
                    "!",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            };
            let item_style = if *section as usize == session.home_index {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("  {}", section.as_str()), item_style),
            ]))
        })
        .collect::<Vec<_>>();
    List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Spire onboarding"),
    )
}

fn section_widget<'a>(
    session: &'a EditorSession,
    section: OnboardingSection,
    _area: Rect,
) -> impl Widget + 'a {
    let mut lines = vec![Line::from(Span::styled(
        section.as_str().to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    match section {
        OnboardingSection::Linear => {
            lines.push(Line::from(format!(
                "credential: {}",
                if session.model.credential_verified {
                    "verified"
                } else {
                    "missing"
                }
            )));
            lines.push(Line::from("Select a team:"));
            for (index, item) in session.section_items(section).iter().enumerate() {
                lines.push(Line::from(format!(
                    "{} {}",
                    if index == session.section_index {
                        ">"
                    } else {
                        " "
                    },
                    item
                )));
            }
        }
        OnboardingSection::WorkflowStates => {
            for kind in LinearStateKind::ALL {
                let value = session
                    .model
                    .workflow_states
                    .get(&kind)
                    .and_then(|state| state.value.as_deref())
                    .unwrap_or("unbound");
                lines.push(Line::from(format!(
                    "{} {}: {}",
                    if kind as usize == session.section_index {
                        ">"
                    } else {
                        " "
                    },
                    kind.as_str(),
                    value
                )));
            }
        }
        OnboardingSection::Complexity => {
            if let Some(complexity) = session.model.complexity.value.as_ref() {
                lines.push(Line::from(format!(
                    "scale: {} ({:?})",
                    complexity.scale.kind, complexity.scale.points
                )));
                for (estimate, class) in &complexity.mapping {
                    lines.push(Line::from(format!("  {} -> {class:?}", estimate.value())));
                }
            } else {
                lines.push(Line::from("waiting for a usable estimate scale"));
            }
        }
        OnboardingSection::Maker | OnboardingSection::Reviewer => {
            let selection = if section == OnboardingSection::Maker {
                session.model.maker.value.as_ref()
            } else {
                session.model.reviewer.value.as_ref()
            };
            if let Some(selection) = selection {
                lines.push(Line::from(format!("provider: {}", selection.provider)));
                lines.push(Line::from(format!("model: {}", selection.model)));
                lines.push(Line::from(format!("effort: {:?}", selection.effort)));
                if (section == OnboardingSection::Maker
                    && session
                        .model
                        .off_catalog_roles
                        .contains(&OnboardingRole::Maker))
                    || (section == OnboardingSection::Reviewer
                        && session
                            .model
                            .off_catalog_roles
                            .contains(&OnboardingRole::Reviewer))
                {
                    lines.push(Line::from(Span::styled(
                        "model is explicitly off-catalog / unverified",
                        Style::default().fg(Color::Yellow),
                    )));
                }
            } else {
                lines.push(Line::from("choose provider, model, and effort"));
            }
        }
        OnboardingSection::TypeLabels | OnboardingSection::Rollout => {
            lines.push(Line::from("Space toggles membership; Enter confirms."));
            for (index, item) in session.section_items(section).iter().enumerate() {
                let selected =
                    session
                        .multi_selected
                        .contains(if section == OnboardingSection::TypeLabels {
                            item
                        } else {
                            &session.discovery.teams[index].id
                        });
                lines.push(Line::from(format!(
                    "{} [{}] {}",
                    if index == session.section_index {
                        ">"
                    } else {
                        " "
                    },
                    if selected { "x" } else { " " },
                    item
                )));
            }
        }
        OnboardingSection::Paths => {
            lines.push(Line::from(format!(
                "configuration: {}",
                session.paths_display()
            )));
        }
        OnboardingSection::ReviewAndWrite => {
            for (section, status) in session.model.statuses() {
                lines.push(Line::from(format!(
                    "{}: {}",
                    section.as_str(),
                    status_summary(&status)
                )));
            }
            for role in &session.model.off_catalog_roles {
                lines.push(Line::from(Span::styled(
                    format!("{role:?} model: explicitly off-catalog / unverified"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            lines.push(Line::from(
                "Enter writes only when every blocking section is complete.",
            ));
        }
    }
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(section.as_str().to_string()),
    )
}

impl EditorSession {
    fn paths_display(&self) -> String {
        self.trace.path.display().to_string()
    }

    fn record_committed_values(&mut self) -> Result<()> {
        let record =
            |trace: &mut TraceWriter,
             section: OnboardingSection,
             field: &str,
             value: serde_json::Value|
             -> Result<()> { trace.mutation(section.as_str(), field, value, false) };

        record(
            &mut self.trace,
            OnboardingSection::Linear,
            "organization_id",
            serde_json::json!(self.model.organization_id.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Linear,
            "bot_actor_id",
            serde_json::json!(self.model.bot_actor_id.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Linear,
            "team_id",
            serde_json::json!(self.model.team_id.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::WorkflowStates,
            "state_ids",
            serde_json::json!(self.model.workflow_states),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Complexity,
            "mapping",
            serde_json::json!(self.model.complexity.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Maker,
            "selection",
            serde_json::json!(self.model.maker.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Reviewer,
            "selection",
            serde_json::json!(self.model.reviewer.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::TypeLabels,
            "labels",
            serde_json::json!(self.model.type_labels.value),
        )?;
        record(
            &mut self.trace,
            OnboardingSection::Rollout,
            "allowed_team_ids",
            serde_json::json!(self.model.rollout_allowed_team_ids.value),
        )?;
        Ok(())
    }
}

fn quit_widget(_area: Rect) -> impl Widget {
    Paragraph::new("Abandon this onboarding session? Nothing will be written. [y/N]")
        .block(Block::default().borders(Borders::ALL).title("Quit"))
}

fn footer_for(screen: Screen) -> Line<'static> {
    match screen {
        Screen::Home => Line::from("↑/↓ move  Enter open  r review  q/Esc quit"),
        Screen::QuitConfirmation => Line::from("Enter/y abandon  Esc/n return"),
        Screen::Section(OnboardingSection::TypeLabels | OnboardingSection::Rollout) => {
            Line::from("↑/↓ move  Space toggle  Enter confirm  Esc back  r review")
        }
        Screen::Section(OnboardingSection::Paths) => Line::from("Esc back  r review"),
        Screen::Section(_) => Line::from("↑/↓ move  Enter confirm  Esc back  r review"),
    }
}

fn status_summary(status: &SectionStatus) -> String {
    match status {
        SectionStatus::Complete => "complete".to_owned(),
        SectionStatus::Incomplete { reasons } => reasons.join(", "),
        SectionStatus::Stale { reason } => format!("stale: {reason}"),
    }
}

/// TestBackend entry point: it exercises rendering and key handling without a
/// terminal or network. The application headless adapter remains the smaller
/// pure contract test; this covers the actual terminal renderer's buffer.
#[cfg(test)]
pub fn run_test_backend(
    model: OnboardingModel,
    events: impl IntoIterator<Item = KeyEvent>,
) -> Result<(OnboardingEditorResult, Buffer)> {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend)?;
    let (_response_tx, mut response_rx) = unbounded_channel::<DiscoveryResponse>();
    let mut session = EditorSession::new(
        model,
        OnboardingDiscovery::default(),
        ModelCatalog {
            version: "test".to_owned(),
            providers: BTreeMap::new(),
        },
        env::temp_dir().join(format!(
            "spire-onboarding-test-{}.jsonl",
            std::process::id()
        )),
    )?;
    let (request_tx, _request_rx) = unbounded_channel();
    for key in events {
        while let Ok(response) = response_rx.try_recv() {
            session.apply_response(response);
        }
        terminal.draw(|frame| render_session(frame, &session))?;
        if session.handle_key(key, &request_tx) {
            let result = if matches!(session.screen, Screen::QuitConfirmation) {
                OnboardingEditorResult::Cancelled
            } else {
                session.record_committed_values()?;
                OnboardingEditorResult::Committed(Box::new(session.model.clone()))
            };
            let buffer = terminal.backend().buffer().clone();
            return Ok((result, buffer));
        }
    }
    terminal.draw(|frame| render_session(frame, &session))?;
    Ok((
        OnboardingEditorResult::Cancelled,
        terminal.backend().buffer().clone(),
    ))
}

#[cfg(test)]
pub fn make_test_events(events: &[HeadlessEvent]) -> Vec<KeyEvent> {
    events
        .iter()
        .map(|event| match event {
            HeadlessEvent::Commit => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            HeadlessEvent::Cancel => KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            _ => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_renders_without_the_credential_sentinel() {
        let sentinel = "linear-secret-sentinel";
        let (result, buffer) = run_test_backend(
            OnboardingModel::empty(),
            [KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)],
        )
        .unwrap();
        assert!(matches!(result, OnboardingEditorResult::Cancelled));
        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!rendered.contains(sentinel));
    }

    #[test]
    fn single_select_space_does_not_confirm_or_advance() {
        let events = [
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        ];
        let (result, _) = run_test_backend(OnboardingModel::empty(), events).unwrap();
        assert!(matches!(result, OnboardingEditorResult::Cancelled));
    }

    #[test]
    fn control_c_abandons_instead_of_committing() {
        let (result, _) = run_test_backend(
            OnboardingModel::empty(),
            [KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)],
        )
        .unwrap();
        assert!(matches!(result, OnboardingEditorResult::Cancelled));
    }

    #[test]
    fn scripted_headless_events_have_a_terminal_key_mapping() {
        let keys = make_test_events(&[HeadlessEvent::Commit, HeadlessEvent::Cancel]);
        assert_eq!(keys[0].code, KeyCode::Enter);
        assert_eq!(keys[1].code, KeyCode::Char('q'));
    }
}
