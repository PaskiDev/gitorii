// Sensitive data scanner — runs before every commit
use crate::error::Result;
use std::path::Path;

/// A detected sensitive pattern in a file
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub pattern_name: String,
    pub preview: String,
}

/// Patterns that indicate sensitive data
struct Pattern {
    name: &'static str,
    /// Returns true if the line matches
    detect: fn(&str) -> bool,
}

fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let visible = 4;
    format!(
        "{}{}",
        &chars[..visible].iter().collect::<String>(),
        "*".repeat(chars.len() - visible)
    )
}

/// Skip blobs larger than this when scanning. Secrets virtually never live in
/// huge files (lockfiles, generated assets) and reading a 500MB blob to
/// memory blows up OOM during `scan --history` on big repos. Override with
/// the env var `TORII_SCAN_MAX_BYTES`.
const DEFAULT_MAX_BLOB_BYTES: usize = 5 * 1024 * 1024;

fn max_blob_bytes() -> usize {
    std::env::var("TORII_SCAN_MAX_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_BLOB_BYTES)
}

/// Find a token starting with `prefix` anywhere in `line` — not just at a
/// whitespace boundary. Real secrets are routinely glued directly to an
/// assignment with no surrounding spaces (`KEY=ghp_xxx`, `-e
/// KEY=AKIA...` container args, `.env` files, JSON/YAML one-liners), which
/// turns `KEY=ghp_xxx` into a single whitespace-delimited word that does
/// not itself *start with* the prefix — the bug this helper replaces.
///
/// `extra_token_chars` lets callers widen what counts as "part of the
/// token" beyond alphanumeric/`_`/`-` (e.g. SendGrid keys use literal
/// dots: `SG.xxx.yyy`).
///
/// Returns the full matched token (prefix + the following run of token
/// characters) for the first occurrence whose total length is `> min_len`.
fn find_prefixed_token(
    line: &str,
    prefix: &str,
    min_len: usize,
    extra_token_chars: &[char],
) -> Option<String> {
    let mut search_from = 0;
    while let Some(rel) = line[search_from..].find(prefix) {
        let idx = search_from + rel;
        let candidate = &line[idx..];
        let token: String = candidate
            .chars()
            .take_while(|c| {
                c.is_alphanumeric() || c == &'_' || c == &'-' || extra_token_chars.contains(c)
            })
            .collect();
        if token.len() > min_len {
            return Some(token);
        }
        // Advance past this occurrence and keep looking — a short/bogus
        // match here shouldn't hide a real one later on the same line.
        search_from = idx + prefix.len();
        if search_from >= line.len() {
            break;
        }
    }
    None
}

/// Does the authority section right after `scheme_marker` (e.g.
/// `"postgresql://"`) actually carry a `user:password@` credential — not
/// just a bare `user@host` (SSH-style, nothing secret to leak) and not a
/// *syntactic* placeholder (`${PASSWORD}`, `$PASSWORD`, `<password>`) that
/// can never be a real value in the first place?
///
/// Scoping the check to the authority segment (up to the first `/`, `?`,
/// `#`, quote, or whitespace) — instead of asking "does `@` appear
/// anywhere on the line" — is what lets this stay precise: a bare
/// `scheme://user@host` never matches, no matter what else is on the line.
///
/// **No minimum length, anywhere** — a 1-character user, password, or
/// host is checked exactly like a 32-character one. A short leaked
/// password is *more* dangerous (easier to guess/brute-force by whoever
/// finds it), not less, so there is deliberately no threshold to lower.
///
/// **Word-based password placeholders are NOT excluded on purpose.**
/// `user:password@host`, `user:pass@host`, `root:root@host`,
/// `admin:changeme@host` are all flagged. A hardcoded allowlist of
/// "obviously fake" words (`password`, `pass`, `changeme`, `root`,
/// `xxx`, …) is exactly the list a real (if weak) password eventually
/// collides with — excluding it would recreate the same "protects
/// backwards" problem the missing-length-threshold bug had. The only
/// values excluded here are ones that are *syntactically* not a value at
/// all — a shell/template variable reference (`${PASSWORD}`,
/// `$PASSWORD`) or an explicit placeholder marker (`<password>`) — so a
/// false positive on a README's `user:pass@localhost` example is
/// accepted as the cheaper failure mode; clear it with `--yes` for one
/// commit or silence it for good via `.toriignore`'s `[secrets]`
/// allowlist.
fn url_has_real_password(line: &str, scheme_marker: &str) -> bool {
    let lower = line.to_lowercase();
    let Some(pos) = lower.find(scheme_marker) else {
        return false;
    };
    let rest = &line[pos + scheme_marker.len()..];
    let end = rest
        .find(|c: char| matches!(c, '/' | '?' | '#' | '"' | '\'') || c.is_whitespace())
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    let Some(at) = authority.find('@') else {
        return false; // no userinfo segment at all
    };
    let userinfo = &authority[..at];
    let Some(colon) = userinfo.find(':') else {
        return false; // `user@host` — no password, nothing to leak
    };
    let password = &userinfo[colon + 1..];
    if password.is_empty() {
        return false;
    }
    // Syntactic placeholders only — see doc comment above for why word
    // guesses like "password"/"pass"/"changeme"/"root" are deliberately
    // NOT on this list.
    !(password.starts_with('<') || password.starts_with("${") || password.starts_with('$'))
}

/// How many patterns the scanner carries out of the box. The safety view
/// shows it beside the user's own rules so the two are never confused.
pub fn builtin_pattern_count() -> usize {
    PATTERNS.len()
}

/// The name of every built-in pattern, for a screen that lists them.
pub fn builtin_pattern_names() -> Vec<&'static str> {
    PATTERNS.iter().map(|p| p.name).collect()
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        name: "Private key (PEM)",
        detect: |l| {
            l.contains("-----BEGIN")
                && (l.contains("PRIVATE KEY")
                    || l.contains("RSA PRIVATE")
                    || l.contains("EC PRIVATE"))
        },
    },
    Pattern {
        name: "JWT token",
        detect: |l| {
            // eyJ... base64 header — at least 3 segments
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '.' && c != '_' && c != '-'
                });
                let parts: Vec<&str> = w.split('.').collect();
                parts.len() == 3
                    && parts[0].starts_with("eyJ")
                    && parts[0].len() > 10
                    && parts[1].len() > 10
            })
        },
    },
    Pattern {
        name: "AWS access key",
        detect: |l| {
            ["AKIA", "ASIA", "AROA"].iter().any(|prefix| {
                // Exact 20-char key ID: min_len 19 means "> 19", i.e. >= 20;
                // the length==20 check below rejects anything longer.
                match find_prefixed_token(l, prefix, 19, &[]) {
                    Some(token) => {
                        token.len() == 20
                            && token
                                .chars()
                                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                    }
                    None => false,
                }
            })
        },
    },
    Pattern {
        name: "AWS secret key",
        detect: |l| {
            let lower = l.to_lowercase();
            (lower.contains("aws_secret") || lower.contains("aws secret"))
                && (l.contains('=') || l.contains(':'))
                && l.len() > 40
        },
    },
    Pattern {
        name: "GitHub/GitLab token",
        detect: |l| {
            let trimmed = l.trim();
            // Skip HTML/template lines — tokens inside HTML are demo content
            if trimmed.starts_with('<') || trimmed.starts_with("//") || trimmed.starts_with("*") {
                return false;
            }
            // min_len 20 ⇒ token must be > 20 chars — bare prefix mentions
            // in docs (`ghp_xxx`, `glpat-xxx`) fall well under that, so the
            // dedicated placeholder-length checks the old word-boundary
            // version needed are redundant here.
            const PREFIXES: &[&str] = &["ghp_", "gho_", "ghs_", "github_pat_", "glpat-", "glptt-"];
            PREFIXES
                .iter()
                .any(|prefix| match find_prefixed_token(l, prefix, 20, &[]) {
                    Some(token) => {
                        let low = token.to_lowercase();
                        !(low.ends_with("xxx") || low.contains("xxxx"))
                    }
                    None => false,
                })
        },
    },
    Pattern {
        name: "Generic API key / token",
        detect: |l| {
            let lower = l.to_lowercase();
            let has_key_word = lower.contains("api_key")
                || lower.contains("api_secret")
                || lower.contains("auth_token")
                || lower.contains("access_token")
                || lower.contains("secret_key")
                || lower.contains("private_key")
                || lower.contains("password")
                || lower.contains("passwd")
                || lower.contains("auth_token");
            let has_assignment = l.contains('=') || l.contains(':');
            let has_value = l
                .split(&['=', ':'][..])
                .nth(1)
                .map(|v| {
                    let v = v
                        .trim()
                        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`');
                    let vl = v.to_lowercase();
                    // Real secrets: no spaces, no sentence punctuation, min length

                    v.len() >= 16
                        && !v.contains(' ')
                        && !v.contains('.')  // sentences have dots
                        && !v.starts_with("${")
                        && !v.starts_with("$(")
                        && !v.starts_with("process.env")
                        && !v.starts_with("env.")
                        && !v.starts_with("os.environ")
                        && !v.starts_with("<")
                        // Type declarations, not values: `brevo_api_key:
                        // Arc<RwLock<String>>` matches key-word + `:` +
                        // "no spaces, long enough" — angle brackets are the
                        // one thing an actual secret value never contains.
                        && !v.contains('<')
                        && !v.contains('>')
                        // English placeholders
                        && !vl.eq("your_secret_here")
                        && !vl.eq("changeme")
                        && !vl.eq("placeholder")
                        && !vl.eq("todo")
                        && !vl.starts_with("your_")
                        && !vl.starts_with("my_")
                        && !vl.contains("example")
                        && !vl.contains("sample")
                        && !vl.contains("replace")
                        && !vl.contains("change_me")
                        && !vl.contains("insert")
                        // Spanish placeholders
                        && !vl.starts_with("tu_")
                        && !vl.starts_with("mi_")
                        && !vl.contains("cambiar")
                        && !vl.contains("reemplazar")
                        && !vl.contains("ejemplo")
                        && !vl.contains("aqui")
                        && !vl.contains("pon_")
                        && !vl.contains("escribe")
                })
                .unwrap_or(false);
            has_key_word && has_assignment && has_value
        },
    },
    Pattern {
        name: "Database connection string with credentials",
        detect: |l| {
            // Every scheme a real `DATABASE_URL`-style value is likely to
            // use, including short/alternate spellings that are just as
            // common in the wild as the canonical name — a missing alias
            // here is a silent blind spot, not a narrower rule.
            // `"postgres://"` (the libpq/Heroku/sqlx/psycopg/pg alias of
            // `postgresql://`) and `"mongodb+srv://"` (MongoDB Atlas'
            // standard form, which doesn't contain `"mongodb://"` as a
            // substring because of the `+srv`) were missing before this
            // fix and are the two most common real-world cases of it.
            const SCHEMES: &[&str] = &[
                "postgresql://",
                "postgres://",
                "mysql://",
                "mongodb://",
                "mongodb+srv://",
                "redis://",
                "libsql://",
                "turso://",
            ];
            SCHEMES
                .iter()
                .any(|scheme| url_has_real_password(l, scheme))
        },
    },
    Pattern {
        name: "Stripe key",
        detect: |l| {
            ["sk_live_", "pk_live_", "rk_live_"]
                .iter()
                .any(|prefix| find_prefixed_token(l, prefix, 16, &[]).is_some())
        },
    },
    Pattern {
        name: "Twilio / SendGrid / Brevo key",
        detect: |l| {
            // SendGrid: "SG." is distinctive enough to scan anywhere in the
            // line (glued to `KEY=SG....` with no spaces is common), and
            // the token itself legitimately contains literal dots
            // (`SG.xxxxxxxx.yyyyyyyy`).
            if find_prefixed_token(l, "SG.", 40, &['.']).is_some() {
                return true;
            }
            // Twilio Account SID ("AC" + 32 chars): the prefix is too
            // short/common to scan as a free substring (plenty of ordinary
            // identifiers start with "AC"), so this one stays anchored to
            // a whole whitespace-delimited word.
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                w.starts_with("AC") && w.len() == 34 && w.chars().all(|c| c.is_ascii_alphanumeric())
            })
        },
    },
    Pattern {
        name: "Bearer token with a literal value",
        detect: |l| {
            // Every other pattern with a vendor-specific prefix (JWT,
            // Stripe, ghp_/glpat-, …) is tried first by the caller and
            // wins the label — this one is the fallback for an opaque
            // bearer token with no recognizable prefix at all, which
            // nothing else here covers.
            let mut search_from = 0;
            while let Some(rel) = l[search_from..].find("Bearer ") {
                let idx = search_from + rel;
                let after = &l[idx + "Bearer ".len()..];
                // A real token is a contiguous run of token-ish
                // characters; a placeholder like `<TOKEN>`, `${TOKEN}`, or
                // `$TOKEN` stops this run at position 0 (`<`/`$` aren't
                // token chars), which already rejects those for free.
                let token: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/'))
                    .collect();
                if token.len() > 20 {
                    let low = token.to_lowercase();
                    let is_placeholder = low.starts_with("your_")
                        || low.starts_with("my_")
                        || low.contains("example")
                        || low.contains("placeholder")
                        || low.contains("xxxx")
                        || low.contains("token_here");
                    if !is_placeholder {
                        return true;
                    }
                }
                search_from = idx + "Bearer ".len();
                if search_from >= l.len() {
                    break;
                }
            }
            false
        },
    },
];

