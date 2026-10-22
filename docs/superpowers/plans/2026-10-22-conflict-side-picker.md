# Elegir lado en conflictos — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** cuando `sync --merge|--rebase`, `history rebase <base>` o
`cherry-pick` dejan conflictos, torii pregunta qué lado gana (todo el merge
o fichero a fichero) y resuelve solo los trozos en conflicto; con
`--local`/`--remote` lo hace sin preguntar.

**Architecture:** módulo nuevo `torii_lib::vcs::conflict` en `torii-core`:
lista los conflictos, traduce local/remoto a ours/theirs según la operación,
resuelve cada fichero con git2 (`merge_file_from_index` con `favor`) y
define un trait `ConflictResolver` que decide. `merge_branch`,
`rebase_branch` y `cherry_pick` reciben un resolver y siguen solos si no
queda conflicto. La CLI aporta el resolver de terminal (prompt + selector
con `crossterm`); su estado es una máquina pura, probable sin tty.

**Tech Stack:** Rust, git2 0.19, crossterm 0.28 (ya en `crates/torii`),
clap. Sin dependencias nuevas.

**Spec:** `docs/superpowers/specs/2026-10-22-conflict-side-picker-design.md`

## Global Constraints

- Sin dependencias nuevas en ningún `Cargo.toml`.
- Control de versiones solo con `torii` (`torii save <ficheros> -m "..."`), nunca `git`; sin trailer `Co-Authored-By`.
- Compilar y testear con `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=33554432`.
- torii-core compila también para `x86_64-pc-windows-gnu`: nada de `std::os::unix` sin `#[cfg(unix)]`.
- Mensajes y ayuda en inglés.
- Nunca se elige un lado por defecto: sin tty y sin flag → comportamiento de hoy (marcadores + mensaje "Resolve … and run …").
- "Gana un lado" = solo los trozos en conflicto; los cambios del otro lado que no chocan se conservan.
- Conflicto sin trozos (binario, borrado/modificado, falta un lado) → fichero entero del lado elegido; si ese lado no lo tiene, se borra.
- Local = trabajo del usuario, remoto = lo que llega. Merge y cherry-pick: local=ours, remoto=theirs. Rebase: local=theirs, remoto=ours.
- Cancelar (`m`, `Esc`, `q`) deja el repo exactamente como hoy.

## Review Focus

1. Rebase con conflicto: "gana local" debe dejar el contenido del commit propio, no el de upstream (inversión ours/theirs). → Task 1 (`rebase_local_means_own_commit`) y Task 2.
2. Fichero con un conflicto y además un cambio remoto en otra zona: con "gana local" el cambio remoto no conflictivo sigue ahí. → Task 1.
3. Borrado en un lado y modificado en el otro, y fichero binario: no revienta `merge_file_from_index`; gana el fichero entero (o el borrado). → Task 1.
4. Sin tty (`stdin` redirigido) y sin flag: marcadores y mensaje de hoy, y `sync` no imprime "✅ Merged" cuando quedó conflicto. → Tasks 2 y 4.
5. Rebase de varios commits con conflictos en dos de ellos: se pregunta en cada parada y, con `--remote`, termina el rebase entero. → Task 2.

## Estructura de ficheros

```
crates/torii-core/src/vcs/conflict.rs     Side, OpKind, ConflictFile, ConflictContext,
                                          Resolution, ConflictResolver, ManualResolver,
                                          FixedResolver, list_conflicts, resolve_files
crates/torii/src/cli/conflict_picker.rs   PickerState (máquina pura), TerminalResolver
                                          (prompt + selector crossterm)
crates/torii/tests/conflict_side.rs       tests del binario sin tty
```

Se modifican: `crates/torii-core/src/vcs/mod.rs` (`pub mod conflict;`),
`crates/torii-core/src/vcs/core_extensions.rs` (`merge_branch`,
`rebase_branch`), `crates/torii-core/src/vcs/core_tag.rs` (`cherry_pick`),
`crates/torii/src/cli.rs` (flags, despacho, tests de parseo),
`crates/torii/src/cli/history.rs` (flags de `Rebase`), `crates/torii/src/cli/repo.rs`
(`sync`, `cherry_pick`), y el `mod` de `cli` donde se declaran los demás
submódulos. `CHANGELOG.md` (`## [Unreleased]`).

