//! `torii history orphans` — unreachable-object recovery (fsck).

use anyhow::Result;

/// Walk the object database, mark everything reachable from refs + reflogs +
/// the index + HEAD, then list / inspect / restore the leftover unreachable
/// objects. Recovery aid after destructive ops (reset --hard, force-push,
/// rebase that drops commits, etc.).
pub(crate) fn run_fsck(
    show: Option<&str>,
    restore: Option<&str>,
    to: Option<&std::path::Path>,
) -> Result<()> {
    use std::collections::HashSet;
    let repo = git2::Repository::discover(".").map_err(|e| anyhow::anyhow!("not a repo: {}", e))?;

    // --- branch: --show <oid>
    if let Some(oid_str) = show {
        let oid = resolve_oid(&repo, oid_str)?;
        let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
        let obj = odb
            .read(oid)
            .map_err(|e| anyhow::anyhow!("read {}: {}", oid, e))?;
        match obj.kind() {
            git2::ObjectType::Blob => {
                use std::io::Write;
                std::io::stdout().write_all(obj.data()).ok();
            }
            git2::ObjectType::Commit => {
                let commit = repo
                    .find_commit(oid)
                    .map_err(|e| anyhow::anyhow!("find commit {}: {}", oid, e))?;
                println!("commit {}", oid);
                if let Some(t) = commit.tree_id().to_string().get(..) {
                    println!("tree   {}", t);
                }
                for p in commit.parent_ids() {
                    println!("parent {}", p);
                }
                let a = commit.author();
                println!(
                    "author {} <{}>",
                    a.name().unwrap_or(""),
                    a.email().unwrap_or("")
                );
                println!();
                println!("{}", commit.message().unwrap_or(""));
            }
            git2::ObjectType::Tree => {
                let tree = repo
                    .find_tree(oid)
                    .map_err(|e| anyhow::anyhow!("find tree {}: {}", oid, e))?;
                println!("tree {} ({} entries)", oid, tree.len());
                for e in tree.iter() {
                    println!(
                        "  {:o} {} {}",
                        e.filemode(),
                        e.id(),
                        e.name().unwrap_or("?")
                    );
                }
            }
            other => println!("object {} kind={:?} size={}", oid, other, obj.len()),
        }
        return Ok(());
    }

    // --- branch: --restore <oid> --to <path>
    if let Some(oid_str) = restore {
        let dest = to.ok_or_else(|| anyhow::anyhow!("--restore requires --to <path>"))?;
        let oid = resolve_oid(&repo, oid_str)?;
        let blob = repo
            .find_blob(oid)
            .map_err(|e| anyhow::anyhow!("not a blob {}: {}", oid, e))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(dest, blob.content())
            .map_err(|e| anyhow::anyhow!("write {}: {}", dest.display(), e))?;
        println!(
            "✅ Restored {} bytes from {} → {}",
            blob.content().len(),
            oid,
            dest.display()
        );
        return Ok(());
    }

    // --- default: list unreachable
    let mut reachable: HashSet<git2::Oid> = HashSet::new();

    // Refs (branches, tags, remotes)
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if let Some(target) = r.target() {
                mark_commit_tree(&repo, target, &mut reachable);
            }
        }
    }
    // HEAD (covers detached HEAD case)
    if let Ok(head) = repo.head() {
        if let Some(target) = head.target() {
            mark_commit_tree(&repo, target, &mut reachable);
        }
    }
    // Reflog of HEAD + every branch — protects work that survived
    // ref deletion but still has a reflog entry.
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            let Some(name) = r.name() else { continue };
            if let Ok(rl) = repo.reflog(name) {
                for entry in rl.iter() {
                    mark_commit_tree(&repo, entry.id_old(), &mut reachable);
                    mark_commit_tree(&repo, entry.id_new(), &mut reachable);
                }
            }
        }
    }
    if let Ok(rl) = repo.reflog("HEAD") {
        for entry in rl.iter() {
            mark_commit_tree(&repo, entry.id_old(), &mut reachable);
            mark_commit_tree(&repo, entry.id_new(), &mut reachable);
        }
    }
    // Index — protects staged blobs not yet committed
    if let Ok(index) = repo.index() {
        for e in index.iter() {
            reachable.insert(e.id);
        }
    }

    // Walk ODB, collect unreachable.
    let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
    let mut unreachable: Vec<(git2::Oid, git2::ObjectType, usize)> = Vec::new();
    odb.foreach(|oid| {
        if !reachable.contains(oid) {
            if let Ok(obj) = odb.read(*oid) {
                unreachable.push((*oid, obj.kind(), obj.len()));
            }
        }
        true
    })
    .map_err(|e| anyhow::anyhow!("odb walk: {}", e))?;

    if unreachable.is_empty() {
        println!("✅ No unreachable objects.");
        return Ok(());
    }

    // Sort: commits first, then trees, then blobs by size desc
    unreachable.sort_by(|a, b| {
        let ka = type_rank(a.1);
        let kb = type_rank(b.1);
        ka.cmp(&kb).then(b.2.cmp(&a.2))
    });

    let total: usize = unreachable.iter().map(|(_, _, s)| *s).sum();
    println!(
        "🔍 {} unreachable object(s), {} bytes total\n",
        unreachable.len(),
        total
    );
    println!("{:<8} {:7} {:>10}  preview", "type", "oid", "size");
    println!("{}", "─".repeat(60));

    for (oid, kind, size) in &unreachable {
        let short: String = oid.to_string().chars().take(7).collect();
        let kind_str = match kind {
            git2::ObjectType::Commit => "commit",
            git2::ObjectType::Tree => "tree",
            git2::ObjectType::Blob => "blob",
            git2::ObjectType::Tag => "tag",
            _ => "any",
        };
        let preview = preview_object(&repo, *oid, *kind);
        println!("{:<8} {:7} {:>10}  {}", kind_str, short, size, preview);
    }
    println!();
    println!("Inspect: torii history fsck --show <oid>");
    println!("Restore: torii history fsck --restore <oid> --to <path>");
    Ok(())
}