/// Extensions/suffixes that are safe to commit with example values
fn is_example_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".example")
        || lower.ends_with(".sample")
        || lower.ends_with(".template")
        || lower.ends_with(".example.env")
        || lower.ends_with(".env.example")
        || lower.ends_with(".env.sample")
        || lower.ends_with(".env.template")
        || lower.contains(".example.")
        || lower.contains(".sample.")
}

/// Files that are inherently sensitive and should never be committed
fn is_sensitive_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    let filename = lower.split('/').next_back().unwrap_or(&lower);

    // Exact filenames
    matches!(filename,
        ".env" | ".envrc" | "secrets.json" | "secrets.yaml" | "secrets.yml" |
        "credentials.json" | "credentials.yml" | "credentials.yaml" |
        ".netrc" | ".npmrc" | ".pypirc"
    )
    // .env variants: .env.local, .env.production, etc.
    || (filename.starts_with(".env.") && !is_example_file(path))
    // Private key files
    || lower.ends_with("_rsa")
    || lower.ends_with("_ed25519")
    || lower.ends_with("_ecdsa")
    || lower.ends_with(".pem")
    || lower.ends_with(".p12")
    || lower.ends_with(".pfx")
    || lower.ends_with(".key")
    || lower.ends_with(".keystore")
    // Auth files
    || filename == "id_rsa"
    || filename == "id_ed25519"
    || filename == "id_ecdsa"
}

