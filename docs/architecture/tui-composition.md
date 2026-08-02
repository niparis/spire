# TUI composition

**Status:** binding for every terminal surface in `crates/spire`
**Applies to:** ratatui 0.29, crossterm 0.29
**Last checked:** 2026-08-01

`spire init` is the only terminal surface today. This document describes how it
is organised, names the components it is built from, and states the rule that
keeps a new question from becoming a new one-off.

## Layers

Three layers, one direction of dependency.

| Layer | Where | Owns | Must not know |
|---|---|---|---|
| Model | `spire-application::onboarding` | `OnboardingModel`, `Editable<T>`, `OnboardingSection`, `SectionStatus`, validation, staleness, `OnboardingEditorPort` | ratatui, crossterm, a terminal, a runtime |
| Components | `crates/spire/src/onboarding_view.rs` | `SectionView`, cursor movement, bounds clamping, key hints, line layout | onboarding — no `OnboardingSection`, no `OnboardingModel` |
| Adapter | `crates/spire/src/onboarding_editor.rs` | terminal lifecycle, event loop, discovery channel, trace writer, binding sections to views and actions to mutations | — |

The component layer is deliberately ignorant of onboarding. It is a library of
interaction shapes, not a library of onboarding questions. The adapter is the
only place that knows both a section and the shape that presents it.

`crates/spire-domain` is not involved. The editor edits an application-layer
document; domain types appear only as values inside it (`ComplexityClass`,
`HarnessId`, `ModelId`).

## The four shapes

Every onboarding question is one of four interactions. `SectionView` has one
variant per shape and no variant per section.

| Variant | The question it asks | Keys | Used by |
|---|---|---|---|
| `Choose` | pick exactly one from a list | up/down, Enter | the home menu, `linear` |
| `Toggle` | pick any number from a fixed list | up/down, Space | `type_labels`, `rollout` |
| `Cycle` | advance a named setting through fixed alternatives | up/down, Enter | `workflow_states`, `complexity`, `maker`, `reviewer` |
| `Readout` | show text, offer at most one action | Enter, when `activates` is set | `paths`, `review_and_write` |

`Toggle` writes straight through to the model on each Space, so leaving the
section is the only confirmation there is. The other shapes report an action and
the adapter decides what it means.

The home menu is a `Choose` like any other. It is not a section, but it is a
list with a cursor, so it uses the same component — which is what stops the
cursor from looking one way there and another way inside a section.

Rows are described by `ChoiceRow`, `ToggleRow`, `CycleRow`, and `ReadoutRow`.
`ChoiceRow::marker` is a `RowMarker`: either `Chosen(bool)`, the radio marker
that distinguishes a chosen value from a merely highlighted one, or
`Status(glyph, tone)`, the progress marker a navigated menu shows instead. They
are variants rather than two fields because a row has one or the other, never
both. `CycleRow::note` carries a per-row caveat, such as an off-catalog model.
`ReadoutRow::tone` and `RowMarker::Status` both take a `Tone` (`Normal`,
`Muted`, `Warning`, `Good`) rather than a raw ratatui `Style`, so colour stays a
vocabulary rather than a decision made per call site.

`SectionView::Cycle` names both of its columns (`headers`). A two-column table
whose columns are unlabelled cannot tell the operator which side is Spire's
vocabulary and which side came from Linear.

## The rule

**No new interface element without a component.**

- A new section MUST be expressible as an existing `SectionView` variant. If it
  cannot be, add the variant to `onboarding_view.rs` first — with its
  navigation, its key hint, and its rendering — and only then use it.
- Rendering, cursor movement, and key hints for a section MUST NOT be written in
  `onboarding_editor.rs`. The adapter may describe rows and apply actions.
  Nothing else.
- The footer MUST take its key hints from `SectionView::key_hint`. A screen may
  not advertise a key its shape does not implement.
- A bespoke ratatui code path for one screen is a defect, not a shortcut.

There is one standing exception. `quit_widget` is a static two-line
confirmation with no cursor, no rows, and no model access; it answers `y`/`Esc`
and nothing else. Giving it a component would add a variant used once to
describe a `Paragraph`. If a second confirmation ever appears, add a `Confirm`
variant then and adopt both.

The rule exists because the editor was first written the other way. There was a
match arm per section in the renderer, another in the key handler, and a third
in the footer. They drifted:

- `workflow_states` rendered Linear's opaque state UUIDs instead of state names.
- `workflow_states` could not be changed at all, because its cycle only visited
  states Spire had already scored as suggestions — in a workspace whose state
  names Spire does not recognise, that is a one-element cycle.
- `complexity` could not be changed either, for an unrelated reason: its mapping
  was never seeded from the selected team's estimate scale, so the section had
  no rows to move through.
- The footer promised "space toggles, enter confirms" on a section where Enter
  did nothing.
- `type_labels` and `rollout` shared one selection set, so confirming type
  labels would have written `type:bug` into `rollout.allowed_team_ids`.

