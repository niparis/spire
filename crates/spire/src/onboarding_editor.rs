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

use crate::onboarding_view::{
    ChoiceRow, CycleRow, ReadoutRow, SectionAction, SectionView, ToggleRow, Tone,
};
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
        let mut session = EditorSession::new(
            model,
            discovery,
            self.catalog.clone(),
            self.paths.clone(),
            trace_path,
        )?;
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

const HARNESS_PROVIDER_ROW: usize = 0;
const HARNESS_MODEL_ROW: usize = 1;
const HARNESS_EFFORT_ROW: usize = 2;

fn harness_role(section: OnboardingSection) -> OnboardingRole {
    if section == OnboardingSection::Maker {
        OnboardingRole::Maker
    } else {
        OnboardingRole::Reviewer
    }
}

struct EditorSession {
    model: OnboardingModel,
    discovery: OnboardingDiscovery,
    catalog: ModelCatalog,
    paths: spire_application::ResolvedPaths,
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
        paths: spire_application::ResolvedPaths,
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
            paths,
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

    /// The single place a section's presentation is described. Everything after
    /// this — cursor, navigation, key hints, layout — is shared by shape.
    fn section_view(&self, section: OnboardingSection) -> SectionView {
        match section {
            OnboardingSection::Linear => SectionView::Choose {
                rows: self
                    .discovery
                    .teams
                    .iter()
                    .map(|team| ChoiceRow {
                        label: format!("{} ({})", team.name, team.key),
                        current: self.model.team_id.value.as_deref() == Some(team.id.as_str()),
                    })
                    .collect(),
                empty: "no teams loaded yet - press r to fetch them from Linear".to_owned(),
            },
            OnboardingSection::WorkflowStates => SectionView::Cycle {
                headers: ("spire role", "linear state"),
                rows: LinearStateKind::ALL
                    .into_iter()
                    .map(|kind| CycleRow {
                        name: kind.as_str().to_owned(),
                        value: self
                            .model
                            .workflow_states
                            .get(&kind)
                            .and_then(|state| state.value.as_deref())
                            .map(|state_id| self.state_label(state_id))
                            .unwrap_or_else(|| "unbound".to_owned()),
                        note: None,
                    })
                    .collect(),
                empty: String::new(),
            },
            OnboardingSection::Complexity => SectionView::Cycle {
                headers: ("linear estimate", "spire complexity"),
                rows: self
                    .model
                    .complexity
                    .value
                    .as_ref()
                    .map(|complexity| {
                        complexity
                            .mapping
                            .iter()
                            .map(|(estimate, class)| CycleRow {
                                name: estimate.value().to_string(),
                                value: format!("{class:?}"),
                                note: None,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                empty: "select a Linear team with an estimate scale first".to_owned(),
            },
            OnboardingSection::Maker | OnboardingSection::Reviewer => self.harness_view(section),
            OnboardingSection::TypeLabels => SectionView::Toggle {
                rows: spire_application::DEFAULT_TYPE_LABELS
                    .iter()
                    .map(|label| ToggleRow {
                        label: (*label).to_owned(),
                        selected: self.selected_type_labels.contains(*label),
                    })
                    .collect(),
                empty: String::new(),
            },
            OnboardingSection::Rollout => SectionView::Toggle {
                rows: self
                    .discovery
                    .teams
                    .iter()
                    .map(|team| ToggleRow {
                        label: format!("{} ({})", team.name, team.key),
                        selected: self.selected_rollout_teams.contains(&team.id),
                    })
                    .collect(),
                empty: "no teams loaded yet - open the linear section and press r".to_owned(),
            },
            OnboardingSection::Paths => SectionView::Readout {
                rows: vec![
                    ReadoutRow::plain(format!(
                        "configuration file: {}",
                        self.paths.config_file.display()
                    )),
                    ReadoutRow::plain(format!(
                        "state directory:    {}",
                        self.paths.state_root.display()
                    )),
                    ReadoutRow::plain(format!("session trace:      {}", self.trace.path.display())),
                    ReadoutRow::toned(
                        format!("installation profile: {:?}", self.paths.profile),
                        Tone::Muted,
                    ),
                    ReadoutRow::toned(
                        "These are resolved before the editor starts. Change them with --config or SPIRE_HOME and re-run.",
                        Tone::Muted,
                    ),
                ],
                activates: None,
            },
            OnboardingSection::ReviewAndWrite => SectionView::Readout {
                rows: self
                    .model
                    .statuses()
                    .into_iter()
                    .map(|(section, status)| {
                        let tone = match status {
                            SectionStatus::Complete => Tone::Good,
                            SectionStatus::Stale { .. } => Tone::Warning,
                            SectionStatus::Incomplete { .. } => Tone::Normal,
                        };
                        ReadoutRow::toned(
                            format!("{}: {}", section.as_str(), status_summary(&status)),
                            tone,
                        )
                    })
                    .chain(self.model.off_catalog_roles.iter().map(|role| {
                        ReadoutRow::toned(
                            format!("{role:?} model: explicitly off-catalog / unverified"),
                            Tone::Warning,
                        )
                    }))
                    .collect(),
                activates: Some("write"),
            },
        }
    }

    fn harness_view(&self, section: OnboardingSection) -> SectionView {
        let role = harness_role(section);
        let Some(selection) = self.role_selection(role) else {
            return SectionView::Cycle {
                headers: ("setting", "value"),
                rows: Vec::new(),
                empty: "no harness selected yet".to_owned(),
            };
        };
        // The accepted levels are listed because they are a property of the
        // model: changing the model above changes this row's alternatives.
        let efforts = self
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
        let off_catalog = self.model.off_catalog_roles.contains(&role);
        SectionView::Cycle {
            headers: ("setting", "value"),
            rows: vec![
                CycleRow {
                    name: "provider".to_owned(),
                    value: selection.provider.to_string(),
                    note: None,
                },
                CycleRow {
                    name: "model".to_owned(),
                    value: selection.model.to_string(),
                    note: off_catalog.then(|| "off-catalog / unverified".to_owned()),
                },
                CycleRow {
                    name: "effort".to_owned(),
                    value: efforts,
                    note: None,
                },
            ],
            empty: String::new(),
        }
    }

    fn role_selection(&self, role: OnboardingRole) -> Option<&spire_application::HarnessSelection> {
        match role {
            OnboardingRole::Maker => self.model.maker.value.as_ref(),
            OnboardingRole::Reviewer => self.model.reviewer.value.as_ref(),
        }
    }

    fn team_configuration(&self) -> Option<&LinearTeamConfiguration> {
        self.model
            .team_id
            .value
            .as_deref()
            .and_then(|team_id| self.discovery.team_configurations.get(team_id))
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
        if matches!(key.code, KeyCode::Char('r') | KeyCode::Char('R')) {
            if section == OnboardingSection::Linear || section == OnboardingSection::Rollout {
                let _ = request_tx.send(DiscoveryRequest::ListTeams);
                if let Some(team_id) = self.model.team_id.value.clone() {
                    let _ = request_tx.send(DiscoveryRequest::TeamConfiguration { team_id });
                }
                self.error = Some("Refreshing from Linear...".to_owned());
            } else {
                self.screen = Screen::Section(OnboardingSection::ReviewAndWrite);
            }
            return false;
        }
        // A section marked stale by an upstream change may already hold the right
        // answer. Without this the only way to clear the mark is to alter a value
        // and alter it back.
        if matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A')) {
            self.model.confirm_section(section);
            let _ =
                self.trace
                    .mutation(section.as_str(), "accepted", serde_json::json!(true), true);
            return false;
        }
        if matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O'))
            && matches!(
                section,
                OnboardingSection::Maker | OnboardingSection::Reviewer
            )
            && self.section_index == HARNESS_MODEL_ROW
        {
            self.enter_off_catalog_model(section);
            return false;
        }
        if section == OnboardingSection::Complexity {
            self.seed_complexity_suggestion();
        }
        let view = self.section_view(section);
        let mut cursor = self.section_index;
        let action = view.navigate(key.code, &mut cursor);
        self.section_index = cursor;
        match action {
            Some(action) => self.apply_section_action(section, action, request_tx),
            None => false,
        }
    }

    /// The only per-section mutation logic left. Navigation, cursor bounds, and
    /// rendering are the view's concern.
    fn apply_section_action(
        &mut self,
        section: OnboardingSection,
        action: SectionAction,
        request_tx: &UnboundedSender<DiscoveryRequest>,
    ) -> bool {
        match (section, action) {
            (OnboardingSection::Linear, SectionAction::Activate(index)) => {
                if let Some(team) = self.discovery.teams.get(index) {
                    let team_id = team.id.clone();
                    self.select_team(team_id, request_tx);
                }
            }
            (OnboardingSection::WorkflowStates, SectionAction::Activate(index)) => {
                self.cycle_workflow_state(LinearStateKind::ALL[index])
            }
            (OnboardingSection::Complexity, SectionAction::Activate(index)) => {
                self.cycle_complexity_class(index)
            }
            (
                OnboardingSection::Maker | OnboardingSection::Reviewer,
                SectionAction::Activate(row),
            ) => self.cycle_harness_row(section, row),
            (OnboardingSection::TypeLabels, SectionAction::Toggle(index)) => {
                if let Some(label) = spire_application::DEFAULT_TYPE_LABELS.get(index) {
                    let label = (*label).to_owned();
                    if !self.selected_type_labels.insert(label.clone()) {
                        self.selected_type_labels.remove(&label);
                    }
                    self.model.type_labels =
                        Editable::complete(self.selected_type_labels.iter().cloned().collect());
                    self.model.confirm_section(section);
                    let _ = self.trace.mutation(
                        section.as_str(),
                        "labels",
                        serde_json::json!(self.model.type_labels.value),
                        false,
                    );
                }
            }
            (OnboardingSection::Rollout, SectionAction::Toggle(index)) => {
                if let Some(team) = self.discovery.teams.get(index) {
                    let team_id = team.id.clone();
                    if !self.selected_rollout_teams.insert(team_id.clone()) {
                        self.selected_rollout_teams.remove(&team_id);
                    }
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
            (OnboardingSection::ReviewAndWrite, SectionAction::Activate(_)) => {
                match self.model.validate() {
                    Ok(()) => return true,
                    Err(error) => self.error = Some(format!("write refused: {error}")),
                }
            }
            _ => {}
        }
        false
    }

    /// Suggestions lead, but every state stays reachable: a workspace whose
    /// names Spire does not recognise would otherwise offer nothing to cycle.
    fn cycle_workflow_state(&mut self, kind: LinearStateKind) {
        let Some(configuration) = self.team_configuration() else {
            return;
        };
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
        let Some(state_id) = candidates
            .get(candidate_index)
            .and_then(|index| configuration.states.get(*index))
            .map(|state| state.id.clone())
        else {
            return;
        };
        self.model
            .workflow_states
            .insert(kind, Editable::complete(state_id.clone()));
        self.model
            .confirm_section(OnboardingSection::WorkflowStates);
        let _ = self.trace.mutation(
            OnboardingSection::WorkflowStates.as_str(),
            kind.as_str(),
            serde_json::json!(state_id),
            false,
        );
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

    fn cycle_complexity_class(&mut self, index: usize) {
        let Some(complexity) = self.model.complexity.value.as_mut() else {
            return;
        };
        let Some((estimate, class)) = complexity
            .mapping
            .iter()
            .nth(index)
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

    fn enter_off_catalog_model(&mut self, section: OnboardingSection) {
        let role = harness_role(section);
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

    fn cycle_harness_row(&mut self, section: OnboardingSection, row: usize) {
        let role = harness_role(section);
        let Some(selection) = self.role_selection(role).cloned() else {
            return;
        };
        match row {
            HARNESS_PROVIDER_ROW => {
                let providers = ["codex", "claude-code"];
                let current = providers
                    .iter()
                    .position(|provider| *provider == selection.provider.as_str())
                    .unwrap_or(0);
                let Ok(provider) = HarnessId::new(providers[(current + 1) % providers.len()])
                else {
                    return;
                };
                let outcome = if role == OnboardingRole::Maker {
                    self.model.set_maker_provider(provider.clone())
                } else {
                    self.model.set_reviewer_provider(provider.clone())
                };
                let _ = self.trace.invalidation(&outcome);
                let _ = self.trace.mutation(
                    section.as_str(),
                    "provider",
                    serde_json::json!(provider.as_str()),
                    false,
                );
            }
            HARNESS_MODEL_ROW => {
                let Some(model) = self
                    .catalog
                    .next_model(&selection.provider, &selection.model)
                else {
                    return;
                };
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
            HARNESS_EFFORT_ROW => {
                let effort = self.catalog.next_effort(
                    &selection.provider,
                    &selection.model,
                    selection.effort,
                );
                self.model.set_effort(role, effort);
                let _ = self.trace.mutation(
                    section.as_str(),
                    "effort",
                    serde_json::json!(effort.as_str()),
                    false,
                );
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
        footer_for(session)
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

/// What the section decides. Shown in the section itself because a name like
/// "rollout" does not say what the value is used for; the key hints come from
/// the view so they cannot promise an interaction the shape does not offer.
fn section_help(section: OnboardingSection) -> &'static str {
    match section {
        OnboardingSection::Linear => "The Linear team Spire watches for tickets.",
        OnboardingSection::WorkflowStates => {
            "Which of this team's Linear states Spire reads and writes as it moves a ticket."
        }
        OnboardingSection::Complexity => {
            "Maps each Linear estimate point to the Spire complexity class that picks a harness."
        }
        OnboardingSection::Maker => {
            "The harness that writes the implementation. Its provider must differ from the reviewer's."
        }
        OnboardingSection::Reviewer => {
            "The harness that reviews the implementation, on a different provider than the maker."
        }
        OnboardingSection::TypeLabels => "Linear labels Spire treats as work types.",
        OnboardingSection::Rollout => {
            "Teams allowed to trigger automation. Everything stays inert until a team is listed here."
        }
        OnboardingSection::Paths => "Where this run will read and write files.",
        OnboardingSection::ReviewAndWrite => {
            "Every section's state. A refusal to write names the section responsible."
        }
    }
}

fn section_widget<'a>(
    session: &'a EditorSession,
    section: OnboardingSection,
    _area: Rect,
) -> impl Widget + 'a {
    let view = session.section_view(section);
    let mut lines = vec![Line::from(Span::styled(
        section_help(section),
        Style::default().fg(Color::DarkGray),
    ))];
    lines.push(Line::from(""));
    lines.extend(view.lines(session.section_index));
    Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(section.as_str().to_string()),
    )
}

impl EditorSession {
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

/// The section's own shape supplies the keys it answers to, so the footer
/// cannot advertise an interaction the component does not implement.
fn footer_for(session: &EditorSession) -> Line<'static> {
    match session.screen {
        Screen::Home => Line::from("up/down move  Enter open  r review  q/Esc quit"),
        Screen::QuitConfirmation => Line::from("Enter/y abandon  Esc/n return"),
        Screen::Section(section) => {
            let mut hints = vec![session.section_view(section).key_hint().to_owned()];
            if matches!(
                section,
                OnboardingSection::Linear | OnboardingSection::Rollout
            ) {
                hints.push("r refresh from Linear".to_owned());
            } else {
                hints.push("r review".to_owned());
            }
            if matches!(
                section,
                OnboardingSection::Maker | OnboardingSection::Reviewer
            ) {
                hints.push("o off-catalog model".to_owned());
            }
            hints.push("a accept as-is".to_owned());
            hints.push("Esc back".to_owned());
            hints.retain(|hint| !hint.is_empty());
            Line::from(hints.join("  "))
        }
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
        spire_application::ResolvedPaths::system(),
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
        assert!(
            screen.contains("model    = wide-model"),
            "maker rows render: {screen}"
        );
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
            screen.contains("1               = Medium"),
            "enter advances the highlighted point off its suggested class: {screen}"
        );
    }

    /// The home cursor persists between visits, so walking to a section has to
    /// start by pinning it to the top.
    fn open_section(section: OnboardingSection) -> Vec<KeyEvent> {
        let mut events = vec![key(KeyCode::Up); OnboardingSection::ALL.len()];
        events.extend(vec![key(KeyCode::Down); section as usize]);
        events.push(key(KeyCode::Enter));
        events
    }

    #[test]
    fn a_toggle_survives_leaving_the_section_without_a_confirmation_key() {
        let (discovery, model) = discovery_fixture();
        // Toggle, leave with Esc, come back: Enter is never pressed.
        let events = open_section(OnboardingSection::Rollout)
            .into_iter()
            .chain([key(KeyCode::Char(' ')), key(KeyCode::Esc)])
            .chain(open_section(OnboardingSection::Rollout));
        let (_, buffer) =
            run_test_backend_with_discovery(model, empty_catalog(), discovery, events).unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("[x] Engineering (ENG)"),
            "the toggle was written straight to the model: {screen}"
        );
    }

    #[test]
    fn rollout_teams_are_named_and_kept_apart_from_type_labels() {
        let (discovery, mut model) = discovery_fixture();
        model.type_labels = Editable::complete(vec!["type:bug".to_owned()]);
        let events = open_section(OnboardingSection::Rollout)
            .into_iter()
            .chain([key(KeyCode::Char(' '))]);
        let (_, buffer) =
            run_test_backend_with_discovery(model, empty_catalog(), discovery, events).unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("[x] Engineering (ENG)") && !screen.contains("team-uuid"),
            "space alone marks the team, named by key rather than UUID: {screen}"
        );
        assert!(
            !screen.contains("type:bug"),
            "type labels do not leak into the rollout allowlist: {screen}"
        );
    }

    #[test]
    fn the_linear_section_marks_the_team_already_selected() {
        let (discovery, model) = discovery_fixture();
        let (_, buffer) = run_test_backend_with_discovery(
            model,
            empty_catalog(),
            discovery,
            open_section(OnboardingSection::Linear),
        )
        .unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("(*) Engineering (ENG)"),
            "a chosen team is distinguishable from a merely highlighted one: {screen}"
        );
    }

    #[test]
    fn the_paths_section_names_the_configuration_file_it_will_write() {
        let (discovery, model) = discovery_fixture();
        let (_, buffer) = run_test_backend_with_discovery(
            model,
            empty_catalog(),
            discovery,
            open_section(OnboardingSection::Paths),
        )
        .unwrap();
        let screen = rendered(&buffer);
        assert!(
            screen.contains("configuration file: /etc/spire/spire.yaml"),
            "the section reports the destination rather than the trace file: {screen}"
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