/// Binary-like or generated files to skip
fn should_skip_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".lock")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".ico")
        || lower.ends_with(".wasm")
        || lower.ends_with(".pdf")
        || lower.ends_with(".zip")
        || lower.contains("bun.lock")
        || lower.contains("package-lock")
        || lower.contains("yarn.lock")
        || lower.contains("/i18n/")
        || lower.contains("\\i18n\\")
}

/// Return list of paths staged vs HEAD (used by hooks/size guard)
pub fn staged_paths(repo_path: &Path) -> Result<Vec<String>> {
    use git2::Repository;
    let repo = Repository::discover(repo_path).map_err(crate::error::ToriiError::Git)?;
    let index = repo.index().map_err(crate::error::ToriiError::Git)?;
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = match &head_tree {
        Some(tree) => repo.diff_tree_to_index(Some(tree), Some(&index), None),
        None => repo.diff_tree_to_index(None, Some(&index), None),
    }
    .map_err(crate::error::ToriiError::Git)?;
    let mut out = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(p) = delta.new_file().path() {
                out.push(p.to_string_lossy().to_string());
            }
            true
        },
        None,
        None,
        None,
    )
    .map_err(crate::error::ToriiError::Git)?;
    Ok(out)
}

/// Return the name of the first built-in pattern that flags `line`, if any.
/// Reused by `history replace-text --redact-secrets` to redact matching lines
/// across history with the same detection logic the pre-save scanner uses.
pub(crate) fn matching_pattern(line: &str) -> Option<&'static str> {
    PATTERNS.iter().find(|p| (p.detect)(line)).map(|p| p.name)
}