/// Resolve a (possibly short) hex OID to a full Oid by walking the ODB.
/// Accepts 4..=40 hex chars, errors on ambiguous prefixes.
fn resolve_oid(repo: &git2::Repository, hex: &str) -> Result<git2::Oid> {
    if hex.len() == 40 {
        return git2::Oid::from_str(hex).map_err(|e| anyhow::anyhow!("bad oid {}: {}", hex, e));
    }
    if hex.len() < 4 {
        anyhow::bail!("oid prefix too short (need ≥4 chars): {}", hex);
    }
    let odb = repo.odb().map_err(|e| anyhow::anyhow!("odb: {}", e))?;
    let mut matches: Vec<git2::Oid> = Vec::new();
    odb.foreach(|oid| {
        if oid.to_string().starts_with(hex) {
            matches.push(*oid);
        }
        true
    })
    .map_err(|e| anyhow::anyhow!("odb walk: {}", e))?;
    match matches.len() {
        0 => anyhow::bail!("no object matches prefix {}", hex),
        1 => Ok(matches[0]),
        n => anyhow::bail!("ambiguous prefix {} ({} matches)", hex, n),
    }
}

fn type_rank(t: git2::ObjectType) -> u8 {
    match t {
        git2::ObjectType::Commit => 0,
        git2::ObjectType::Tag => 1,
        git2::ObjectType::Tree => 2,
        git2::ObjectType::Blob => 3,
        _ => 4,
    }
}

fn mark_commit_tree(
    repo: &git2::Repository,
    oid: git2::Oid,
    set: &mut std::collections::HashSet<git2::Oid>,
) {
    if !set.insert(oid) {
        return;
    }
    let Ok(obj) = repo.find_object(oid, None) else {
        return;
    };
    match obj.kind() {
        Some(git2::ObjectType::Commit) => {
            if let Ok(commit) = obj.peel_to_commit() {
                set.insert(commit.tree_id());
                if let Ok(tree) = commit.tree() {
                    mark_tree(repo, &tree, set);
                }
                for p in commit.parent_ids() {
                    mark_commit_tree(repo, p, set);
                }
            }
        }
        Some(git2::ObjectType::Tag) => {
            if let Ok(tag) = obj.peel_to_tag() {
                mark_commit_tree(repo, tag.target_id(), set);
            }
        }
        Some(git2::ObjectType::Tree) => {
            if let Ok(tree) = obj.peel_to_tree() {
                mark_tree(repo, &tree, set);
            }
        }
        _ => {}
    }
}

fn mark_tree(
    repo: &git2::Repository,
    tree: &git2::Tree,
    set: &mut std::collections::HashSet<git2::Oid>,
) {
    for entry in tree.iter() {
        let id = entry.id();
        if !set.insert(id) {
            continue;
        }
        if entry.kind() == Some(git2::ObjectType::Tree) {
            if let Ok(sub) = repo.find_tree(id) {
                mark_tree(repo, &sub, set);
            }
        }
    }
}

fn preview_object(repo: &git2::Repository, oid: git2::Oid, kind: git2::ObjectType) -> String {
    match kind {
        git2::ObjectType::Commit => repo
            .find_commit(oid)
            .ok()
            .and_then(|c| c.summary().map(|s| s.to_string()))
            .unwrap_or_default(),
        git2::ObjectType::Blob => repo
            .find_blob(oid)
            .ok()
            .and_then(|b| std::str::from_utf8(b.content()).ok().map(|s| s.to_string()))
            .map(|s| s.lines().next().unwrap_or("").chars().take(50).collect())
            .unwrap_or_else(|| "<binary>".to_string()),
        git2::ObjectType::Tree => repo
            .find_tree(oid)
            .ok()
            .map(|t| format!("({} entries)", t.len()))
            .unwrap_or_default(),
        _ => String::new(),
    }
}
