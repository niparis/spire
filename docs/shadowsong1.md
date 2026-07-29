 just added a new piece to my agent harness: an AI that reviews every PR before I do.

Not a SaaS product. Not a GitHub App. Just OpenCode wired into my self-hosted GitHub Actions runner, authenticating through my ChatGPT subscription.

Here’s what it does and the harder questions it exposed.

The Setup

Three agents, each with a role:

Claude Opus — local supervisor. Picks tickets, runs the loop, decides what to do with review feedback.
Kimi / Deepseek — delegate. Does the actual implementation, pushes commits.
GPT (via ChatGPT subscription) — reviewer. Runs on every PR push through a GitHub Actions workflow on my self-hosted runner.
The workflow is packaged as a reusable GitHub Action under Shadowsong27/gh-actions-commons: trigger on PR events, skip drafts and forks, feed the diff + a review prompt to opencode run, pipe output through jq, post a comment.

Design Choices Worth Noting

Auth via subscription, not API key. The official OpenCode GitHub integration wants ANTHROPIC_API_KEY or OPENAI_API_KEY in GitHub Secrets — metered, per-token. The CLI supports headless device auth against a ChatGPT Pro/Plus subscription instead: visit a URL, enter a device code, runner gets an OAuth token tied to my account. Token lives on the runner filesystem, not in Secrets. No per-review billing, no key management.

Append-only comments. Each push gets its own review comment rather than updating a sticky thread. You get a paper trail of what the model thought at each commit — useful when the delegate is iterating through feedback across multiple pushes.

Model as a config variable. The reviewer model is a GitHub settings value, not hardcoded in the workflow. Easier to swap without touching the action itself.

The Orchestration Problem

The reviewer is simple: read diff, output comment, forget everything. The loop around it is where things break.

Opus is supposed to: pick ticket → delegate to Kimi/Deepseek → wait for PR → wait for GPT review → read feedback → instruct delegate to fix → repeat. In practice, Opus drifts. It reads the review, decides the PR looks fine, and stops issuing instructions. Or chases something tangential. Or the context grows long enough that the loop instruction dissolves. Soft instructions decay.

LLMs aren’t state machines. “Keep doing this until condition X” conflicts with a model that’s also trying to be helpful, non-repetitive, and context-aware. Those goals eat the loop.

What’s needed is a hard orchestration layer that owns state and enforces transitions — Temporal, or even a simple state machine. The LLM makes judgment calls; the orchestrator does the bookkeeping. CI/CD learned this: you don’t trust developers to remember to run tests. Same principle.

Exit conditions are a policy, not a prompt. My current instruction to Opus: fix medium and high severity issues, file tickets for low ones, ignore cosmetic. Reasonable policy, but it lives in a prompt — so Opus both executes the work and decides when it’s done. Severity classification is subjective. “File a ticket” produces work outside the loop that may never happen. And the loop exits when Opus feels done, which drifts in both directions.

The fix is the same: the orchestrator owns the exit policy, tracks open issues, decides termination. The model reports; the orchestrator judges.

Should the Reviewer Stay Stateless?

The bot sees the diff, posts a comment, forgets everything. Next push, same thing.

The case for it: each review is independent and reproducible. No accumulated bias. The diff is the full context.

The gaps: a stateless reviewer repeats itself across pushes, flagging the same issue each time. And it can’t tell whether the delegate ignored feedback or was instructed by Opus to ignore it — two very different situations that look identical from the outside.

The append-only comment partially covers this: the human sees the full review history even though the bot doesn’t. Stateless execution, stateful record. The model doesn’t remember; the thread does.

Giving the reviewer memory might make things worse. A model that’s learned a codebase’s conventions is harder to correct when those conventions are wrong. Statelessness keeps it honest.

HITL and the Boundary Problem

No human is in the loop. GPT posts a comment, Opus reads it, Opus tells the delegate what to fix. Nothing is approved before it drives work.

When all three agents behave, fine. When they don’t — GPT flags something spuriously, Opus takes it literally, the delegate pushes bad code, GPT reviews the bad code and doubles down or contradicts itself — there’s no circuit breaker.

A few directions worth exploring, none fully thought through:

Review-gate. Human sees the review before Opus does. Strong guarantee, kills the latency benefit entirely.

Confidence-threshold. GPT flags severity. High-severity items go to a human first; low-severity go straight to Opus.

Iteration-limit. After N cycles without resolution, escalate. Circuit breaker, not a gate — doesn’t prevent bad loops, just stops them from running forever.

Agreement-protocol. Before Opus instructs the delegate, it must acknowledge each review point in structured output: fixing / leaving as-is (reason) / disagreeing (reason). Orchestrator validates and escalates if Opus skips a point. Closest to how human code review actually works — but also requires Opus to be reliable enough to follow the protocol, which is the same problem we already have.

Model cost and tiers. The setup is already tiered: Opus for judgment (expensive), Kimi/Deepseek for implementation (cheaper), GPT via subscription for review (fixed cost). That’s deliberate — you don’t need a strong model writing the code, just deciding what to write and whether feedback is worth acting on.

The risk: cheaper models push cost onto the harness. The delegate won’t catch bad instructions from Opus — it just executes. Downgrading the supervisor means the orchestration layer needs to compensate with harder guardrails. The cost saving gets eaten by the scaffolding required to make it reliable.

The Caveats I’m Living With

The trigger condition doesn’t survive an agentic loop. Reviewer fires on every push. Fine for human-paced commits. Falls apart when the delegate is pushing several commits quickly — each one triggers a review inference, floods the PR thread with stale intermediate reviews, burns quota. Needs a debounce window, a CI-green gate, or an explicit ready-for-review signal. None of that is in place.

Subscription quota failures are opaque. When quota runs out, OpenCode exits with an error and the workflow fails — with no signal distinguishing quota exhaustion from model unavailability from a network error. Right fix: detect the specific error, swap to a secondary token or metered key, alert. Not built yet.

Trust boundary. The fork guard blocks external contributors from running on my runner. Same-repo branch authors can modify the workflow YAML or prompt file. Fine for my setup; worth being explicit about.

Prompt injection is real. PR diffs are untrusted text fed to an agent with tool access. Anti-exfiltration instructions help, but it’s an arms race.

Why This Matters (to Me)

I don’t think the agentic harness problem is solved. The tooling is moving fast, the patterns are unsettled, and most writing about it is either hype or hindsight.

You could wait. Let the dust settle, adopt whatever emerges as the consensus stack. But that means following a wave someone else built.

I enjoy building automation. I’d rather build it now, imperfectly, and evolve alongside the tooling and the engineers working through the same problems. The suboptimal part doesn’t bother me.

There’s also a practical reason. If I haven’t built it, I can’t lead a team to build it well. Without a point of view, you get ten engineers each picking a different harness, a different model, a different pattern — and a codebase that reflects all of that. Someone has to develop opinions early, and that requires doing it first.

What’s Next

Almost every problem above has the same root cause: no real orchestration layer. The loop lives in Opus’s context. The trigger is a naive event hook. The exit condition is a prompt instruction. HITL doesn’t exist. None of these are separate problems.

A proper orchestration layer — one that owns state, enforces transitions, controls when the reviewer fires, tracks open issues, knows when to escalate — would solve or directly enable a solution to most of this. That’s the next investment. Everything else is patching around its absence.