/// Scan staged content using user-defined regex rules from .toriignore.
/// Returns findings — empty if no rules or no matches.
pub fn scan_staged_with_custom(
    repo_path: &Path,
    rules: &[crate::toriignore::SecretRule],
) -> Result<Vec<Finding>> {
    use git2::Repository;
    if rules.is_empty() {
        return Ok(Vec::new());
    }

    let mut findings = Vec::new();
    let repo = Repository::discover(repo_path).map_err(crate::error::ToriiError::Git)?;
    let index = repo.index().map_err(crate::error::ToriiError::Git)?;
    let paths = staged_paths(repo_path)?;

    for file_path in &paths {
        let p = std::path::Path::new(file_path);
        if is_example_file(file_path) || should_skip_file(file_path) {
            continue;
        }
        let entry = match index.get_path(p, 0) {
            Some(e) => e,
            None => continue,
        };
        let blob = match repo.find_blob(entry.id) {
            Ok(b) => b,
            Err(_) => continue,
        };
        if blob.size() > max_blob_bytes() {
            continue;
        }
        let content = String::from_utf8_lossy(blob.content()).to_string();

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // Same comment-skip as scan_staged — custom rules should not
            // false-positive on documentation/comments that mention the
            // very patterns they describe.
            if trimmed.starts_with('#')
                || trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                continue;
            }
            for rule in rules {
                if rule.regex.is_match(line) {
                    findings.push(Finding {
                        file: file_path.clone(),
                        line: i + 1,
                        pattern_name: format!("custom: {}", rule.name),
                        preview: mask(line.trim()),
                    });
                    break;
                }
            }
        }
    }
    Ok(findings)
}