Each was a single-section bug that the other two copies of the section logic
could not see. Collapsing nine sections onto four shapes made all four
impossible to reintroduce in one section only.

## Event flow

There is exactly one path from a key press to a model mutation.

```
crossterm KeyEvent
└── EditorSession::handle_key          global keys (Ctrl-C -> quit confirmation)
    ├── handle_home_key                menu keys (r, q/Esc)
    │   └── SectionView::navigate      via home_view()
    │       └── open_section           reset cursor, seed suggestions
    └── handle_section_key             section-scoped keys (Esc, r, a, o)
        └── SectionView::navigate      cursor movement + clamping
            │                          -> Option<SectionAction>
            └── apply_section_action   the only place a key mutates the model
                └── OnboardingModel    validation, staleness, statuses
```

Both screens reach `navigate` the same way. Neither moves a cursor itself.

`SectionView::navigate` resolves navigation itself and never reports it upward;
the adapter only ever sees `SectionAction::Activate` or `SectionAction::Toggle`.
`apply_section_action` matches on `(section, action)` and is the only per-section
mutation logic in the editor.

Keys handled above the view — `Esc` (back), `r` (refresh from Linear, or jump to
review), `a` (accept a stale section as-is), `o` (enter an off-catalog model) —
are cross-section commands, not part of any shape. They are handled in
`handle_section_key` before the view is consulted.

## Rendering

`render_session` is the single draw path. It splits the frame vertically into a
body (`Min(5)`) and a footer (`Length(2)`), dispatches the body on `Screen`, and
renders the footer from the session's error, or from `footer_for` when there is
none.

A section body is: the help line from `section_help`, a blank line, then
`view.lines(session.section_index)`. `section_help` says what the section
decides — a name like "rollout" does not tell an operator what the value is used
for — and never says which keys to press, because keys come from the view.

Every render function takes `&EditorSession`. Rendering cannot mutate, by
signature.

## State ownership

- **Values** live in `OnboardingModel`, in the application layer.
- **Cursor** lives in `EditorSession` (`home_index`, `section_index`), not in the
  view. Views are rebuilt from current discovery data on every frame, so a view
  cannot carry a cursor across a rebuild — and a rebuilt view can be shorter than
  it was when the cursor last moved, which is why `navigate` clamps before it
  acts.
- **Pending multi-select** state is one `BTreeSet` per multi-select section.
  Never one shared set. See the drift list above.

## Terminal lifecycle

`validate_terminal` refuses to start unless stdin and stdout are TTYs, `TERM`
names a usable terminal, and the terminal is at least
`MIN_TERMINAL_COLUMNS`x`MIN_TERMINAL_ROWS` (80x24).

`TerminalGuard::enter` enables raw mode and the alternate screen; `restore` is
idempotent and also runs from `Drop`. `install_panic_cleanup` chains a panic hook
that leaves the alternate screen before the default hook prints, so a panic
cannot leave the operator with an unusable terminal.

## Testing

| Test surface | What it proves |
|---|---|
| `SectionView` unit tests in `onboarding_view.rs` | navigation, clamping, empty-state rendering, and column labelling per shape, with no model and no `Terminal` involved |
| `run_test_backend_with_discovery` | the real `render_session` and `handle_key` over ratatui's `TestBackend`, asserting on the resulting `Buffer` — no terminal, no network |
| `HeadlessOnboardingEditor` in `spire-application` | the `OnboardingEditorPort` contract, pure |

That split follows ratatui's own steer. `TestBackend`'s documentation says it is
"intended for integration tests that test the entire terminal UI", and that for a
single widget "it is preferable to write unit tests for widgets directly against
the buffer rather than using this backend". So shape-level behaviour is tested
without a `Terminal`, and `TestBackend` is reserved for whole-editor runs.

Buffer assertions check visible text, not ratatui's formatting. Column widths
and padding are the view's business and change when a row is added; asserting on
them makes every new section a test edit.

Discovery data in tests comes from `discovery_fixture`, which deliberately
contains one Linear state whose name Spire recognises and one it does not, so
the reachability guarantee above is exercised rather than assumed.

Two ratatui testing facilities we do not use, and why:

- `assert_buffer_eq!` is deprecated (ratatui 0.26.3) in favour of plain
  `assert_eq!`. `TestBackend::assert_buffer_lines` is the current ergonomic form
  and is a reasonable future move; our substring assertions survive layout
  changes that a full-line assertion would not.
