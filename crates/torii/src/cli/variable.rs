//! `torii variable` — CI/CD variables and secrets.
//!
//! The surface is small on purpose. What it adds over the platform's web
//! UI is the two checks that are easy to get wrong there and expensive to
//! debug afterwards: masking is validated before the write instead of
//! coming back as a generic 400, and a protected variable warns when the
//! project has no protected ref for it to reach.

use crate::pr::detect_platform_from_remote_named;
use crate::variable::{VariableKind, VariableWrite};
use anyhow::{anyhow, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum VariableCommands {
    /// List the project's variables. Values are elided unless you ask.
    List {
        /// Print values. They land in your scrollback and, if you are
        /// recording or screen-sharing, somewhere less forgiving.
        #[arg(long)]
        show_values: bool,
    },

    /// Show one variable.
    Get {
        key: String,
        /// Environment scope, when the key is defined more than once.
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        show_value: bool,
    },

    /// Create or replace a variable.
    Set {
        key: String,
        /// The value, inline. Convenient and not private: everything in
        /// argv is readable by any process on the machine. Fine for a
        /// build flag, wrong for a credential — use --from-file or
        /// --stdin for those.
        #[arg(long, conflicts_with_all = ["from_file", "stdin"])]
        value: Option<String>,
        /// Read the value from a file. The right way in for an SSH key
        /// or a certificate, which have to keep their newlines anyway.
        #[arg(long, conflicts_with_all = ["value", "stdin"])]
        from_file: Option<std::path::PathBuf>,
        /// Read the value from standard input.
        #[arg(long, conflicts_with_all = ["value", "from_file"])]
        stdin: bool,

        /// Store as a file variable: the job gets a path, not the
        /// content. Required for anything multi-line.
        #[arg(long)]
        file: bool,
        /// Only expose it to pipelines on protected branches and tags.
        #[arg(long)]
        protected: bool,
        /// Ask the platform to strike the value out of job logs.
        #[arg(long)]
        masked: bool,
        /// Let the platform expand `$OTHER_VAR` inside the value. Off by
        /// default, because an expanded fragment in a credential is a
        /// credential that silently stops working.
        #[arg(long)]
        expand: bool,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },

    /// Delete a variable.
    #[command(alias = "remove")]
    Rm {
        key: String,
        #[arg(long)]
        scope: Option<String>,
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// Read the value for `set` from whichever of the three inputs was given.
fn read_value(
    value: &Option<String>,
    from_file: &Option<std::path::PathBuf>,
    stdin: bool,
) -> Result<String> {
    match (value, from_file, stdin) {
        (Some(v), _, _) => Ok(v.clone()),
        (_, Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Could not read {}: {}", path.display(), e)),
        (_, _, true) => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        _ => Err(anyhow!(
            "No value given. Pass one of --value, --from-file or --stdin."
        )),
    }
}

fn elide(value: Option<&String>) -> String {
    match value {
        None => "<not disclosed>".to_string(),
        Some(v) if v.is_empty() => "<empty>".to_string(),
        Some(v) => format!("<{} chars>", v.chars().count()),
    }
}

pub(crate) fn run(action: &VariableCommands, remote: &String) -> Result<()> {
    let repo_path = std::env::current_dir()?.to_string_lossy().to_string();
    let (platform, owner, repo_name) = detect_platform_from_remote_named(&repo_path, remote)
        .ok_or_else(|| anyhow!("Could not detect platform from remote `{}`.", remote))?;
    let client = crate::variable::get_variable_client(&platform)?;

    match action {
        VariableCommands::List { show_values } => {
            let vars = client.list(&owner, &repo_name)?;
            if vars.is_empty() {
                println!(
                    "No variables set on {} for {}/{}.",
                    platform, owner, repo_name
                );
                return Ok(());
            }
            println!("{:<34} {:<8} {:<22} FLAGS", "KEY", "TYPE", "VALUE");
            for v in &vars {
                let mut flags = Vec::new();
                if v.protected {
                    flags.push("protected");
                }
                if v.masked {
                    flags.push("masked");
                }
                if v.hidden {
                    flags.push("hidden");
                }
                if !v.raw {
                    flags.push("expanded");
                }
                if let Some(s) = &v.scope {
                    flags.push(s.as_str());
                }
                let shown = if *show_values {
                    v.value
                        .clone()
                        .unwrap_or_else(|| "<not disclosed>".to_string())
                } else {
                    elide(v.value.as_ref())
                };
                println!(
                    "{:<34} {:<8} {:<22} {}",
                    v.key,
                    match v.kind {
                        VariableKind::File => "file",
                        VariableKind::EnvVar => "env",
                    },
                    shown,
                    if flags.is_empty() {
                        "—".to_string()
                    } else {
                        flags.join(",")
                    }
                );
            }
            Ok(())
        }

        VariableCommands::Get {
            key,
            scope,
            show_value,
        } => {
            let v = client.get(&owner, &repo_name, key, scope.as_deref())?;
            println!("Key:         {}", v.key);
            println!(
                "Type:        {}",
                match v.kind {
                    VariableKind::File => "file (the job receives a path)",
                    VariableKind::EnvVar => "env_var",
                }
            );
            println!("Protected:   {}", v.protected);
            println!("Masked:      {}", v.masked);
            println!("Raw:         {}", v.raw);
            println!("Scope:       {}", v.scope.as_deref().unwrap_or("* (all)"));
            if let Some(d) = &v.description {
                println!("Description: {}", d);
            }
            println!(
                "Value:       {}",
                if *show_value {
                    v.value
                        .clone()
                        .unwrap_or_else(|| "<not disclosed>".to_string())
                } else {
                    elide(v.value.as_ref())
                }
            );
            Ok(())
        }

        VariableCommands::Set {
            key,
            value,
            from_file,
            stdin,
            file,
            protected,
            masked,
            expand,
            scope,
            description,
        } => {
            let raw_value = read_value(value, from_file, *stdin)?;

            // Refuse an unmaskable value here rather than letting the
            // platform answer 400 with no reason. The common case is a
            // PEM key: multi-line, therefore unmaskable, and the useful
            // answer is "store it as a file variable", not "invalid".
            if *masked {
                if let Some(reason) = client.masking_rejection(&raw_value) {
                    return Err(anyhow!(
                        "{} cannot be masked on {}: {}.\n\
                         Masking is not the only protection — an unmasked variable is still \
                         only readable by jobs — but do not ask for it and assume you got it.",
                        key,
                        platform,
                        reason
                    ));
                }
            }

            // Multi-line content in an environment variable arrives
            // mangled. This is the failure that costs an afternoon,
            // because the write succeeds and the job fails later.
            if !*file && raw_value.contains('\n') {
                return Err(anyhow!(
                    "{} spans several lines, so it has to be a file variable. \
                     Re-run with --file; the job then receives a path to a temporary \
                     file holding the content.",
                    key
                ));
            }

            let write = VariableWrite {
                key: key.clone(),
                value: raw_value,
                kind: if *file {
                    VariableKind::File
                } else {
                    VariableKind::EnvVar
                },
                protected: *protected,
                masked: *masked,
                raw: !*expand,
                scope: scope.clone(),
                description: description.clone(),
            };

            let saved = client.set(&owner, &repo_name, &write)?;
            println!("✅ {} saved on {}/{}.", saved.key, owner, repo_name);

            // A protected variable is only handed to pipelines running on
            // a protected ref. On a project with no protected tag, a
            // release pipeline never sees it, the job that needed it is
            // skipped by its own rule, and the pipeline stays green —
            // which is the hardest shape of failure to notice.
            if *protected {
                println!(
                    "ℹ️  Protected: only pipelines on a protected branch or tag receive it. \
                     Check the project has one that matches — for tag pipelines that means \
                     protecting the tag pattern, not just the default branch."
                );
            }
            Ok(())
        }

        VariableCommands::Rm { key, scope, yes } => {
            if !*yes {
                print!(
                    "Delete variable {} on {}/{}? Jobs that read it start failing. [y/N] ",
                    key, owner, repo_name
                );
                use std::io::Write;
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("❌ Cancelled.");
                    return Ok(());
                }
            }
            client.remove(&owner, &repo_name, key, scope.as_deref())?;
            println!("🗑️  {} deleted.", key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_value_prefers_the_inline_value() {
        let v = read_value(&Some("inline".into()), &None, false).unwrap();
        assert_eq!(v, "inline");
    }

    #[test]
    fn read_value_reads_a_file_whole_including_its_trailing_newline() {
        // The trailing newline matters: an SSH key without it is rejected
        // by ssh with an error that does not mention the newline.
        let dir = std::env::temp_dir().join("torii-var-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        std::fs::write(&path, "line one\nline two\n").unwrap();
        let v = read_value(&None, &Some(path.clone()), false).unwrap();
        assert_eq!(v, "line one\nline two\n");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn read_value_names_the_file_it_could_not_read() {
        let missing = std::path::PathBuf::from("/nonexistent/torii/variable/value");
        let err = read_value(&None, &Some(missing), false).unwrap_err();
        assert!(
            err.to_string()
                .contains("/nonexistent/torii/variable/value"),
            "got: {err}"
        );
    }

    #[test]
    fn read_value_with_no_input_says_which_flags_exist() {
        let err = read_value(&None, &None, false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("--value"), "got: {msg}");
        assert!(msg.contains("--from-file"), "got: {msg}");
        assert!(msg.contains("--stdin"), "got: {msg}");
    }

    #[test]
    fn elide_distinguishes_empty_from_undisclosed() {
        assert_eq!(elide(None), "<not disclosed>");
        assert_eq!(elide(Some(&String::new())), "<empty>");
        assert_eq!(elide(Some(&"abcd".to_string())), "<4 chars>");
    }
}