/// Scan staged files in the git index for sensitive data.
/// Returns a list of findings.
pub fn scan_staged(repo_path: &Path) -> Result<Vec<Finding>> {
    use git2::Repository;

    let mut findings = Vec::new();

    let repo = Repository::discover(repo_path).map_err(crate::error::ToriiError::Git)?;
    let index = repo.index().map_err(crate::error::ToriiError::Git)?;

    // Walk staged entries (index vs HEAD diff gives us changed files)
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());

    let diff = match &head_tree {
        Some(tree) => repo.diff_tree_to_index(Some(tree), Some(&index), None),
        None => repo.diff_tree_to_index(None, Some(&index), None),
    }
    .map_err(crate::error::ToriiError::Git)?;

    let mut staged_files: Vec<String> = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path() {
                staged_files.push(path.to_string_lossy().to_string());
            }
            true
        },
        None,
        None,
        None,
    )
    .map_err(crate::error::ToriiError::Git)?;

    for file_path in &staged_files {
        let file_path_str = file_path.as_str();

        if is_example_file(file_path_str) || should_skip_file(file_path_str) {
            continue;
        }

        if is_sensitive_file(file_path_str) {
            findings.push(Finding {
                file: file_path.clone(),
                line: 0,
                pattern_name: "Sensitive file — should not be committed".to_string(),
                preview: format!("⚠  {} should not be tracked by version control", file_path),
            });
            continue;
        }

        // Read staged content from index blob
        let entry = index.get_path(std::path::Path::new(file_path_str), 0);
        let content = match entry {
            Some(e) => match repo.find_blob(e.id) {
                Ok(blob) => {
                    if blob.size() > max_blob_bytes() {
                        continue;
                    }
                    String::from_utf8_lossy(blob.content()).to_string()
                }
                Err(_) => continue,
            },
            None => continue,
        };

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#')
                || trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
            {
                continue;
            }

            for pattern in PATTERNS {
                if (pattern.detect)(line) {
                    let preview = mask(line.trim());
                    findings.push(Finding {
                        file: file_path.clone(),
                        line: line_num + 1,
                        pattern_name: pattern.name.to_string(),
                        preview,
                    });
                    break;
                }
            }
        }
    }

    Ok(findings)
}

/// Scan an entire git history for sensitive data (for migration use).
/// Returns findings grouped by commit.
pub fn scan_history(repo_path: &Path) -> Result<Vec<(String, Vec<Finding>)>> {
    use git2::Repository;

    let mut results = Vec::new();

    let repo = Repository::discover(repo_path).map_err(crate::error::ToriiError::Git)?;

    // Walk all commits reachable from any reference
    let mut revwalk = repo.revwalk().map_err(crate::error::ToriiError::Git)?;
    revwalk
        .push_glob("*")
        .map_err(crate::error::ToriiError::Git)?;

    let commits: Vec<(git2::Oid, String)> = revwalk
        .filter_map(|id| id.ok())
        .filter_map(|id| {
            repo.find_commit(id).ok().map(|c| {
                let subject = c.summary().unwrap_or("").to_string();
                (id, subject)
            })
        })
        .collect();

    println!("🔍 Scanning {} commits...", commits.len());

    for (oid, subject) in &commits {
        let commit = match repo.find_commit(*oid) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Get diff against first parent (or empty tree for root commits)
        let commit_tree = match commit.tree() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

        let diff = match repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut commit_findings = Vec::new();

        // For each changed file, read its content from the commit tree
        let mut changed_files: Vec<String> = Vec::new();
        let _ = diff.foreach(
            &mut |delta, _| {
                if let Some(path) = delta.new_file().path() {
                    changed_files.push(path.to_string_lossy().to_string());
                }
                true
            },
            None,
            None,
            None,
        );

        for file_path in &changed_files {
            if is_example_file(file_path) || should_skip_file(file_path) {
                continue;
            }

            // Read file content from this commit's tree
            let entry = commit_tree.get_path(std::path::Path::new(file_path));
            let content = match entry {
                Ok(e) => match repo.find_blob(e.id()) {
                    Ok(blob) => {
                        if blob.size() > max_blob_bytes() {
                            continue;
                        }
                        String::from_utf8_lossy(blob.content()).to_string()
                    }
                    Err(_) => continue,
                },
                Err(_) => continue,
            };

            for (line_num, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with('#')
                    || trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with('*')
                {
                    continue;
                }

                for pattern in PATTERNS {
                    if (pattern.detect)(line) {
                        commit_findings.push(Finding {
                            file: file_path.clone(),
                            line: line_num + 1,
                            pattern_name: pattern.name.to_string(),
                            preview: mask(line.trim()),
                        });
                        break;
                    }
                }
            }
        }

        if !commit_findings.is_empty() {
            results.push((
                format!("{} — {}", &oid.to_string()[..8], subject),
                commit_findings,
            ));
        }
    }

    Ok(results)
}

