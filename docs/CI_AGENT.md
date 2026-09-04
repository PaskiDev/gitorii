# The CI agent

`torii runner serve` runs CI jobs. It talks to a forge the way that forge's
own agent does, so no `gitlab-runner`, no `act_runner`, no per-platform
daemon to install next to torii.

This is a deliberate reversal of three lines in the ROADMAP that said torii
would stay above the runners and never become one. The reason for the
reversal is in this repository's own history: `packaging/aur` was blocked for
four months behind a self-hosted runner that turned out not to exist, on a
project whose entire subject is talking to forges. The control plane without
an executor could describe the problem and not fix it.

## Shape

The platform-specific part is small. Cloning, building the environment,
spilling file variables to disk, running the script, capturing output,
**masking** it, zipping artifacts and reporting a result are the same work
whatever answered the poll. So the platform sits behind four verbs and a
normalized job:

```rust
pub trait JobSource: Send {
    /// None when the queue is empty. Blocks for at most `timeout`.
    fn poll(&mut self, timeout: Duration) -> Result<Option<Job>>;
    /// Append to the live log. Returns whether the forge wants a cancel.
    fn append_trace(&mut self, job: &Job, chunk: &[u8], offset: usize) -> Result<TraceAck>;
    fn upload_artifacts(&mut self, job: &Job, archive: &Path, spec: &ArtifactSpec) -> Result<()>;
    fn finish(&mut self, job: &Job, outcome: Outcome) -> Result<()>;
}
```

Same pattern as `RunnerClient` and `PipelineClient` in
`crates/torii-core/src/platforms/`, which already normalize a heterogeneous
set of REST APIs behind one trait.

`Job` carries only what an executor needs: an id and a token, the repository
and the ref to check out, the ordered steps, the variables, and what to
collect afterwards. Everything a specific forge sends beyond that is the
platform module's business and does not leak into the executor.

### `CiVariable` is shared with the variables view

```rust
pub struct CiVariable {
    pub key: String,
    /// None when the forge will not read a secret back — which is most of them.
    pub value: Option<String>,
    pub kind: VariableKind, // EnvVar | File
    pub protected: bool,
    pub masked: bool,
    pub raw: bool,
    pub scope: Option<String>,
}
```

The same struct is what `torii ci var` and the TUI variables view manage, and
what `poll` hands the executor. Two consumers, one definition:

- `masked` decides what gets struck out of the trace before it is sent.
- `kind == File` decides what gets spilled to a temporary file with the path
  exported in place of the value.

Neither is optional in v1. Without file variables an SSH key cannot be used
at all; without masking the first job to echo its environment publishes it.

## GitLab

Pinned 04-09-2026 against both ends of the protocol, not from memory:

- server: `gitlab-org/gitlab`, `lib/api/ci/runner.rb`
- client: `gitlab-org/gitlab-runner`, `common/spec/spec.go`,
  `common/network.go`, `network/gitlab.go`, `common/consts.go`

Base is `<server>/api/v4/`. Two bearer headers, both plain values, no
`Bearer` prefix:

| Header         | Value                    | Used by                     |
| -------------- | ------------------------ | --------------------------- |
| `RUNNER-TOKEN` | runner authentication    | register, verify, poll      |
| `JOB-TOKEN`    | per-job, from the payload| trace, artifacts, job update|

### Register — `POST /runners`

Body: `{token, description, info, tag_list, run_untagged, locked,
access_level, paused, maximum_timeout}`, where `token` is the registration
token and `info` is `{name, version, revision, platform, architecture,
executor, shell, features, config}`. Returns `{id, token,
token_expires_at}`; that `token` is the runner authentication token, and the
only copy of it. `POST /runners/verify` with `{token, system_id}` checks a
token without consuming a job.

### Poll — `POST /jobs/request`

Body: `{info, token, system_id, last_update, session?}`.

- **201** — a job, as JSON.
- **204** — nothing queued. This is the normal answer, not an error.
- **403** — the token is dead; stop polling and say so, do not retry.
- **409 / 422 / 429** — conflict, orphaned runner, rate limited.

`system_id` identifies this *installation* of the agent, separately from the
runner registration; it must be stable across restarts. `last_update` is
echoed back from the previous response and lets the server skip work.

The response (`spec.Job`, exposed by
`API::Entities::Ci::JobRequest::Response`):