**Ruling (Claude, desviación del spec):** la TUI no lanza hoy ningún merge,
rebase ni cherry-pick (su vista Sync solo hace pull fast-forward, push y
fetch), así que no hay dónde enganchar el selector. v1 no toca la TUI;
`PickerState` queda en un módulo sin dependencias de terminal para que la
TUI lo reutilice cuando tenga esas operaciones.

---

### Task 1: Resolver ficheros en conflicto (torii-core)

**Files:**
- Create: `crates/torii-core/src/vcs/conflict.rs`
- Modify: `crates/torii-core/src/vcs/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Side { Local, Remote }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum OpKind { Merge, Rebase, CherryPick }
  impl OpKind { pub fn favors_ours(self, side: Side) -> bool; } // Merge/CherryPick: Local→true; Rebase: Local→false
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub struct ConflictFile { pub path: String, pub hunks: bool } // hunks=false → se resuelve por fichero entero
  pub struct ConflictContext { pub op: OpKind, pub local_label: String, pub remote_label: String }
  #[derive(Clone, Debug, PartialEq, Eq)]
  pub enum Resolution { All(Side), PerFile(Vec<(String, Side)>), Manual }
  pub trait ConflictResolver { fn choose(&self, ctx: &ConflictContext, files: &[ConflictFile]) -> anyhow::Result<Resolution>; }
  pub struct ManualResolver;                 // siempre Manual
  pub struct FixedResolver(pub Side);        // siempre All(side)
  pub fn list_conflicts(repo: &git2::Repository) -> anyhow::Result<Vec<ConflictFile>>; // orden por path
  /// Aplica la resolución; devuelve cuántos ficheros siguen en conflicto.
  pub fn resolve_files(repo: &git2::Repository, op: OpKind, res: &Resolution) -> anyhow::Result<usize>;
  ```

Reglas:
- `hunks = true` solo si existen las entradas ours y theirs y ninguno de los dos blobs es binario (`Blob::is_binary`).
- Con `hunks`: `Index::merge_file_from_index` (ancestro, ours, theirs) con `MergeFileOptions::favor(Ours|Theirs)` según `favors_ours`; escribir el resultado en el árbol de trabajo con el modo del lado elegido y dejar la ruta en el índice sin conflicto (`conflict_remove` + `add_path`).
- Sin `hunks`: la entrada del lado elegido entera (blob + modo) al árbol de trabajo y al índice; si ese lado es `None`, borrar del árbol de trabajo y del índice.
- `PerFile` ignora rutas que no están en conflicto; los ficheros sin elección siguen en conflicto. `Manual` no toca nada.
- `index.write()` al final.

- [ ] **Step 1: Tests que fallan** en `conflict.rs`. Helper: repo temporal con firma explícita (sin depender de HOME), patrón de `core_extensions.rs:~2500` (`write` + `commit_all`), que deja un merge en conflicto con `repo.merge`.

