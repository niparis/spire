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
use spire_domain::{ComplexityClass, HarnessId, ModelId};
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
    // One set per multi-select section: a shared set would let confirmed type
    // labels be written out as allowed rollout team IDs.
    selected_type_labels: BTreeSet<String>,
    selected_rollout_teams: BTreeSet<String>,
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
        let selected_type_labels = model
            .type_labels
            .value
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let selected_rollout_teams = model
            .rollout_allowed_team_ids
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
            selected_type_labels,
            selected_rollout_teams,
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
                .map(|team| format!("{} ({})", team.name, team.key))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn team_configuration(&self) -> Option<&LinearTeamConfiguration> {
        self.model
            .team_id
            .value
            .as_deref()
            .and_then(|team_id| self.discovery.team_configurations.get(team_id))
    }

    fn team_label(&self, team_id: &str) -> String {
        self.discovery
            .teams
            .iter()
            .find(|team| team.id == team_id)
            .map(|team| format!("{} ({})", team.name, team.key))
            .unwrap_or_else(|| team_id.to_owned())
    }

    /// Linear identifies a state by an opaque UUID, but an operator recognises it
    /// by name; the ID is only useful when the name cannot be resolved.
    fn state_label(&self, state_id: &str) -> String {
        self.team_configuration()
            .and_then(|configuration| {
                configuration
                    .states
                    .iter()
                    .find(|state| state.id == state_id)
            })
            .map(|state| format!("{} [{:?}]", state.name, state.category))
            .unwrap_or_else(|| format!("{state_id} (unknown to this team)"))
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
                let section = OnboardingSection::ALL[self.home_index];
                if section == OnboardingSection::Complexity {
                    self.seed_complexity_suggestion();
                }
                self.screen = Screen::Section(section);
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
        // A section marked stale by an upstream change may already hold the right
        // answer. Without this the only way to clear the mark is to alter a value
        // and alter it back.
        if key.code == KeyCode::Char('a') || key.code == KeyCode::Char('A') {
            self.model.confirm_section(section);
            let _ =
                self.trace
                    .mutation(section.as_str(), "accepted", serde_json::json!(true), true);
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
            OnboardingSection::Complexity => self.handle_complexity_key(key),
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

    /// The mapping is derived from the team's estimate scale so the operator has
    /// something to adjust rather than something to author.
    fn seed_complexity_suggestion(&mut self) {
        if self.model.complexity.value.is_some() {
            return;
        }
        if let Some(configuration) = self.team_configuration()
            && let Ok(mapping) = suggest_complexity_mapping(&configuration.estimates)
        {
            self.model.complexity = Editable::complete(spire_application::ComplexitySelection {
                scale: configuration.estimates.clone(),
                mapping,
            });
        }
    }

    fn handle_complexity_key(&mut self, key: KeyEvent) {
        self.seed_complexity_suggestion();
        let Some(complexity) = self.model.complexity.value.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Up => self.section_index = self.section_index.saturating_sub(1),
            KeyCode::Down => {
                self.section_index =
                    (self.section_index + 1).min(complexity.mapping.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                let Some((estimate, class)) = complexity
                    .mapping
                    .iter()
                    .nth(self.section_index)
                    .map(|(estimate, class)| (*estimate, *class))
                else {
                    return;
                };
                let next = ComplexityClass::ALL[(ComplexityClass::ALL
                    .iter()
                    .position(|candidate| *candidate == class)
                    .unwrap_or(0)
                    + 1)
                    % ComplexityClass::ALL.len()];
                complexity.mapping.insert(estimate, next);
                self.model.confirm_section(OnboardingSection::Complexity);
                let _ = self.trace.mutation(
                    OnboardingSection::Complexity.as_str(),
                    "mapping",
                    serde_json::json!({ "estimate": estimate.value(), "class": format!("{next:?}") }),
                    false,
                );
            }
            _ => {}
        }
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
                // Suggestions lead, but every state stays reachable: a workspace
                // whose names Spire does not recognise would otherwise offer the
                // operator nothing to cycle through.
                let mut candidates = rank_states(kind, &configuration.states);
                let unranked = (0..configuration.states.len())
                    .filter(|index| !candidates.contains(index))
                    .collect::<Vec<_>>();
                candidates.extend(unranked);
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
                    serde_json::json!(selection.effort.as_str()),
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
                let (item, chosen) = if section == OnboardingSection::TypeLabels {
                    (
                        items[self.section_index].clone(),
                        &mut self.selected_type_labels,
                    )
                } else {
                    (
                        self.discovery.teams[self.section_index].id.clone(),
                        &mut self.selected_rollout_teams,
                    )
                };
                if !chosen.insert(item.clone()) {
                    chosen.remove(&item);
                }
            }
            KeyCode::Enter => {
                if section == OnboardingSection::TypeLabels {
                    self.model.type_labels =
                        Editable::complete(self.selected_type_labels.iter().cloned().collect());
                    self.model.confirm_section(section);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "labels",
                        serde_json::json!(self.model.type_labels.value),
                        false,
                    );
                } else {
                    self.model.rollout_allowed_team_ids =
                        Editable::complete(self.selected_rollout_teams.iter().cloned().collect());
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

/// What the section decides and how it is driven. Shown in the section itself
/// because a section name alone does not say what the value is used for.
fn section_help(section: OnboardingSection) -> &'static str {
    match section {
        OnboardingSection::Linear => {
            "The Linear team Spire watches for tickets. Enter selects; r refetches."
        }
        OnboardingSection::WorkflowStates => {
            "Which of this team's states Spire reads and writes as it moves a ticket. Enter cycles the highlighted role through every state."
        }
        OnboardingSection::Complexity => {
            "Maps each Linear estimate point to the complexity class that picks a harness. Enter cycles the highlighted point; a accepts the suggestion."
        }
        OnboardingSection::Maker => {
            "The harness that writes the implementation. Its provider must differ from the reviewer's."
        }
        OnboardingSection::Reviewer => {
            "The harness that reviews the implementation, on a different provider than the maker."
        }
        OnboardingSection::TypeLabels => {
            "Ticket labels Spire treats as work types. Space toggles; Enter stores the selection."
        }
        OnboardingSection::Rollout => {
            "Teams allowed to trigger automation. Everything stays inert until a team is listed here. Space toggles; Enter stores the selection."
        }
        OnboardingSection::Paths => "Where the configuration file will be written.",
        OnboardingSection::ReviewAndWrite => {
            "Every section's state. Enter writes the configuration; a refusal names the section."
        }
    }
}

fn section_widget<'a>(
    session: &'a EditorSession,
    section: OnboardingSection,
    _area: Rect,
) -> impl Widget + 'a {
    let mut lines = vec![
        Line::from(Span::styled(
            section.as_str().to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            section_help(section),
            Style::default().fg(Color::DarkGray),
        )),
    ];
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
                    .map(|state_id| session.state_label(state_id))
                    .unwrap_or_else(|| "unbound".to_owned());
                lines.push(Line::from(format!(
                    "{} {:<14} {}",
                    if kind as usize == session.section_index {
                        ">"
                    } else {
                        " "
                    },
                    format!("{}:", kind.as_str()),
                    value
                )));
            }
        }
        OnboardingSection::Complexity => {
            if let Some(complexity) = session.model.complexity.value.as_ref() {
                lines.push(Line::from(format!(
                    "estimate scale: {} ({:?})",
                    complexity.scale.kind, complexity.scale.points
                )));
                for (index, (estimate, class)) in complexity.mapping.iter().enumerate() {
                    lines.push(Line::from(format!(
                        "{} {} -> {class:?}",
                        if index == session.section_index {
                            ">"
                        } else {
                            " "
                        },
                        estimate.value()
                    )));
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
                let cursor = |row: usize| {
                    if row == session.section_index {
                        ">"
                    } else {
                        " "
                    }
                };
                lines.push(Line::from(format!(
                    "{} provider: {}",
                    cursor(0),
                    selection.provider
                )));
                lines.push(Line::from(format!(
                    "{} model:    {}",
                    cursor(1),
                    selection.model
                )));
                // The accepted levels are shown because they are a property of
                // the model: changing the model above changes this list.
                let efforts = session
                    .catalog
                    .efforts_for(&selection.provider, &selection.model)
                    .iter()
                    .map(|effort| {
                        if *effort == selection.effort {
                            format!("[{effort}]")
                        } else {
                            effort.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                lines.push(Line::from(format!("{} effort:   {}", cursor(2), efforts)));
                lines.push(Line::from(
                    "enter cycles the selected row; o types a model outside the catalog",
                ));
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
            let type_labels = section == OnboardingSection::TypeLabels;
            let chosen = if type_labels {
                &session.selected_type_labels
            } else {
                &session.selected_rollout_teams
            };
            for (index, item) in session.section_items(section).iter().enumerate() {
                let selected = chosen.contains(if type_labels {
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
            // Toggling only stages a choice; showing what Enter actually stored
            // is the difference between a confirmed section and an untouched one.
            let stored = if type_labels {
                session.model.type_labels.value.clone()
            } else {
                session
                    .model
                    .rollout_allowed_team_ids
                    .value
                    .as_ref()
                    .map(|ids| ids.iter().map(|id| session.team_label(id)).collect())
            };
            lines.push(Line::from(match stored {
                Some(values) if !values.is_empty() => format!("stored: {}", values.join(", ")),
                Some(_) => "stored: none".to_owned(),
                None => "stored: nothing yet - press Enter".to_owned(),
            }));
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
    run_test_backend_with_catalog(
        model,
        ModelCatalog {
            version: "test".to_owned(),
            providers: BTreeMap::new(),
        },
        events,
    )
}

#[cfg(test)]
pub fn run_test_backend_with_catalog(
    model: OnboardingModel,
    catalog: ModelCatalog,
    events: impl IntoIterator<Item = KeyEvent>,
) -> Result<(OnboardingEditorResult, Buffer)> {
    run_test_backend_with_discovery(model, catalog, OnboardingDiscovery::default(), events)
}

#[cfg(test)]
pub fn run_test_backend_with_discovery(
    model: OnboardingModel,
    catalog: ModelCatalog,
    discovery: OnboardingDiscovery,
    events: impl IntoIterator<Item = KeyEvent>,
) -> Result<(OnboardingEditorResult, Buffer)> {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend)?;
    let (_response_tx, mut response_rx) = unbounded_channel::<DiscoveryResponse>();
    let mut session = EditorSession::new(
        model,
        discovery,
        catalog,
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

    fn rendered(buffer: &Buffer) -> String {
        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn the_maker_section_shows_a_cursor_and_only_the_selected_model_s_efforts() {
        let catalog = ModelCatalog {
            version: "test".to_owned(),
            providers: BTreeMap::from([(
                "codex".to_owned(),
                vec![
                    spire_application::CatalogModel {
                        id: "wide-model".to_owned(),
                        default_effort: spire_domain::Effort::Medium,
                        efforts: vec![spire_domain::Effort::Medium, spire_domain::Effort::Ultra],
                    },
                    spire_application::CatalogModel {
                        id: "narrow-model".to_owned(),
                        default_effort: spire_domain::Effort::Low,
                        efforts: vec![spire_domain::Effort::Low],
                    },
                ],
            )]),
        };
        let mut model = OnboardingModel::empty();
        model.maker.value = Some(spire_application::HarnessSelection {
            provider: HarnessId::new("codex").unwrap(),
            model: ModelId::new("wide-model").unwrap(),
            effort: spire_domain::Effort::Ultra,
        });

        // Home -> Maker, then move the cursor onto the model row.
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let open_maker = [
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Down),
        ];

        let (_, buffer) =
            run_test_backend_with_catalog(model.clone(), catalog.clone(), open_maker).unwrap();
        let screen = rendered(&buffer);
        assert!(screen.contains("model:"), "maker rows render: {screen}");
        assert!(
            screen.contains("[ultra]"),
            "the selected effort is marked among the model's own levels: {screen}"
        );

        // Cycling the model onto the narrower entry must drop ultra from the
        // offered levels rather than carry it to a model that rejects it.
        let cycle_model = open_maker.into_iter().chain([key(KeyCode::Enter)]);
        let (_, buffer) = run_test_backend_with_catalog(model, catalog, cycle_model).unwrap();
        let screen = rendered(&buffer);
        assert!(screen.contains("narrow-model"), "model cycled: {screen}");
        assert!(
            !screen.contains("ultra"),
            "ultra is not offered for the narrower model: {screen}"
        );
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn empty_catalog() -> ModelCatalog {
        ModelCatalog {
            version: "test".to_owned(),
            providers: BTreeMap::new(),
        }
    }

    /// One name Spire recognises and one it does not, so a cycle that only walked
    /// the ranked suggestions would visibly stall.
    fn discovery_fixture() -> (OnboardingDiscovery, OnboardingModel) {
        let team = spire_application::LinearTeamSummary {
            id: "team-uuid".to_owned(),
            key: "ENG".to_owned(),
            name: "Engineering".to_owned(),
        };
        let configuration = LinearTeamConfiguration {
            team: team.clone(),
            states: vec![
                spire_application::LinearWorkflowState {
                    id: "state-ready".to_owned(),
                    name: "Ready".to_owned(),
                    category: spire_application::LinearStateCategory::Unstarted,
                },
                spire_application::LinearWorkflowState {
                    id: "state-bespoke".to_owned(),
                    name: "Awaiting hardware".to_owned(),
                    category: spire_application::LinearStateCategory::Unstarted,
                },
            ],
            estimates: spire_application::LinearEstimateScale {
                kind: "fibonacci".to_owned(),
                points: vec![1, 2, 3, 5],
            },
        };
        let discovery = OnboardingDiscovery {
            teams: vec![team],
            team_configurations: BTreeMap::from([("team-uuid".to_owned(), configuration)]),
            failures: BTreeMap::new(),
        };
        let mut model = OnboardingModel::empty();
        model.team_id = Editable::complete("team-uuid".to_owned());
        model.workflow_states.insert(
            LinearStateKind::Ready,
            Editable::complete("state-ready".to_owned()),
        );
        (discovery, model)
    }

    #[test]
    fn workflow_states_render_names_and_cycle_past_the_suggestions() {
        let (discovery, model) = discovery_fixture();
        let open = [key(KeyCode::Down), key(KeyCode::Enter)];

        let (_, buffer) = run_test_backend_with_discovery(
            model.clone(),
            empty_catalog(),
            discovery.clone(),
            open,
        )
        .unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("Ready") && !screen.contains("state-ready"),
            "the operator sees the state name, not its opaque ID: {screen}"
        );

        let cycle = open.into_iter().chain([key(KeyCode::Enter)]);
        let (_, buffer) =
            run_test_backend_with_discovery(model, empty_catalog(), discovery, cycle).unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("Awaiting hardware"),
            "a state Spire cannot name-match is still reachable: {screen}"
        );
    }

    #[test]
    fn complexity_classes_are_editable_per_estimate_point() {
        let (discovery, model) = discovery_fixture();
        // Home -> complexity, then cycle the first estimate's class.
        let events = [
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Enter),
        ];
        let (_, buffer) =
            run_test_backend_with_discovery(model, empty_catalog(), discovery, events).unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("1 -> Medium"),
            "enter advances the highlighted point off its suggested class: {screen}"
        );
    }

    #[test]
    fn rollout_teams_are_named_and_kept_apart_from_type_labels() {
        let (discovery, mut model) = discovery_fixture();
        model.type_labels = Editable::complete(vec!["type:bug".to_owned()]);
        // Home -> rollout, toggle the only team, confirm.
        let events = [
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Char(' ')),
            key(KeyCode::Enter),
        ];
        let (_, buffer) =
            run_test_backend_with_discovery(model, empty_catalog(), discovery, events).unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("Engineering (ENG)") && !screen.contains("team-uuid"),
            "rollout names teams by key, not by UUID: {screen}"
        );
        assert!(
            !screen.contains("type:bug"),
            "type labels do not leak into the rollout allowlist: {screen}"
        );
    }

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