```
id, token, allow_git_fetch
job_info    { name, stage, project_id, project_name, project_full_path, ... }
git_info    { repo_url, ref, sha, before_sha, ref_type, refspecs, depth,
              repo_object_format, protected }
runner_info { uuid, timeout }
variables[] { key, value, public, file, masked, raw }
steps[]     { name, script[], timeout, when, allow_failure }
artifacts[] { name, untracked, paths[], exclude[], when, artifact_type,
              artifact_format, expire_in }
image, services, cache, credentials, dependencies, features, hooks, run,
inputs, policy_options, suspend_options
```

Everything after `steps` is either out of v1 scope or executor-specific.
`repo_url` carries credentials in it — never log it; the runner has a
`RepoCleanURL()` for exactly this reason.

`steps[].name` is `script`, `after_script` or `run`. `when` is `on_success`,
`on_failure` or `always`, and — this is the part that is easy to get wrong —
an **empty** `when` counts as `on_success`. `after_script` arrives as its own
step and must run whatever happened to the first one.

### Trace — `PATCH /jobs/:id/trace?debug_trace=<bool>`

`Content-Type: text/plain`, `JOB-TOKEN`, and a mandatory
`Content-Range: <start>-<end-1>` over the bytes in this chunk. A missing
Content-Range is a 400.

Responses, and what each means:

- **202** — appended. Advance the offset by what was sent.
- **416** — the server's idea of the offset differs. It returns
  `Range: 0-<size>`; resume from that, do not resend from zero.
- **403** — abort the job; **404** — the job is gone.

Every response carries `Job-Status` and `X-GitLab-Trace-Update-Interval`.
`Job-Status` is how a cancel arrives: the forge never pushes, the agent
learns the job was canceled by reading the header on its next append. The
interval header is the server asking to be patched more or less often, and
ignoring it is how a runner gets rate limited (`runner_jobs_patch_trace_api`
is a real limit on gitlab.com).

An empty chunk is skipped client-side rather than sent.

### Artifacts — `POST /jobs/:id/artifacts`

Query: `expire_in`, `artifact_format`, `artifact_type`. Body: multipart with
the archive in a form field literally named `file`. `JOB-TOKEN` authorizes
it. Formats the server accepts are `zip`, `gzip`, `raw`, `zipzstd`,
`tarzstd`; v1 writes `zip`, which needs a zip crate — nothing in the tree
writes archives today.

`POST /jobs/:id/artifacts/authorize` first is optional and exists for
Workhorse's direct-upload path. Skip it in v1.

### Finish — `PUT /jobs/:id`

Body: `{token, state, failure_reason, exit_code, output: {checksum,
bytesize}}`. `state` is `running`, `success` or `failed` — there is no
`canceled` to report, cancellation is observed, not reported.
`failure_reason` is `script_failure` when the user's script exited non-zero,
`runner_system_failure` when the agent itself broke, `job_execution_timeout`
on timeout. The distinction is visible in the GitLab UI and worth getting
right: `script_failure` blames the user, `runner_system_failure` blames us.

`output.bytesize` lets the server detect a trace it did not fully receive.

## Order of platforms

1. **GitLab** — the protocol above, documented and stable.
2. **Gitea / Forgejo / Codeberg** — `act_runner` speaks gRPC over Connect,
   open and versioned. One implementation covers three forges.
3. Decide with data after that.

Not planned, with reasons, so this does not get re-litigated:

- **GitHub and Azure** are reverse engineering against a moving target. Both
  descend from Azure DevOps, negotiate an encrypted session, and carry no
  compatibility promise for third-party agents.
- **Bitbucket** ships its runner as a container image, not a specification.
- **Radicle** is a different architecture — a broker pushes to adapters,
  there is no queue to poll.
- **SourceHut has no runners at all.** Jobs are submitted as a manifest to
  its build API. If torii ever drives sourcehut CI it will be as a client of
  that API, and it belongs in the ROADMAP that way rather than as a silent
  gap in a list of five supported forges.

## v1 boundary

In: shell executor, clone and checkout, variables (masked and file), steps
with `when` and `allow_failure`, live trace with resume, zip artifacts,
cancellation, timeout.

Out: docker and kubernetes executors, cache, services, dependency artifact
download, ephemeral runners, the session/terminal endpoint, OTEL tracing,
`run` steps (the native-steps successor to `script`).
