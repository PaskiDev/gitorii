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
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                (w.starts_with("AKIA") || w.starts_with("ASIA") || w.starts_with("AROA"))
                    && w.len() == 20
                    && w.chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
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
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
                // Skip obvious placeholder/example tokens
                if w.ends_with("xxx") || w.ends_with("_xxx") || w.contains("xxxx") {
                    return false;
                }
                // Skip bare prefixes used in docs (ghp_, glpat-, etc.) — real tokens are longer
                let is_prefix_only = (w.starts_with("ghp_") && w.len() <= 5)
                    || (w.starts_with("gho_") && w.len() <= 5)
                    || (w.starts_with("ghs_") && w.len() <= 5)
                    || (w.starts_with("glpat-") && w.len() <= 7)
                    || (w.starts_with("glptt-") && w.len() <= 7)
                    || (w.starts_with("github_pat_") && w.len() <= 12);
                if is_prefix_only {
                    return false;
                }
                (w.starts_with("ghp_")
                    || w.starts_with("gho_")
                    || w.starts_with("ghs_")
                    || w.starts_with("github_pat_")
                    || w.starts_with("glpat-")
                    || w.starts_with("glptt-"))
                    && w.len() > 20
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
            let lower = l.to_lowercase();
            (lower.contains("postgresql://")
                || lower.contains("mysql://")
                || lower.contains("mongodb://")
                || lower.contains("redis://")
                || lower.contains("libsql://")
                || lower.contains("turso://"))
                && l.contains('@')
                && !l.contains("user:password@")
                && !l.contains("user:pass@")
                && !l.contains("<password>")
        },
    },
    Pattern {
        name: "Stripe key",
        detect: |l| {
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
                (w.starts_with("sk_live_")
                    || w.starts_with("pk_live_")
                    || w.starts_with("rk_live_"))
                    && w.len() > 16
            })
        },
    },
    Pattern {
        name: "Twilio / SendGrid / Brevo key",
        detect: |l| {
            l.split_whitespace().any(|w| {
                let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
                // SG. prefix = SendGrid
                (w.starts_with("SG.") && w.len() > 40) ||
                // AC... = Twilio account SID
                (w.starts_with("AC") && w.len() == 34 && w.chars().all(|c| c.is_ascii_alphanumeric()))
            })
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