```rust
#[test] fn favors_ours_mapping() {
    assert!(OpKind::Merge.favors_ours(Side::Local));
    assert!(!OpKind::Merge.favors_ours(Side::Remote));
    assert!(!OpKind::Rebase.favors_ours(Side::Local));
    assert!(OpKind::CherryPick.favors_ours(Side::Local));
}
#[test] fn local_wins_only_conflicting_hunk() {
    // base: "a\nb\nc\nd\ne\n"; ours cambia la línea 1 a "A-ours"; theirs cambia línea 1 a "A-theirs" y línea 5 a "E-theirs"
    // merge en conflicto → list_conflicts == [ConflictFile{path:"f.txt", hunks:true}]
    // resolve_files(Merge, All(Local)) == 0 → f.txt == "A-ours\nb\nc\nd\nE-theirs\n"; index sin conflictos
}
#[test] fn remote_wins_only_conflicting_hunk() { /* mismo repo, All(Remote) → "A-theirs\nb\nc\nd\nE-theirs\n" */ }
#[test] fn per_file_leaves_unchosen_in_conflict() {
    // dos ficheros en conflicto, PerFile([("a.txt", Remote)]) → devuelve 1; b.txt conserva marcadores "<<<<<<<"
}
#[test] fn manual_touches_nothing() { /* Manual → devuelve 2; contenido y conflictos intactos */ }
#[test] fn delete_vs_modify_takes_whole_side() {
    // ours borra g.txt, theirs lo modifica → hunks == false
    // All(Local) → g.txt no existe ni en disco ni en índice; en otro repo igual, All(Remote) → g.txt con el contenido de theirs
}
#[test] fn binary_conflict_takes_whole_side() { /* blobs con byte 0 distintos → hunks == false; All(Remote) → bytes de theirs */ }
#[test] fn rebase_local_means_own_commit() {
    // índice en conflicto donde ours = "upstream\n" y theirs = "mine\n" (como deja un rebase)
    // resolve_files(Rebase, All(Local)) → "mine\n"
}
```

- [ ] **Step 2:** `cargo test -p torii-lib vcs::conflict` → no compila (FAIL).
- [ ] **Step 3:** Implementar con las interfaces y reglas de arriba.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/vcs/mod.rs crates/torii-core/src/vcs/conflict.rs -m "feat(conflict): resolve conflicted files in favor of local or remote"`

---

### Task 2: Merge, rebase y cherry-pick con resolver (torii-core)

**Files:**
- Modify: `crates/torii-core/src/vcs/core_extensions.rs` (`merge_branch` ~1393, `rebase_branch` ~1465), `crates/torii-core/src/vcs/core_tag.rs` (`cherry_pick` ~76)

**Interfaces:**
- Consumes: todo lo de Task 1.
- Produces:
  ```rust
  #[derive(Debug, PartialEq, Eq)]
  pub enum IntegrateOutcome { Done, Conflicts(usize) } // en conflict.rs, re-exportado
  impl GitRepo {
      pub fn merge_branch_with(&self, branch: &str, r: &dyn ConflictResolver) -> Result<IntegrateOutcome>;
      pub fn rebase_branch_with(&self, branch: &str, r: &dyn ConflictResolver) -> Result<IntegrateOutcome>;
      pub fn cherry_pick_with(&self, commit: &str, r: &dyn ConflictResolver) -> Result<IntegrateOutcome>;
  }
  ```
  `merge_branch`, `rebase_branch` y `cherry_pick` conservan su firma y llaman a la versión `_with` con `&ManualResolver` (los tests existentes no cambian).

Reglas:
- En el punto donde hoy se detecta `index.has_conflicts()`: `list_conflicts` → `ConflictContext` → `r.choose` → `resolve_files`. Si quedan 0, seguir el camino sin conflicto que ya existe (commit de merge con dos padres + `cleanup_state`; `rebase.commit` y siguiente operación; commit del cherry-pick). Si quedan > 0, imprimir el mensaje de hoy y devolver `Conflicts(n)`.
- Etiquetas: merge → local = rama actual (o `HEAD`), remoto = `branch`. Rebase → local = `<sha7> <asunto>` del commit propio que se reaplica (`op.id()`), remoto = `branch`. Cherry-pick → local = rama actual, remoto = `<sha7> <asunto>` del commit.
- Rebase: se pregunta en cada parada con conflicto, con su propio contexto.

- [ ] **Step 1: Tests que fallan** en el módulo de tests de `core_extensions.rs` (reutilizar su helper de conflicto):

```rust
#[test] fn merge_with_remote_resolver_commits_merge() {
    // conflicto en f.txt (ours "ours\n", theirs "theirs\n") → merge_branch_with("origin/main", &FixedResolver(Side::Remote)) == Done
    // HEAD tiene 2 padres; f.txt == "theirs\n"; state() == Clean
}
#[test] fn merge_with_manual_resolver_reports_conflicts() { /* == Conflicts(1); state() == Merge */ }
#[test] fn rebase_with_local_resolver_keeps_own_content_across_stops() {
    // upstream y rama propia con 2 commits que chocan cada uno en un fichero distinto
    // rebase_branch_with(upstream, &FixedResolver(Side::Local)) == Done; los dos ficheros con el contenido propio; sin rebase en curso
}
#[test] fn cherry_pick_with_remote_resolver_commits() { /* en core_tag.rs: == Done, contenido del commit elegido, HEAD avanzó 1 */ }
```

- [ ] **Step 2:** `cargo test -p torii-lib merge_with_ rebase_with_ cherry_pick_with_` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** `cargo test -p torii-lib` → PASS (salvo `commit_during_merge_records_both_parents` si HOME está vacío: fallo previo conocido, no de este plan).
- [ ] **Step 5:** `torii save crates/torii-core/src -m "feat(conflict): merge, rebase and cherry-pick ask a resolver on conflicts"`

---

### Task 3: Prompt y selector en terminal (CLI)

**Files:**
- Create: `crates/torii/src/cli/conflict_picker.rs` (y su `mod` junto a los demás submódulos de `cli`)

**Interfaces:**
- Consumes: `Side`, `ConflictFile`, `ConflictContext`, `Resolution`, `ConflictResolver` (Task 1).
- Produces:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum FileState { Conflict, Remote, Local }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PickerKey { Up, Down, Space, Backspace, Enter, Cancel }
  #[derive(Debug, PartialEq, Eq)] pub enum PickerAction { Continue, Apply(Resolution), Cancel }
  pub struct PickerState { pub files: Vec<(String, FileState)>, pub cursor: usize }
  impl PickerState { pub fn new(files: &[ConflictFile]) -> Self; pub fn handle(&mut self, key: PickerKey) -> PickerAction; }
  pub struct TerminalResolver { pub forced: Option<Side> }
  impl ConflictResolver for TerminalResolver { ... }
  ```

