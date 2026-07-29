# GitHub App identity

**Status:** accepted; the `contents` permission remains conditional on the push-model decision
**Decision owner:** platform operator
**Last checked:** 2026-07-29

Spire authenticates to the GitHub API as a GitHub App. The scoped bot token
alternative recorded in [`security-and-authority.md`](security-and-authority.md)
is rejected.

The App was chosen because it is the only option under which Spire can read back
the authority it actually holds. An installation exposes its granted permissions
as a read-only object, so `spire doctor` can name a missing pull-request, check,
or comment permission individually instead of asserting a scope it cannot verify.
It also mints short-lived tokens with a permission subset chosen per call path,
which expresses the maker, reviewer, and publisher boundary in the credential
rather than in application discipline.

## What this is not

Spire is not distributed as a shared GitHub App. Operators do not install an App
owned by this project. Every installation registers its own App in its own
account or organization.

Two properties of Spire's topology force this:

- Spire mints installation tokens locally, inside the adapter, on the operator's
  own host. A shared App would require its private key on every host, or a
  central minting service holding write access to every operator's repositories.
- A GitHub App has one webhook URL. Each operator has their own webhook hostname,
  so one App registration cannot deliver events to every installation.

Per-operator registration is not a manual burden. Section 3 defines the automated
flow; no operator fills in the App form by hand.

## 1. Named parts

| Part | Owned by | Lifetime | Stored as | Sent to GitHub |
|---|---|---|---|---|
| App registration (App ID) | operator's account or org | durable | non-secret configuration | no |
| App private key | operator | until rotated | user secret store; system installations use `systemd:credentials/github-app-private-key` | never |
| Installation (installation ID) | the repositories the operator selects | durable | non-secret configuration | no |
| Installation access token | minted by Spire | one hour or less | process memory only | yes, as the bearer |
| Webhook secret | generated during registration | until rotated | user secret store | no |

The durable secret is the private key. An installation access token is never a
durable secret, is never written to the secret store, and never appears in
configuration.

## 2. Authority

### 2.1 GitHub API permissions

Approved for every installation:

| Permission | Level | Needed for |
|---|---|---|
| Metadata | read | repository and installation identity |
| Pull requests | write | open a pull request, comment, submit a review |
| Checks | read | required-check state for a head SHA |
| Actions | read | `workflow_run` outcomes |

Conditional on the still-blocked push-model row in
[`security-and-authority.md`](security-and-authority.md):

| Push model | Contents |
|---|---|
| mechanical publisher | write |
| maker direct push | read |

Spire requests no administration, members, repository-secrets, packages, or
workflow permission. Adding a permission is a change to this document and to the
registration manifest, not an operator-local adjustment.

### 2.2 Git transport authority is a separate credential

GitHub API authentication and Git transport authentication are different
credentials with different failure modes. The App authenticates API calls. Git
fetch and push use the runtime user's own SSH configuration. `spire doctor`
reports them as separate checks, and neither result implies the other.

### 2.3 What the App does not enforce

The permission set does not make a merge impossible. Branch protection or a
repository ruleset is the enforcement boundary, and the App must not appear in
any bypass list. A configuration in which the App can merge is reported by
`spire doctor` as unsafe rather than treated as working.

Spire submits pull-request reviews as comments only. It does not submit an
approving review.

## 3. Registration and onboarding

`spire auth login github` drives the GitHub App Manifest flow:

1. Spire posts a manifest to `https://github.com/settings/apps/new`, or the
   organization equivalent, with the permissions in section 2.1, the event set in
   the implementation contract, and the operator's webhook hostname.
2. The operator confirms one pre-filled page. GitHub redirects back with a
   temporary code that is valid once, for one hour.
3. Spire exchanges the code for the App ID, the private key, and the webhook
   secret, then writes the secret material directly to the user secret store.
4. The operator installs the App on the repositories Spire may act on.

The private key and webhook secret never pass through a clipboard, a shell
history, or an operator-authored file. The permission set is identical for every
installation because it comes from Spire's manifest rather than from a form.

The operator must be able to register and install an App in the target account or
organization. Where they cannot, the installation is blocked; Spire does not fall
back to another identity.

## 4. Rotation and failure behavior

- Private-key rotation verifies the replacement before activating it atomically
  and keeps the prior key active when verification fails.
- Installation-token minting and refresh happen inside the adapter against an
  injected clock. Concurrent refresh produces one active token and no partial
  cache entry.
- A suspended, uninstalled, or permission-reduced installation resolves to
  `permission_denied` or `unavailable`. An unrecognized response is `ambiguous`.
  None of these is ever reported as authenticated.
- A missing pull-request, check, or comment permission is named individually.

## Unknown / Unverified

- Whether the merge endpoint requires `contents: write`. This determines whether
  a mechanical publisher's token can merge at all, or whether branch protection
  is the only boundary.
- Whether a review submitted by an App with pull-request write satisfies a
  required approving review.
- Whether GitHub accepts a loopback `redirect_url` in the manifest flow.
- Installation identity and branch-protection evidence remain operator-supplied
  and unverified in [`external-identities.md`](external-identities.md).