#[cfg(test)]
mod pattern_audit_tests {
    use super::matching_pattern;

    // ---- Real DB-connection-string credentials (the case that started
    // this audit — already handled by the "Database connection string
    // with credentials" pattern; kept here as a regression lock so a
    // future refactor can't silently regress it). ----

    #[test]
    fn postgres_url_with_plaintext_password_is_flagged() {
        // Split like the save_no_tty.rs fixture does, so this source file
        // doesn't itself trip the scanner during `torii save`.
        let scheme = concat!("postgresql", "://");
        let creds_and_host = "quota:s3cr3tPass9@postgres:5432/quota_edge";
        let line = format!(r#"        "-e", "DATABASE_URL={scheme}{creds_and_host}","#);
        assert!(matching_pattern(&line).is_some());
    }

    #[test]
    fn db_url_without_credentials_is_not_flagged() {
        assert!(matching_pattern("postgresql://postgres:5432/quota_edge").is_none());
        assert!(matching_pattern("DATABASE_URL=postgres://host:5432/db").is_none());
        assert!(matching_pattern("https://example.com/callback").is_none());
    }

    #[test]
    fn url_with_user_but_no_password_is_not_flagged() {
        // user@host with no `:password` segment — nothing secret to leak.
        assert!(matching_pattern("ssh://git@github.com/org/repo.git").is_none());
        assert!(matching_pattern("postgresql://quota@postgres:5432/quota_edge").is_none());
    }

    // ---- Reported gap: "postgres://" (the short/libpq form — the one
    // Heroku, Render, sqlx, psycopg, node-postgres etc. actually emit) was
    // entirely missing from the recognized scheme list, so a real leaked
    // credential under that spelling was invisible no matter how long the
    // password was. There is no length minimum anywhere in this check —
    // verified by testing 1-character user/password/host against a scheme
    // that *is* recognized (`postgresql://`), which matches fine. The
    // apparent "short credentials slip through" symptom was entirely the
    // missing scheme alias, not a threshold on any of the three fields. ----

    #[test]
    fn short_postgres_scheme_with_any_length_credentials_is_flagged() {
        let scheme = concat!("postgres", "://");
        // Single-character user, password, host, db — the shortest
        // possible real credential.
        assert!(matching_pattern(&format!("{scheme}u:p@h/d")).is_some());
        // The exact case reported: short scheme alias + short everything.
        let creds = "u:p4ssw0rd@h:5432/d";
        assert!(matching_pattern(&format!("{scheme}{creds}")).is_some());
    }

    #[test]
    fn no_length_minimum_on_user_password_or_host() {
        // Regression lock: each of user/password/host individually at
        // 1 character, under the scheme that already worked before this
        // fix — confirms the detector never had a length gate to begin
        // with, only a missing-scheme gap.
        let scheme = concat!("postgresql", "://");
        assert!(matching_pattern(&format!("{scheme}u:p@h/d")).is_some());
    }

    #[test]
    fn mongodb_atlas_srv_scheme_is_flagged() {
        // "mongodb+srv://" (MongoDB Atlas' standard connection-string
        // form) does not contain "mongodb://" as a contiguous substring
        // (the "+srv" sits in between), so it was equally invisible.
        let scheme = concat!("mongodb+srv", "://");
        let creds = "dbuser:S3cr3tAtlas@cluster0.abcde.mongodb.net/mydb";
        assert!(matching_pattern(&format!("{scheme}{creds}")).is_some());
    }

    // ---- Policy: mark documentation-style example URLs rather than
    // allowlisting guessable password words.
    //
    // A hardcoded exclusion list ("password", "pass", "changeme", "root",
    // "xxx", ...) is exactly the kind of list someone's *real* (if weak)
    // password eventually matches — the same "shorter is worse, not
    // safer" logic that motivated fixing the missing-scheme gap above
    // applies here too. So this scanner only excludes *syntactic*
    // placeholders that can never be a real leaked value no matter what
    // — a template reference (`${PASSWORD}`, `$PASSWORD`) or a
    // placeholder marker (`<password>`) — and flags literal word guesses
    // like "password"/"pass"/"root"/"changeme". A README with
    // `user:pass@localhost` in an example will get flagged; that's a
    // false positive its author clears with `--yes` or silences for good
    // with `.toriignore`'s `[secrets]` allowlist — cheaper than a real
    // "changeme" or "root" password shipped in a container arg slipping
    // through because it happened to spell a word on the exclusion list.

    #[test]
    fn doc_style_example_urls_with_guessable_passwords_are_flagged() {
        let scheme = concat!("postgres", "://");
        assert!(matching_pattern(&format!("{scheme}user:pass@localhost/db")).is_some());
        let mysql_scheme = concat!("mysql", "://");
        assert!(matching_pattern(&format!("{mysql_scheme}root:root@127.0.0.1/test")).is_some());
        let word = concat!("change", "me");
        assert!(matching_pattern(&format!("{scheme}admin:{word}@localhost/db")).is_some());
    }

    #[test]
    fn syntactic_placeholders_are_still_not_flagged() {
        // These can never be a real leaked value — they're a reference to
        // config elsewhere, not a value at all.
        assert!(matching_pattern("postgresql://user:${DB_PASSWORD}@host/db").is_none());
        assert!(matching_pattern("postgresql://user:$DB_PASSWORD@host/db").is_none());
        assert!(matching_pattern("postgresql://user:<password>@host/db").is_none());
    }

    // ---- PEM private keys ----

    // The PEM marker is split across a variable + a separate literal on
    // each assert line, so no single raw source line ever carries both
    // "-----BEGIN" and "PRIVATE KEY" together — exactly what the pattern
    // requires to fire.
    #[test]
    fn pem_rsa_private_key_is_flagged() {
        let begin = "-----BEGIN";
        assert!(matching_pattern(&format!("{begin} RSA PRIVATE KEY-----")).is_some());
    }

    #[test]
    fn pem_openssh_private_key_is_flagged() {
        let begin = "-----BEGIN";
        assert!(matching_pattern(&format!("{begin} OPENSSH PRIVATE KEY-----")).is_some());
    }

    #[test]
    fn pem_encrypted_and_pgp_private_keys_are_flagged() {
        let begin = "-----BEGIN";
        assert!(matching_pattern(&format!("{begin} ENCRYPTED PRIVATE KEY-----")).is_some());
        assert!(matching_pattern(&format!("{begin} PGP PRIVATE KEY BLOCK-----")).is_some());
    }

    // ---- AWS access keys ----

    #[test]
    fn aws_akia_key_is_flagged() {
        // Split like the save_no_tty.rs fixture does, so this source file
        // doesn't itself trip the scanner during `torii save`.
        let key = concat!("AKIA", "IOSFODNN7EXAMPLE");
        let line = format!("aws_access_key_id = {}", key);
        assert!(matching_pattern(&line).is_some());
    }

    // ---- GitHub / GitLab tokens ----

    #[test]
    fn github_classic_pat_is_flagged() {
        let var_name = "GITHUB_TOKEN";
        let token = concat!("ghp_", "1234567890abcdefghijklmnopqrstuvwxyz");
        let line = format!("{var_name}={token}");
        assert!(matching_pattern(&line).is_some());
    }

    #[test]
    fn gitlab_pat_is_flagged() {
        let var_name = "GITLAB_TOKEN";
        let token = concat!("glpat-", "abcdefghijklmnopqrst12");
        let line = format!("{var_name}={token}");
        assert!(matching_pattern(&line).is_some());
    }

    // ---- Same "glued to KEY=value, no surrounding spaces" shape,
    // checked against every other prefix-based pattern in the file — the
    // GitHub/GitLab gap above isn't a one-off, it's the same root cause
    // (prefix match requires the *whole* whitespace-delimited word to
    // start with the prefix) wherever it's used. ----

    #[test]
    fn aws_key_glued_to_assignment_with_no_spaces_is_flagged() {
        let key = concat!("AKIA", "IOSFODNN7EXAMPLE");
        let line = format!("AWS_ACCESS_KEY_ID={}", key);
        assert!(matching_pattern(&line).is_some());
    }

    #[test]
    fn stripe_key_glued_to_assignment_with_no_spaces_is_flagged() {
        let var_name = concat!("STRIPE_SECRET", "_KEY");
        let token = concat!("sk_live_", "51H8xyzABCDEFGHIJKLMNOPQRSTUVWXYZ");
        let line = format!("{var_name}={token}");
        assert!(matching_pattern(&line).is_some());
    }

    #[test]
    fn sendgrid_key_glued_to_assignment_with_no_spaces_is_flagged() {
        let var_name = concat!("SENDGRID_API", "_KEY");
        let token = concat!(
            "SG.",
            "abcdefghijklmnopqrstuv.wxyzABCDEFGHIJKLMNOPQRSTUVWXYZ123456"
        );
        let line = format!("{var_name}={token}");
        assert!(matching_pattern(&line).is_some());
    }

    #[test]
    fn bare_token_prefixes_in_docs_are_not_flagged() {
        // Prefix-only mentions (docs/help text) — no real secret present.
        assert!(matching_pattern("torii auth set github ghp_xxx").is_none());
        assert!(matching_pattern("torii auth set gitlab glpat-xxx").is_none());
    }

    // ---- Bearer tokens with a literal value ----
    //
    // GAP found by this audit: a bare `Bearer <literal>` header value with
    // no recognizable vendor prefix (not a JWT, not sk_live_, not ghp_/
    // glpat-) was not covered by any pattern — "Generic API key / token"
    // only fires on a `key_word = value` shape, and "Authorization" /
    // "Bearer" aren't in its keyword list.

    #[test]
    fn bearer_with_opaque_literal_token_is_flagged() {
        let word = concat!("Bear", "er ");
        let token = concat!("4f8a9c2e1b3d7f6a", "5e0c9b8a7d6e5f4c3b2a1908");
        let line = format!(r#"req.Header.Set("Authorization", "{word}{token}")"#);
        assert!(
            matching_pattern(&line).is_some(),
            "a literal Bearer token should be flagged"
        );
    }

    #[test]
    fn bearer_placeholders_are_not_flagged() {
        assert!(matching_pattern("Authorization: Bearer <YOUR_TOKEN>").is_none());
        assert!(matching_pattern("Authorization: Bearer ${API_TOKEN}").is_none());
        assert!(matching_pattern("Authorization: Bearer $TOKEN").is_none());
        assert!(matching_pattern("Authorization: Bearer your_token_here_1234567890").is_none());
        assert!(matching_pattern("curl -H 'Authorization: Bearer xxx'").is_none());
    }

    // ---- False positive: typed field declarations, no value present ----
    //
    // `pub brevo_api_key: Arc<RwLock<String>>` was flagged as "Generic API
    // key / token" even though it holds no secret — it's a Rust struct
    // field declaration. The rule keyed off `api_key` + `:` + a
    // long/no-space "value", and `Arc<RwLock<String>>` happens to satisfy
    // all of those.

    #[test]
    fn rust_type_declaration_is_not_flagged_as_a_secret() {
        assert!(
            matching_pattern("pub brevo_api_key: Arc<RwLock<String>>,").is_none(),
            "a type declaration must not be flagged as a secret value"
        );
        assert!(matching_pattern("api_key: Option<String>,").is_none());
        assert!(matching_pattern("secret_key: Box<dyn SecretStore>,").is_none());
    }

    #[test]
    fn real_generic_api_key_assignment_is_still_flagged() {
        // The fix for the type-declaration false positive must not blind
        // the rule to actual secrets.
        assert!(matching_pattern(r#"api_key = "sk_9f8e7d6c5b4a3210deadbeef""#).is_some());
        assert!(matching_pattern(r#"password: "Tr0ub4dor&3xtraLong""#).is_some());
    }
}