Reglas (`PickerState::handle`):
- `Space`: Conflict→Remote, Remote→Local, Local→Remote.
- `Backspace`: → Conflict.
- `Up`/`Down`: mueven el cursor, sin salirse de la lista.
- `Enter` → `Apply(PerFile(..))` con los ficheros que no están en Conflict, en el orden de la lista.
- `Cancel` → `Cancel`.

`TerminalResolver::choose`:
- `forced = Some(s)` → `All(s)` sin preguntar.
- Si stdin o stdout no son tty (`IsTerminal`) → `Manual`.
- Si no, el prompt del spec, en stderr:

```text
⚠️  {n} files have conflicts ({op}: {remote_label} into {local_label})
  local  = {local_label} (your work)
  remote = {remote_label} (incoming)
Resolve: [l] local wins everywhere  [r] remote wins everywhere
         [f] choose file by file    [m] leave markers and resolve manually
```

  Lee teclas en raw mode (`crossterm`): `l`→`All(Local)`, `r`→`All(Remote)`, `m`/`Esc`→`Manual`, `f`→ selector. Cualquier otra tecla, incluida Enter, no hace nada.
- Selector: una línea por fichero con su estado y color (Conflict amarillo, Remote azul, Local verde), el cursor marcado, y una leyenda con estados y teclas (`space` cycle remote/local, `backspace` back to conflict, `↑↓/jk` move, `enter` apply, `esc/q` cancel). j/k equivalen a Down/Up y q/Esc a Cancel. `Cancel` → `Manual`.
- El raw mode se restaura siempre, también si hay error (guardia con `Drop`).

- [ ] **Step 1: Tests que fallan** (solo la máquina; el render se prueba a mano):

```rust
#[test] fn space_cycles_remote_local_remote() { /* Conflict→Remote→Local→Remote */ }
#[test] fn backspace_returns_to_conflict() {}
#[test] fn enter_applies_only_decided_files_in_order() {
    // a: Remote, b: sin tocar, c: Local → Apply(PerFile([("a",Remote),("c",Local)]))
}
#[test] fn cursor_stays_in_bounds() {}
#[test] fn cancel_cancels() {}
#[test] fn forced_side_skips_prompt() { /* TerminalResolver{forced:Some(Local)}.choose(..) == All(Local) */ }
```