- `insta` snapshot testing is officially documented for ratatui, but snapshots
  do not capture colour ([ratatui#1402]). Tone is load-bearing here — a stale
  section is yellow, a complete one green — so a snapshot would pass on a
  regression that an operator would see immediately.

[ratatui#1402]: https://github.com/ratatui/ratatui/issues/1402

## Relation to ratatui's own guidance

ratatui documents three application patterns — [the Elm architecture][tea],
[component][component], and [flux][flux] — but does not recommend one over
another, and its Elm page carries a disclaimer that it is "for theoretical
understanding and pedagogical purposes only". Nothing below should be read as
ratatui endorsing this design. Where we align, we align by argument.

**Where it agrees.** ratatui's [guidance on custom widgets][widgets] makes
exactly the argument the drift list above makes empirically:

> Implementing a widget is useful when a section of UI needs a meaningful
> boundary, not only when it will be reused. A helper such as
> `fn render_sidebar(area, buf, app: &App)` can accidentally depend on any field
> in `App`. A widget struct makes the inputs explicit, such as
> `Sidebar { selected, items }`, and the render implementation can only use
> those fields.

Our pre-refactor `section_widget(session, section, area)` was that helper. Every
section's rendering could read every field of `EditorSession`, which is how the
workflow-state renderer came to print a raw ID: nothing stopped it. `SectionView`
is the explicit-inputs form — a `ChoiceRow` carries a label and a flag and cannot
reach the model at all.

`SectionAction` is also the upward-communication shape the component template
uses: its `Component::handle_key_event` returns `Result<Option<Action>>`, a
"reified method call" that decouples the key from the behaviour. Ours is
narrower (`Option<SectionAction>`, no error, no async sender) because the editor
is synchronous.

**Where it differs, deliberately.**

- *`SectionView` is not a `Widget`.* It returns `Vec<Line>` and the adapter
  composes those with the help line into one `Paragraph` inside one bordered
  block. Making it a widget would require giving each section its own `Rect` and
  moving the block into the shape. Migration trigger: the first section that
  needs its own area — a scrollbar, a split pane, an inline editor — should
  become a `StatefulWidget` with `State = usize` rather than growing a special
  case here. ratatui's rule points the same way: keep state outside the widget
  "when state should live outside the widget and persist independently, such as
  selection, scroll offset, or cursor position", which is precisely our cursor.
- *No `Component` trait.* ratatui has no `Component` trait in the crate; the one
  in the [component template][template] is convention, tokio-based, and carries
  seven optional methods for an app with focus management and mouse routing.
  Our sections have neither. A four-variant enum is checked exhaustively at
  compile time, which a trait object is not — adding a shape will not compile
  until `len`, `navigate`, `key_hint`, and `lines` all account for it.
- *Key handling is partly centralised.* ratatui's [event handling][events] page
  warns that matching all events in one place "simply does not scale well",
  because keybind groups cannot be split out. We split the part that varies —
  each shape owns its own keys through `navigate` and `key_hint` — and keep the
  part that does not: `Esc`, `r`, `a`, and `o` mean the same thing in every
  section, so nine copies of them would be nine chances to disagree.

**Not documented upstream at all.** ratatui has no guidance on help footers; its
examples hard-code a hint string per app. Deriving the footer from
`SectionView::key_hint` is ours, and it is the fix for a real defect: a
hard-coded footer promised "enter confirms" on a section where Enter did nothing.

**Version.** These claims are checked against ratatui 0.29 / crossterm 0.29.
ratatui 0.30 split the crate into `ratatui-core` and `ratatui-widgets` and
reversed the `WidgetRef` blanket impl in favour of `impl Widget for &T`. We use
neither `WidgetRef` nor `StatefulWidgetRef` — both are behind the
`unstable-widget-ref` feature on 0.29 and deprecated in direction on 0.30 — so
the upgrade should not touch this design.

[tea]: https://ratatui.rs/concepts/application-patterns/the-elm-architecture/
[component]: https://ratatui.rs/concepts/application-patterns/component-architecture/
[flux]: https://ratatui.rs/concepts/application-patterns/flux-architecture/
[widgets]: https://ratatui.rs/concepts/widgets/#when-to-make-a-custom-widget
[events]: https://ratatui.rs/concepts/event-handling/
[template]: https://github.com/ratatui/templates/tree/main/component

## Adding a section

1. Add the variant to `OnboardingSection` in `spire-application::onboarding`, and
   to `ALL`. The `as_str` name appears in the trace, so it is a contract.
2. Extend `OnboardingModel` with the field and teach `statuses`/`validate` what
   completeness means for it.
3. Add one arm to `section_view` returning an existing `SectionView` variant.
4. Add one arm to `apply_section_action`.
5. Add one arm to `section_help` describing what the section decides.
6. Nothing else. If step 3 forces you to reach for a new shape, add the variant
   to `onboarding_view.rs` with its navigation, hint, and rendering first.

Steps 3 to 5 are compiler-enforced: all three match on `OnboardingSection`
without a catch-all, so a section that reaches none of them does not build. That
is deliberate. `apply_section_action` used to end in `_ => {}`, which meant a
section could be added, rendered, navigated, and be silently inert on Enter —
the same class of defect as the four above, and invisible for the same reason.
`HarnessRow` is an enum rather than row-index constants for the same purpose.
