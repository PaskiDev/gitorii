# Security Policy

## 🔒 Reporting a Vulnerability

**Please DO NOT open a public issue for security vulnerabilities.**

### How to Report

Send security vulnerabilities to: **paski@paski.dev**

Include in your report:
- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact
- Suggested fix (if you have one)

### What to Expect

- **Acknowledgment**: Within 48 hours
- **Initial Assessment**: Within 5 business days
- **Status Updates**: Every 7 days until resolved
- **Public Disclosure**: After fix is released (coordinated disclosure)

### Responsible Disclosure

We follow responsible disclosure practices:

1. You report the vulnerability privately
2. We confirm and investigate
3. We develop and test a fix
4. We release the fix
5. We publicly disclose (with credit to you if desired)
6. You can publish your findings after disclosure

## 🛡️ Security Features in Torii

### Credentials Management

- ✅ Credentials stored in OS keychain (not plaintext)
- ✅ Support for SSH keys and tokens
- ✅ OAuth flows for GitHub/GitLab/Bitbucket
- ❌ Never logs credentials
- ❌ Never transmits credentials except to authenticated endpoints

### Snapshot Security

- Snapshots are **local-only** by default
- Sensitive files (`.env`, credentials) excluded from snapshots
- Snapshots encrypted at rest (optional, configurable)
- Auto-cleanup prevents unlimited disk usage

### Network Security

- ✅ TLS for all external communications
- ✅ Certificate pinning for critical endpoints
- ✅ Webhook signature verification
- ❌ No telemetry or tracking by default

### Code Analysis Security

- AI analysis runs **locally** by default
- Premium tier analysis: data encrypted in transit
- No code sent to third parties without explicit consent
- You can review analysis queries before sending

## 🔐 Supported Versions

We provide security updates for:

| Version | Supported          |
| ------- | ------------------ |
| 0.x.x   | :white_check_mark: |

Once 1.0 is released:

| Version | Supported          |
| ------- | ------------------ |
| 1.x.x   | :white_check_mark: |
| < 1.0   | :x:                |

## 🚨 Known Security Considerations

### Git Protocol Security

Torii inherits security properties of `git2-rs` and `libgit2`:
- SSH key management follows git standards
- HTTPS uses system certificate stores
- Git hooks are **disabled by default** (security risk)

### Snapshot Privacy

**Important**: Snapshots may contain:
- Uncommitted code
- API keys in files (if not in .toriignore)
- Passwords in config files
- Personal data in test files

**Recommendation**: 
- Use `.toriignore` properly
- Enable snapshot encryption
- Review snapshot contents before sharing

### Mirror Sync Security

When syncing to mirrors:
- Credentials for each platform stored separately
- Sync operations use platform APIs (not git protocol)
- Webhook secrets rotated periodically
- Failed auth attempts logged and rate-limited

## 📡 Upstream / Hosting-Provider Advisories

Vulnerabilities in the git hosting providers themselves (GitHub, GitLab,
Codeberg, etc.) are out of scope for torii — torii is just a client. We
list noteworthy ones here so users running self-hosted instances know to
patch.

### CVE-2026-3854 — GitHub Enterprise Server `git push -o` RCE (April 2026)

A server-side bug in GitHub's `babeld` / `gitauth` / `gitrpcd` proxy
chain let any authenticated user inject semicolons into a `git push -o`
option value, override an internal `X-Stat` header, and gain remote code
execution on GitHub's backend. CVSS 8.8.

- **Not a torii bug.** Identical to using stock `git push -o` against the
  same server. torii's transport (rustls / russh) only ferries the bytes
  the user typed; the server mis-parses them.
- **GitHub.com:** patched by GitHub on 2026-03-04 (~2h after report).
- **GitHub Enterprise Server:** upgrade to 3.14.25 / 3.15.20 / 3.16.16 /
  3.17.13 / 3.18.7 / 3.19.4 or newer.
- **No torii action required.**

References: [NVD CVE-2026-3854](https://nvd.nist.gov/vuln/detail/CVE-2026-3854),
[Wiz writeup](https://www.wiz.io/blog/github-rce-vulnerability-cve-2026-3854),
[GitHub blog](https://github.blog/security/securing-the-git-push-pipeline-responding-to-a-critical-remote-code-execution-vulnerability/).

## 🛠️ Security Best Practices for Users

### Credentials

```bash
# ✅ Good: Use SSH keys
torii config --auth-method ssh

# ✅ Good: Use tokens with minimal scope
torii config --token <token> --scope "repo:read,repo:write"

# ❌ Bad: Store credentials in plaintext
# Never do: git config credential.helper store
```

### Snapshots

```toml
# .torii/config.toml

[snapshots]
enabled = true
encrypt = true  # ✅ Recommended
exclude_patterns = [
    "**/.env*",
    "**/credentials.json",
    "**/*_rsa",
    "**/*.key",
]
```

### Mirror Sync

```toml
[mirrors]
# ✅ Good: Use deploy keys (read-only on mirrors)
[[mirrors.github]]
url = "https://github.com/user/repo"
auth_method = "deploy_key"
permissions = "read"

# ⚠️ Caution: Personal access tokens have broad scope
# Use with minimal permissions only
```

## 📋 Security Checklist for Contributors

Before submitting code:

- [ ] No hardcoded credentials or API keys
- [ ] No use of `unwrap()` on user input
- [ ] Input validation for all external data
- [ ] Proper error handling (no panic on bad input)
- [ ] Dependency review (cargo audit)
- [ ] No `unsafe` blocks without justification
- [ ] Sensitive data sanitized from logs

## 🔍 Security Audits

We welcome security audits of Torii. If you're conducting an audit:

1. Email paski@paski.dev with your intent
2. We'll provide guidance on scope and priorities
3. You can request clarification on implementation details
4. Report findings according to responsible disclosure above

## 🏆 Security Hall of Fame

Security researchers who have responsibly disclosed vulnerabilities:

<!-- Will be populated as researchers contribute -->
_No reports yet - be the first!_

## 📞 Contact

- **Security Issues**: paski@paski.dev
- **General Contact**: paski@paski.dev
- **PGP Key**: [To be published]

---

**Last Updated**: 2026-04-09