- [ ] **Step 2:** `cargo test -p gitorii conflict_picker` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismo comando → PASS. Prueba a mano en un repo temporal con un conflicto en 2 ficheros: `l`, `r`, `m` y `f` (espacio ×3, backspace, enter); anotar el resultado en el informe.
- [ ] **Step 5:** `torii save crates/torii/src/cli -m "feat(conflict): interactive side picker for the terminal"`

---

### Task 4: Flags `--local`/`--remote` y cableado de la CLI

**Files:**
- Modify: `crates/torii/src/cli.rs` (`Sync` ~219, `CherryPick` ~399, despacho ~1318/~1356, tests de parseo), `crates/torii/src/cli/history.rs` (`Rebase` ~43, despacho ~413), `crates/torii/src/cli/repo.rs` (`sync` ~209, `cherry_pick` ~465), `CHANGELOG.md`
- Test: `crates/torii/tests/conflict_side.rs`

**Interfaces:**
- Consumes: `*_with`, `IntegrateOutcome` (Task 2); `TerminalResolver` (Task 3).
- Produces: en `Sync`, `CherryPick` e `History Rebase`:
  ```rust
  /// On conflicts, keep local changes in conflicting hunks (no prompt)
  #[arg(long, conflicts_with = "remote")] local: bool,
  /// On conflicts, take incoming changes in conflicting hunks (no prompt)
  #[arg(long)] remote: bool,
  ```

Reglas:
- `sync <rama> --merge` (y la integración por defecto), `sync <rama> --rebase`, `history rebase <base>` (rama no interactiva) y `cherry-pick <commit>` llaman a la versión `_with` con `TerminalResolver { forced }`.
- Con `Done`, el "✅ …" de hoy. Con `Conflicts(_)`, nada de "✅" (el mensaje "Resolve … and run …" ya lo imprimió core) y exit 0, como hoy.
- `--local`/`--remote` con `--interactive`, `--todo-file`, `--root`, `--continue`, `--abort` o `--skip` → error de clap.
- Ayuda: un ejemplo por comando (`torii sync origin/main --merge --remote`, `torii history rebase main --local`, `torii cherry-pick abc123 --remote`).
- `CHANGELOG.md`: entrada en `## [Unreleased]`, sección Added.

- [ ] **Step 1: Tests que fallan.** En `cli.rs`:

```rust
#[test] fn conflict_side_flags_parse_and_exclude() {
    // `sync origin/main --merge --remote` → remote true, local false
    // `sync origin/main --local --remote` → Err
    // `history rebase main --local` → local true
    // `history rebase main -i --local` → Err
    // `cherry-pick abc --remote` → remote true
}
```

En `crates/torii/tests/conflict_side.rs` (binario `env!("CARGO_BIN_EXE_torii")`, stdin null, HOME en un tempdir, repo temporal con conflicto en `f.txt` contra `origin/main` creado con git2):

```rust
#[test] fn merge_without_tty_or_flag_leaves_markers() {
    // `torii sync origin/main --merge` → exit 0; stdout sin "✅ Merged"; f.txt contiene "<<<<<<<"
}
#[test] fn merge_with_remote_flag_resolves_and_commits() {
    // `torii sync origin/main --merge --remote` → exit 0; f.txt == contenido de theirs; HEAD con 2 padres
}
#[test] fn cherry_pick_with_local_flag_keeps_head_content() {}
```

- [ ] **Step 2:** `cargo test -p gitorii conflict_side_flags_parse_and_exclude` y `cargo test -p gitorii --test conflict_side` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismos comandos → PASS. `cargo clippy -p gitorii -p torii-lib -- -D warnings` limpio.
- [ ] **Step 5:** `torii save crates/torii CHANGELOG.md -m "feat(conflict): --local/--remote and the interactive picker on sync, rebase and cherry-pick"`
