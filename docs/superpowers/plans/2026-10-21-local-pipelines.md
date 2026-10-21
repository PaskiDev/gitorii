# Pipelines locales — plan de implementación

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `torii pipeline run|check|runs` ejecuta en local, con Docker, pipelines
definidas en `pipelines/<nombre>.toml`, y el release de gitorii pasa a
`pipelines/release.toml`.

**Architecture:** Módulo nuevo `torii_lib::local_pipeline` en `torii-core`:
modelo TOML → confianza → exportar el commit con git2 → planificador que
lanza cada job con `docker run` o `podman run` (CLI por subprocess) detrás de un trait
`Launcher`, para que el planificador se pruebe sin Docker. La CLI solo
parsea flags, instala el manejador de Ctrl+C y pinta.

**Tech Stack:** Rust, git2 0.19, toml 0.8, serde, sha2 0.10, chrono,
std::process, std::thread. Sin dependencias nuevas.

**Spec:** `docs/superpowers/specs/2026-10-21-local-pipelines-design.md`

## Global Constraints

- Sin dependencias nuevas en ningún `Cargo.toml` (nada de `bollard`, `ctrlc`, `which`).
- Control de versiones solo con `torii` (`torii save <ficheros> -m "..."`), nunca `git`.
- Compilar y testear con `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=33554432` (OOM y SIGSEGV de rustc en este host).
- Motor: Docker **o** Podman, sin integrarlos: torii no los empaqueta, no los instala y no añade crates para hablar con ellos. Solo se invoca el binario. `TORII_CONTAINER_ENGINE=docker|podman` fuerza uno; si no, `docker` y luego `podman`.
- Sin motor, `pipeline run` falla antes de empezar con `MISSING_ENGINE_HELP` (Task 5). Solo CLI en v1; la constante queda pública para que la futura vista de la TUI la muestre en una ventana.
- Jobs de uno en uno por defecto (`--parallel 1`).
- Datos de ejecución en `<repo>/.torii/pipelines/<run-id>/` (`.torii/` ya está ignorado).
- Confianza en `~/.config/torii/pipeline-trust.toml`, separado de `hook-trust.toml`, con sha256. `TORII_TRUST_PIPELINES=1` acepta sin preguntar.
- Variables inyectadas: `TORII_PIPELINE`, `TORII_JOB`, `TORII_RUN_ID`, `TORII_COMMIT`, `TORII_REF`, `TORII_TAG`, `TORII_BRANCH`. Nada de variables de plataforma.
- Los valores de `secrets` nunca van en argv, `run.toml` ni logs; en la salida se sustituyen por `***`.
- El script de cada job se ejecuta con `sh -e`.
- `run`, `check` y `runs` no detectan el remoto: se despachan antes de `detect_platform_full`.
- Mensajes de usuario y ayuda en inglés, como el resto de la CLI.

## Review Focus

1. Nombre de pipeline o de job con `/`, `..` o espacios (`torii pipeline run ../x`): error de validación, nunca una ruta fuera de `pipelines/` ni un nombre de contenedor inválido. → Task 1.
2. Secreto definido en el host pero vacío (`GITLAB_TOKEN=`): cuenta como ausente; enmascarar `""` destrozaría cada línea. → Task 6.
3. Repo en una ruta con espacios: cada `-v` va como un solo elemento de argv. → Task 5.
4. Máquina sin Docker ni Podman, o con Docker instalado y el demonio parado: mensaje que dice qué instalar o qué falla, nunca un `No such file or directory` crudo. → Tasks 5 y 7.
5. `--ref` que no existe, o `--job` que no existe: error antes de crear el directorio de ejecución. → Task 6.
6. Tag anotado frente a tag ligero: los dos rellenan `TORII_TAG` y construyen el commit al que apuntan, no el objeto tag. → Task 3.

## Estructura de ficheros

```
crates/torii-core/src/local_pipeline/
  mod.rs       re-exports
  model.rs     TOML → Pipeline validada (orden topológico, selección --job)
  trust.rs     pipeline-trust.toml + prompt
  checkout.rs  resolver --ref, exportar árbol, copiar directorios, árbol sucio
  store.rs     run-id, RunRecord (run.toml), RunDir, list_runs
  engine.rs    RunSpec, Launcher, Engine (docker CLI), nombres de volúmenes
  executor.rs  planificador, Masker, artefactos, cancelación
crates/torii/src/cli/local_pipeline.rs   handlers de run/check/runs + SIGINT
pipelines/release.toml
```

Se modifican: `crates/torii-core/src/lib.rs` (`pub mod local_pipeline;`),
`crates/torii/src/cli/pipeline.rs`, `crates/torii/src/cli.rs` (ayuda y
`mod`), `.gitlab-ci.yml`, `.github/workflows/release.yml`, `COMMANDS.md`,
`ROADMAP.md`, `CHANGELOG.md`.

Los tests unitarios van en `#[cfg(test)] mod tests` dentro de cada fichero,
como en `util/hooks.rs`. Repos de prueba: `tempfile::TempDir` +
`git2::Repository::init`.

---

### Task 1: Modelo TOML y validación

**Files:**
- Create: `crates/torii-core/src/local_pipeline/mod.rs`, `crates/torii-core/src/local_pipeline/model.rs`
- Modify: `crates/torii-core/src/lib.rs` (añadir `pub mod local_pipeline;` junto a los otros `pub mod`)

**Interfaces:**
- Produces:
  ```rust
  pub struct Job {
      pub name: String, pub image: String, pub script: String,
      pub needs: Vec<String>, pub env: BTreeMap<String, String>,
      pub secrets: Vec<String>, pub artifacts: Vec<String>,
      pub cache: Vec<String>, pub memory: Option<String>, pub cpus: Option<f64>,
  }
  pub struct Pipeline { pub name: String, pub jobs: Vec<Job> } // jobs en orden topológico
  pub fn parse(name: &str, src: &str) -> anyhow::Result<Pipeline>;
  pub fn pipeline_path(repo: &Path, name: &str) -> anyhow::Result<PathBuf>; // valida el nombre
  pub fn load(repo: &Path, name: &str) -> anyhow::Result<(Pipeline, String)>; // (pipeline, fuente cruda)
  impl Pipeline { pub fn select(&self, job: Option<&str>) -> anyhow::Result<Vec<&Job>>; }
  ```
  `mod.rs` re-exporta los tipos públicos de cada submódulo a medida que se crean.

Reglas que fija el spec:
- Ficheros serde privados con `#[serde(deny_unknown_fields)]`: `[defaults]` (opcional: `image`, `memory`, `cpus`, `env`, `cache`) y `[[job]]` (`name` y `script` obligatorios; `image`, `needs`, `env`, `secrets`, `artifacts`, `cache`, `memory`, `cpus`).
- Resolución: cada campo del job gana sobre `[defaults]`; `env` se fusiona (gana el job); `cache` del job sustituye al de defaults.
- Errores: sin `image` en job ni defaults; nombres repetidos; `needs` a un job que no existe; ciclo (el mensaje nombra los jobs del ciclo); `script` vacío; `cpus <= 0`; `artifacts` absolutos o con `..`; `cache` no absolutos (son rutas del contenedor); pipeline sin jobs.
- Nombres de pipeline y de job: solo `[A-Za-z0-9_.-]+` y sin empezar por `.`.
- `select(Some(j))`: `j` y sus `needs` transitivos, en orden topológico. Desconocido → error que lista los jobs válidos. `select(None)` → todos.
- Orden topológico estable: entre jobs listos, el orden del fichero.

- [ ] **Step 1: Tests que fallan** en `model.rs`:

```rust
const BASE: &str = r#"
[defaults]
image = "rust:1-bookworm"
env = { A = "1", B = "1" }
cache = ["/usr/local/cargo/registry"]

[[job]]
name = "test"
script = "cargo test"
needs = ["build"]
env = { B = "2" }

[[job]]
name = "build"
script = "cargo build"
cache = []
"#;

#[test] fn defaults_merge_and_override() {
    let p = parse("ci", BASE).unwrap();
    let t = p.jobs.iter().find(|j| j.name == "test").unwrap();
    assert_eq!(t.image, "rust:1-bookworm");
    assert_eq!(t.env["A"], "1"); assert_eq!(t.env["B"], "2");
    assert_eq!(t.cache, vec!["/usr/local/cargo/registry"]);
    let b = p.jobs.iter().find(|j| j.name == "build").unwrap();
    assert!(b.cache.is_empty());
}
#[test] fn jobs_come_out_in_topological_order() {
    let names: Vec<_> = parse("ci", BASE).unwrap().jobs.iter().map(|j| j.name.clone()).collect();
    assert_eq!(names, ["build", "test"]);
}
#[test] fn select_pulls_transitive_needs() {
    let p = parse("ci", BASE).unwrap();
    let s: Vec<_> = p.select(Some("test")).unwrap().iter().map(|j| j.name.as_str()).collect();
    assert_eq!(s, ["build", "test"]);
    assert_eq!(p.select(Some("build")).unwrap().len(), 1);
    assert!(p.select(Some("nope")).unwrap_err().to_string().contains("build"));
}
#[test] fn rejects_cycle_naming_jobs() { /* a needs b, b needs a → err contiene "a" y "b" y "cycle" */ }
#[test] fn rejects_unknown_need() { /* needs = ["ghost"] → err contiene "ghost" */ }
#[test] fn rejects_duplicate_names() {}
#[test] fn rejects_job_without_image() { /* sin defaults ni image → err contiene el nombre del job */ }
#[test] fn rejects_unknown_field() { /* `scrpit = ...` → Err */ }
#[test] fn rejects_bad_paths() { /* artifacts = ["../x"], artifacts = ["/x"], cache = ["rel"] → Err cada uno */ }
#[test] fn rejects_bad_names() {
    for bad in ["../x", "a/b", "a b", ".hidden", ""] {
        assert!(pipeline_path(Path::new("/r"), bad).is_err(), "{bad}");
    }
    assert_eq!(pipeline_path(Path::new("/r"), "release").unwrap(), Path::new("/r/pipelines/release.toml"));
    // job name "build linux" → parse Err
}
```

- [ ] **Step 2:** `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=33554432 cargo test -p torii-lib local_pipeline::model` → no compila (FAIL).
- [ ] **Step 3:** Implementar `model.rs` con las interfaces y reglas de arriba. `load` lee `pipeline_path`; si no existe, error que lista los `.toml` de `pipelines/`.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/lib.rs crates/torii-core/src/local_pipeline -m "feat(pipeline): parse and validate local pipeline TOML"`

---

### Task 2: Confianza

**Files:**
- Create: `crates/torii-core/src/local_pipeline/trust.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct TrustStore { path: PathBuf }
  impl TrustStore {
      pub fn new(path: PathBuf) -> Self;
      pub fn default_store() -> Option<Self>; // dirs::config_dir()/torii/pipeline-trust.toml
      pub fn is_trusted(&self, repo: &Path, pipeline: &str, source: &str) -> anyhow::Result<bool>;
      pub fn mark_trusted(&self, repo: &Path, pipeline: &str, source: &str) -> anyhow::Result<()>;
  }
  pub fn ensure_trusted(store: &TrustStore, repo: &Path, pipeline: &str, source: &str) -> anyhow::Result<()>;
  ```

Formato: TOML real (crate `toml`, no el parser a mano de `hooks.rs`):
una tabla `[trusted]` con clave `"<ruta canónica del repo>#<pipeline>"` y
valor el sha256 hex de `source`. `mark_trusted` sustituye la entrada de esa
clave y conserva las demás. `ensure_trusted`: confiado → Ok; si no,
`TORII_TRUST_PIPELINES` definida → `mark_trusted` y Ok; si no hay tty en
stdin → error que menciona `TORII_TRUST_PIPELINES=1`; si hay tty → muestra
la ruta del fichero y su contenido completo, pregunta `Trust and run? [y/N]`,
y solo `y`/`yes` guarda y sigue. Mismo flujo que `prompt_trust` en
`util/hooks.rs:170`.

- [ ] **Step 1: Tests que fallan:**

```rust
#[test] fn trust_roundtrip_and_change_invalidates() {
    let dir = tempfile::tempdir().unwrap();
    let s = TrustStore::new(dir.path().join("t.toml"));
    let repo = dir.path();
    assert!(!s.is_trusted(repo, "release", "a").unwrap());
    s.mark_trusted(repo, "release", "a").unwrap();
    assert!(s.is_trusted(repo, "release", "a").unwrap());
    assert!(!s.is_trusted(repo, "release", "b").unwrap());
    assert!(!s.is_trusted(repo, "ci", "a").unwrap());
}
#[test] fn retrust_replaces_and_keeps_others() {
    // mark(release,"a"), mark(ci,"x"), mark(release,"b")
    // → release con "b" sí, con "a" no; ci con "x" sí
}
```

- [ ] **Step 2:** `cargo test -p torii-lib local_pipeline::trust` (con las variables de entorno globales) → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/local_pipeline -m "feat(pipeline): trust store for local pipelines"`

---

### Task 3: Resolver la ref y exportar el commit

**Files:**
- Create: `crates/torii-core/src/local_pipeline/checkout.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ResolvedRef { pub spec: String, pub commit: String, pub tag: Option<String>, pub branch: Option<String> }
  pub fn resolve(repo: &git2::Repository, spec: &str) -> anyhow::Result<ResolvedRef>;
  pub fn export_tree(repo: &git2::Repository, commit: &str, dest: &Path) -> anyhow::Result<()>;
  pub fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()>;
  pub fn is_dirty(repo: &git2::Repository) -> anyhow::Result<bool>;
  ```

Reglas:
- `resolve`: `revparse_single(spec)` → `peel_to_commit`. `tag = Some(spec)` si existe `refs/tags/<spec>` (ligero o anotado; se aceptan también `spec` con el prefijo `refs/tags/`, y entonces `tag` lleva el nombre corto). `branch`: si `spec` es una rama local, su nombre; si `spec == "HEAD"` y HEAD está en una rama, el nombre de esa rama; si no, `None`. Ref inexistente → error que contiene `spec`.
- `export_tree`: recorre el árbol del commit sin red ni índice. Blobs `0o100755` con modo 755, `0o100644` con 644, symlinks como symlinks (unix), gitlinks (submódulos) se saltan con un aviso por `eprintln!`.
- `copy_dir`: recursivo, conserva permisos y symlinks (no los sigue).
- `is_dirty`: cambios en el índice o el árbol de trabajo, ignorando untracked ignorados.

- [ ] **Step 1: Tests que fallan:**

```rust
// helper: repo temporal con un commit que tiene `a.txt`, `bin/run.sh` (755) y `link -> a.txt`
#[test] fn resolves_head_on_branch() { /* branch == Some("main" o la rama inicial), tag None, commit == head */ }
#[test] fn resolves_lightweight_and_annotated_tags_to_commit() {
    // tag ligero "v1.0.0" y anotado "v1.0.1" sobre el mismo commit
    // ambos: commit == sha del commit (no del objeto tag), tag == Some(nombre), branch None
}
#[test] fn resolves_raw_sha_without_tag_or_branch() {}
#[test] fn unknown_ref_errors_with_name() { assert!(resolve(&r, "nope").unwrap_err().to_string().contains("nope")); }
#[test] fn export_writes_commit_not_worktree() {
    // modificar a.txt sin commitear → export → contenido del commit
    // bin/run.sh con modo 0o755; link es symlink a "a.txt"
    // is_dirty == true antes; tras restaurar a.txt, false
}
#[test] fn copy_dir_keeps_modes_and_links() {}
```

- [ ] **Step 2:** `cargo test -p torii-lib local_pipeline::checkout` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/local_pipeline -m "feat(pipeline): export the commit tree for local runs"`

---

### Task 4: Directorio de ejecución y `run.toml`

**Files:**
- Create: `crates/torii-core/src/local_pipeline/store.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)] #[serde(rename_all = "lowercase")]
  pub enum JobStatus { Pending, Running, Success, Failed, Skipped, Canceled }
  #[derive(...)] #[serde(rename_all = "lowercase")]
  pub enum RunStatus { Running, Success, Failed, Canceled }
  pub struct JobRecord { pub name: String, pub status: JobStatus, pub started: Option<String>,
                         pub finished: Option<String>, pub exit_code: Option<i32> }
  pub struct RunRecord { pub id: String, pub pipeline: String, pub git_ref: String, pub commit: String,
                         pub tag: Option<String>, pub started: String, pub finished: Option<String>,
                         pub status: RunStatus, pub jobs: Vec<JobRecord> }
  pub fn runs_root(repo: &Path) -> PathBuf;                       // <repo>/.torii/pipelines
  pub struct RunDir { pub root: PathBuf }
  impl RunDir {
      pub fn create(repo: &Path, pipeline: &str, now: chrono::DateTime<chrono::Local>) -> anyhow::Result<(String, RunDir)>;
      pub fn src_dir(&self) -> PathBuf;                   // root/src
      pub fn work_dir(&self, job: &str) -> PathBuf;       // root/work/<job>
      pub fn artifacts_dir(&self, job: &str) -> PathBuf;  // root/artifacts/<job>
      pub fn script_path(&self, job: &str) -> PathBuf;    // root/scripts/<job>.sh
      pub fn log_path(&self, job: &str) -> PathBuf;       // root/<job>.log
      pub fn write_record(&self, rec: &RunRecord) -> anyhow::Result<()>; // root/run.toml
  }
  pub fn list_runs(repo: &Path) -> anyhow::Result<Vec<RunRecord>>; // más reciente primero
  ```

Reglas: run-id = `now.format("%Y%m%d-%H%M%S")` + `-` + pipeline; si el
directorio ya existe, sufijo `-2`, `-3`… Fechas en RFC 3339. `run.toml`
en TOML con serde (campo `git_ref` serializado como `ref`).
`list_runs` salta directorios sin `run.toml` legible y ordena por `id`
descendente; sin `.torii/pipelines` → vector vacío.

- [ ] **Step 1: Tests que fallan:**

```rust
#[test] fn run_id_format_and_collision_suffix() {
    // now fijo 2026-10-21 17:46:41 → "20261021-174641-release", segunda llamada → "...-release-2"
}
#[test] fn record_roundtrip_and_listing_newest_first() {
    // escribir dos RunRecord en dos RunDir, un directorio basura sin run.toml
    // list_runs → 2 entradas, la de id mayor primero; status y jobs intactos
    // run.toml contiene la línea `ref = "...`
}
#[test] fn list_runs_without_dir_is_empty() {}
```

- [ ] **Step 2:** `cargo test -p torii-lib local_pipeline::store` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/local_pipeline -m "feat(pipeline): run directory and run.toml records"`

---

### Task 5: Motor de contenedores

**Files:**
- Create: `crates/torii-core/src/local_pipeline/engine.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct RunSpec {
      pub container_name: String,
      pub image: String,
      pub workspace: PathBuf,              // → /workspace (rw), también -w /workspace
      pub script: PathBuf,                 // → /torii/script.sh (ro)
      pub needs: Vec<(String, PathBuf)>,   // (job, dir) → /artifacts/<job> (ro)
      pub caches: Vec<(String, String)>,   // (volumen, ruta del contenedor)
      pub env: Vec<(String, String)>,      // en argv solo `-e NAME`; el valor va en el env del proceso
      pub memory: Option<String>,
      pub cpus: Option<f64>,
      pub owner: Option<(u32, u32)>,       // chown de /workspace al acabar
  }
  pub trait Launcher: Sync {
      /// Ejecuta el contenedor y llama a `on_line` por cada línea de stdout/stderr. Devuelve el código de salida.
      fn launch(&self, spec: &RunSpec, on_line: &(dyn Fn(&str) + Sync)) -> anyhow::Result<i32>;
      fn kill(&self, container_name: &str);
  }
  pub struct Engine { pub bin: String }
  #[derive(Debug)]
  pub enum EngineError {
      NotInstalled { tried: Vec<String> },                  // ningún binario en PATH
      NotResponding { bin: String, detail: String },        // existe pero `<bin> version` falla
      BadOverride(String),                                  // TORII_CONTAINER_ENGINE no es docker ni podman
  }
  impl std::fmt::Display for EngineError { ... }            // + impl std::error::Error
  pub const MISSING_ENGINE_HELP: &str;                      // texto de abajo
  impl Engine {
      pub fn detect() -> Result<Engine, EngineError>;       // lee TORII_CONTAINER_ENGINE
      pub fn detect_with(override_: Option<&str>, candidates: &[&str]) -> Result<Engine, EngineError>; // testable
      pub fn run_args(spec: &RunSpec) -> Vec<String>;
  }
  impl Launcher for Engine { ... }                  // kill = `<bin> kill <name>`, errores ignorados
  pub fn cache_volume_name(repo: &Path, container_path: &str) -> String;
  ```

`detect_with`: con override, solo ese candidato (`docker` o `podman`; otro
valor → `BadOverride`). Sin override, prueba `candidates` en orden
(`detect()` pasa `["docker", "podman"]`) con `<bin> version`: el primero
que sale con 0 gana. Si ninguno arranca porque no está en PATH →
`NotInstalled`; si alguno existe pero falla → `NotResponding` del primero
que existe, con su stderr. `Display` de `NotInstalled` y `BadOverride` acaba
en `MISSING_ENGINE_HELP`; el de `NotResponding` dice que `<bin>` está
instalado pero no responde y, para docker, sugiere comprobar que el demonio
corre y que el usuario está en el grupo `docker`. `MISSING_ENGINE_HELP`:

```text
Local pipelines need a container engine: install Docker or Podman.
  Docker: https://docs.docker.com/engine/install/
  Podman: https://podman.io/docs/installation
torii does not bundle or install either. Set TORII_CONTAINER_ENGINE=docker|podman to choose one.
```

`run_args` produce exactamente, en este orden (los bind mounts llevan `z`
para SELinux; Docker y Podman lo aceptan e ignoran fuera de SELinux):
`run --rm --name <container_name> -w /workspace -v <workspace>:/workspace:z
-v <script>:/torii/script.sh:ro,z`, por cada need `-v <dir>:/artifacts/<job>:ro,z`,
por cada cache `-v <vol>:<path>`, `--memory <m>` si hay, `--cpus <c>` si
hay, por cada env `-e <NAME>`, `<image> sh -c <WRAPPER>`. Con `owner`,
además `-e TORII_OWNER=<uid>:<gid>`. `WRAPPER` es la constante:

```sh
sh -e /torii/script.sh; rc=$?; [ -n "$TORII_OWNER" ] && chown -R "$TORII_OWNER" /workspace 2>/dev/null; exit $rc
```

`launch`: `Command::new(bin).args(run_args).envs(spec.env)`, stdout y stderr
por pipe, un hilo por stream que llama a `on_line`, `wait()`; sin código
(señal) → 128 + señal en unix, o -1.

`cache_volume_name`: `torii-<12 hex de sha256(ruta canónica del repo)>-<12 hex de sha256(container_path)>`.

- [ ] **Step 1: Tests que fallan:**

```rust
#[test] fn run_args_exact_order_and_spaces_stay_single_args() {
    let spec = RunSpec { container_name: "torii-r-build".into(), image: "alpine:3.20".into(),
        workspace: "/my repo/w".into(), script: "/my repo/s.sh".into(),
        needs: vec![("build".into(), "/my repo/a/build".into())],
        caches: vec![("torii-x-y".into(), "/cache".into())],
        env: vec![("TOKEN".into(), "s3cr3t".into())], memory: Some("6g".into()), cpus: Some(2.0), owner: Some((1000, 1000)) };
    let a = Engine::run_args(&spec);
    assert_eq!(&a[..9], ["run","--rm","--name","torii-r-build","-w","/workspace","-v","/my repo/w:/workspace:z","-v"]);
    assert!(a.contains(&"/my repo/a/build:/artifacts/build:ro,z".to_string()));
    assert!(a.windows(2).any(|w| w == ["-e", "TOKEN"]));
    assert!(!a.iter().any(|s| s.contains("s3cr3t")));
    assert!(a.windows(2).any(|w| w == ["-e", "TORII_OWNER=1000:1000"]));
    assert_eq!(&a[a.len()-4..a.len()-1], ["alpine:3.20", "sh", "-c"]);
}
#[test] fn cache_volume_name_is_stable_and_distinct() {
    // mismo repo+ruta → igual; otra ruta → distinto; formato "torii-" + 12 hex + "-" + 12 hex
}
#[test] fn detect_reports_missing_engines_with_help() {
    let e = Engine::detect_with(None, &["torii-no-such-a", "torii-no-such-b"]).unwrap_err();
    assert!(matches!(e, EngineError::NotInstalled { .. }));
    let msg = e.to_string();
    assert!(msg.contains("Docker") && msg.contains("Podman") && msg.contains("TORII_CONTAINER_ENGINE"));
}
#[test] fn detect_falls_through_to_next_candidate() {
    // `true` sale con 0 aunque reciba `version` → Engine { bin: "true" }
    assert_eq!(Engine::detect_with(None, &["torii-no-such-a", "true"]).unwrap().bin, "true");
}
#[test] fn detect_reports_installed_but_not_responding() {
    // `false` existe y sale con 1 → NotResponding { bin: "false", .. }; el mensaje contiene "false" y no MISSING_ENGINE_HELP
}
#[test] fn detect_rejects_unknown_override() {
    assert!(matches!(Engine::detect_with(Some("lxc"), &["docker", "podman"]), Err(EngineError::BadOverride(_))));
}
```

- [ ] **Step 2:** `cargo test -p torii-lib local_pipeline::engine` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismo comando → PASS.
- [ ] **Step 5:** `torii save crates/torii-core/src/local_pipeline -m "feat(pipeline): docker engine and launcher trait"`

---

### Task 6: Planificador

**Files:**
- Create: `crates/torii-core/src/local_pipeline/executor.rs`

**Interfaces:**
- Consumes: `Pipeline::select` (T1), `resolve`/`export_tree`/`copy_dir`/`is_dirty` (T3), `RunDir`/`RunRecord`/`JobStatus`/`RunStatus` (T4), `RunSpec`/`Launcher`/`cache_volume_name` (T5).
- Produces:
  ```rust
  pub struct RunOptions { pub git_ref: String, pub job: Option<String>, pub parallel: usize }
  pub struct RunOutcome { pub record: RunRecord, pub dir: PathBuf }
  pub fn run(repo: &Path, pipeline: &Pipeline, opts: &RunOptions, launcher: &dyn Launcher,
             cancel: &AtomicBool) -> anyhow::Result<RunOutcome>;
  pub struct Masker { secrets: Vec<String> }
  impl Masker { pub fn new(values: Vec<String>) -> Self; pub fn mask(&self, line: &str) -> String; }
  ```

Orden dentro de `run` (los errores de los pasos 1-3 salen antes de crear nada en disco):
1. `select(opts.job)`; abrir el repo con git2; `resolve(git_ref)`.
2. Reunir secretos de todos los jobs seleccionados desde el entorno del host. Ausente **o vacío** → un único error que lista todos los que faltan.
3. `parallel == 0` → error.
4. `is_dirty` → aviso por `eprintln!` y seguir.
5. `RunDir::create`, `export_tree` a `src_dir`, `run.toml` inicial con todos los jobs `Pending`.
6. Bucle: lanza jobs listos (todos sus `needs` en `Success`) hasta `parallel` a la vez, con `std::thread::scope` y un canal de terminados; `recv_timeout(200ms)` para mirar `cancel`. Un job cuyo `need` acabó `Failed`/`Skipped`/`Canceled` pasa a `Skipped` sin lanzarse. `run.toml` se reescribe en cada cambio de estado.
7. Por job: `copy_dir(src, work_dir)`; escribir `script_path`; `RunSpec` con `container_name = "torii-<run-id>-<job>"`, `needs` = `artifacts_dir` de cada need, `caches` con `cache_volume_name`, `env` = `job.env` + `TORII_*` + secretos (orden por nombre), `owner` = uid/gid de `repo` (`MetadataExt` en unix, `None` fuera). Cada línea → `Masker::mask` → stdout con prefijo `[<job>] ` y al `<job>.log` sin prefijo.
8. Salida 0 → copiar cada ruta de `artifacts` de `work_dir` a `artifacts_dir` (fichero o directorio); si una falta, el job pasa a `Failed` con la línea `artifact not produced: <ruta>` en log y terminal. Después borrar `work_dir`; si falla, aviso y seguir.
9. Cancelación: con `cancel` activo no se lanza nada más, se llama a `launcher.kill` de cada contenedor en marcha, lo pendiente pasa a `Canceled` y todo job que acabe después con código distinto de 0 también. `RunStatus`: `Canceled` si hubo cancelación, si no `Failed` si algún job falló, si no `Success`.

`Masker::mask`: sustituye cada valor por `***`, primero los más largos; ignora valores vacíos.

- [ ] **Step 1: Tests que fallan**, con un `FakeLauncher` en el módulo de tests que guarda el orden de `container_name`, devuelve el código configurado por job, escribe ficheros en `spec.workspace` para simular artefactos, emite líneas por `on_line` y puede activar `cancel` al lanzar un job dado:

```rust
#[test] fn masker_hides_longest_first_and_ignores_empty() {
    let m = Masker::new(vec!["ab".into(), "abc".into(), "".into()]);
    assert_eq!(m.mask("x abc ab y"), "x *** *** y");
}
#[test] fn runs_in_order_and_passes_artifacts() {
    // build (artifacts=["out/bin"]) → test (needs build)
    // fake: build escribe out/bin; test comprueba que spec.needs == [("build", <artifacts_dir/build>)] y que el fichero existe
    // status Success, los dos jobs Success, run.toml en disco con status "success"
}
#[test] fn failed_job_skips_dependents_but_not_independents() {
    // a falla(1); b needs a; c independiente → a Failed, b Skipped (nunca lanzado), c Success; run Failed
}
#[test] fn missing_artifact_fails_job() {}
#[test] fn secrets_reach_env_and_are_masked_in_log() {
    // TORII_TEST_SECRET_X="hunter2"; job secrets=["TORII_TEST_SECRET_X"]; fake emite "token=hunter2"
    // spec.env contiene el valor; <job>.log contiene "token=***" y no "hunter2"; run.toml no contiene "hunter2"
}
#[test] fn empty_or_missing_secret_errors_before_run_dir() {
    // TORII_TEST_SECRET_EMPTY="" y TORII_TEST_SECRET_ABSENT sin definir → Err que nombra los dos; .torii/pipelines no existe
}
#[test] fn unknown_ref_or_job_errors_before_run_dir() {}
#[test] fn injects_torii_vars() {
    // --ref v1.0.0 (tag) → TORII_TAG="v1.0.0", TORII_BRANCH="", TORII_COMMIT=sha, TORII_PIPELINE, TORII_JOB, TORII_RUN_ID, TORII_REF="v1.0.0"
}
#[test] fn cancel_kills_running_and_cancels_rest() {
    // a, b (needs a); fake activa cancel al lanzar a y devuelve 130
    // kill llamado con el contenedor de a; a Canceled, b Canceled; run Canceled
}
#[test] fn parallel_zero_is_rejected() {}
```

Los secretos de prueba usan nombres `TORII_TEST_SECRET_*` distintos por test para no pisarse entre hilos.

- [ ] **Step 2:** `cargo test -p torii-lib local_pipeline::executor` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** `cargo test -p torii-lib local_pipeline` → PASS (todo el módulo).
- [ ] **Step 5:** `torii save crates/torii-core/src/local_pipeline -m "feat(pipeline): schedule local jobs with artifacts, secrets and cancel"`

---

### Task 7: CLI `torii pipeline run|check|runs`

**Files:**
- Create: `crates/torii/src/cli/local_pipeline.rs`
- Modify: `crates/torii/src/cli/pipeline.rs` (variantes nuevas en `PipelineCommands`; despacho al principio de `run`, antes de `detect_platform_full`), `crates/torii/src/cli.rs` (`mod local_pipeline;`, texto de ayuda en el bloque de `Pipeline` hacia la línea 768, tests de parseo junto a `pipeline_remote_flag_is_global` en la línea 1570)
- Test: `crates/torii/tests/local_pipeline.rs`

**Interfaces:**
- Consumes: `load`, `ensure_trusted`, `TrustStore::default_store`, `Engine::detect`, `EngineError`, `run`, `list_runs` (T1-T6).
- Produces (en `PipelineCommands`):
  ```rust
  /// Run a pipeline from pipelines/<name>.toml locally in Docker (no platform involved)
  Run { name: String, #[arg(long = "ref", default_value = "HEAD")] git_ref: String,
        #[arg(long)] job: Option<String>, #[arg(long, default_value = "1")] parallel: usize },
  /// Validate pipelines/<name>.toml without running it (local)
  Check { name: String },
  /// List local pipeline runs stored in .torii/pipelines/
  Runs { #[arg(long, default_value = "20")] limit: usize },
  ```
  `pub(crate) fn run_local(action: &PipelineCommands) -> anyhow::Result<()>` en `local_pipeline.rs`.

Comportamiento:
- Raíz del repo con `git2::Repository::discover(current_dir)` → `workdir()`.
- `check`: `load`; OK → `✅ <name>: N jobs (<orden>)`; error → Err. Después, si `Engine::detect()` falla, imprime `⚠️ ` + el error por stderr, sin cambiar el exit 0.
- `run`: `load` → `Engine::detect()` (error → Err con su `Display`, antes de pedir confianza) → `ensure_trusted` (sin store → error) → instala SIGINT (unix: `libc::signal(SIGINT, handler)` cuyo handler hace `CANCEL.store(true)` sobre un `static CANCEL: AtomicBool`) → `run`. Al final, tabla de jobs con estado y la ruta de la ejecución. Estado distinto de `Success` → Err (exit ≠ 0).
- `runs`: tabla `ID  STATUS  PIPELINE  COMMIT(7)  STARTED`, hasta `limit`; vacío → `No local pipeline runs.`
- Ayuda de `torii pipeline`: añadir tres ejemplos (`run release --ref v0.16.0`, `check release`, `runs`) y una frase: `run`, `check` y `runs` son locales y no usan `--remote`.

- [ ] **Step 1: Tests que fallan.** En `cli.rs`:

```rust
#[test] fn pipeline_run_parses_local_flags() {
    let a = parse(&["pipeline", "run", "release", "--ref", "v1.0.0", "--job", "build", "--parallel", "2"]).unwrap();
    // match Commands::Pipeline { action: PipelineCommands::Run { name, git_ref, job, parallel }, .. }
    // name "release", git_ref "v1.0.0", job Some("build"), parallel 2
    // y `pipeline run release` → git_ref "HEAD", parallel 1
}
```

En `crates/torii/tests/local_pipeline.rs` (binario con `env!("CARGO_BIN_EXE_torii")`, repo temporal **sin remotos**):

```rust
#[test] fn check_works_without_remote() { /* pipelines/ci.toml válido → exit 0, stdout contiene "ci" */ }
#[test] fn check_reports_cycle() { /* exit ≠ 0, stderr contiene "cycle" */ }
#[test] fn runs_empty() { /* stdout contiene "No local pipeline runs." */ }
#[test] fn run_without_engine_explains_docker_or_podman() {
    // PATH apuntando a un tempdir vacío
    // → exit ≠ 0, stderr contiene "Docker", "Podman" y "TORII_CONTAINER_ENGINE"; no existe .torii/pipelines
}
#[test] fn check_without_engine_warns_but_passes() { /* mismo PATH vacío → exit 0, stderr contiene "Podman" */ }
#[test] fn run_untrusted_without_tty_refuses() {
    // motor falso que responde: script `docker` con `exit 0` en un tempdir, primero en PATH
    // stdin null, sin TORII_TRUST_PIPELINES, HOME/XDG_CONFIG_HOME en un tempdir → exit ≠ 0, stderr contiene "TORII_TRUST_PIPELINES=1"; no existe .torii/pipelines
}
#[test] #[ignore = "needs docker"] fn run_end_to_end_with_docker() {
    // ci.toml: job "make" (alpine:3.20) escribe out.txt con $TORII_JOB, artifacts=["out.txt"]; job "read" needs make y hace cat /artifacts/make/out.txt
    // TORII_TRUST_PIPELINES=1 → exit 0; .torii/pipelines/<id>/artifacts/make/out.txt == "make\n"; read.log contiene "make"
}
```

- [ ] **Step 2:** `cargo test -p gitorii pipeline_run_parses_local_flags` y `cargo test -p gitorii --test local_pipeline` → FAIL.
- [ ] **Step 3:** Implementar.
- [ ] **Step 4:** Mismos comandos → PASS. Luego `cargo test -p gitorii --test local_pipeline -- --ignored` con Docker → PASS, y otra vez con `TORII_CONTAINER_ENGINE=podman` si Podman está instalado (si no, decirlo en el informe). `cargo clippy -p gitorii -p torii-lib -- -D warnings` limpio.
- [ ] **Step 5:** `torii save crates/torii -m "feat(pipeline): torii pipeline run/check/runs for local Docker pipelines"`

---

### Task 8: `pipelines/release.toml`, CI de plataformas y docs

**Files:**
- Create: `pipelines/release.toml`
- Modify: `.gitlab-ci.yml`, `.github/workflows/release.yml`, `COMMANDS.md`, `ROADMAP.md`, `CHANGELOG.md`

`release.toml`, trasladado de `.gitlab-ci.yml` (`.build_linux`, jobs de build
y `release`) y de `.github/workflows/release.yml`:
- `[defaults]`: `image = "rust:1-bookworm"`, `memory = "6g"`, `cpus = 2`, `env = { RUST_MIN_STACK = "33554432", CARGO_BUILD_JOBS = "2" }`, `cache = ["/usr/local/cargo/registry", "/workspace/target"]`.
- `build-linux-x86_64`, `build-linux-aarch64`, `build-windows-x86_64`: `apt-get install` de los paquetes de cruce (`gcc-aarch64-linux-gnu`; `mingw-w64 nasm`), `rustup target add $TARGET`, linker por `CARGO_TARGET_<TRIPLE>_LINKER` en `env` (no escribir `.cargo/config.toml`), `cargo build --release --locked --target $TARGET`, copia a `torii-linux-x86_64`, `torii-linux-aarch64`, `torii-windows-x86_64.exe` y los declara en `artifacts`.
- `gitlab-release` (alpine:3.20, `needs` los tres builds, `secrets = ["GITLAB_TOKEN"]`, `env = { GITLAB_API = "https://gitlab.com/api/v4", GITLAB_PROJECT = "paskidev%2Fgitorii" }`, `cache = []`): mismo script que el job `release` actual con estos cambios: `TAG=$TORII_TAG`, cabecera `PRIVATE-TOKEN: $GITLAB_TOKEN` en las subidas y en la Release, binarios desde `/artifacts/<job>/`, URL del proyecto `https://gitlab.com/paskidev/gitorii`; 409 sigue siendo "ya existe".
- `github-release` (alpine:3.20, `needs` los tres builds, `secrets = ["GITHUB_TOKEN"]`, `env = { GITHUB_REPO = "PaskiDev/gitorii" }`, `cache = []`): `POST https://api.github.com/repos/$GITHUB_REPO/releases` con `name = "Torii $TORII_TAG"`, cuerpo = notas de `CHANGELOG.md` (mismo `awk`), `prerelease` si el tag casa con `-(rc|beta|alpha)\.`; 422 → "ya existe".
- `crates-publish` (`needs` los tres builds, `secrets = ["CARGO_REGISTRY_TOKEN"]`): `cargo publish --locked`.
- Todo job que publica empieza con `[ -n "$TORII_TAG" ] || { echo "run with --ref vX.Y.Z"; exit 1; }`.

CI de plataformas (todo local; ficheros como plan B):
- `.gitlab-ci.yml`: `workflow.rules` = solo `- if: $CI_PIPELINE_SOURCE == "web"` y `- when: never`, con un comentario arriba: el release normal es `torii pipeline run release --ref vX.Y.Z`; esto queda como plan B lanzándolo a mano desde la web sobre el tag.
- `.github/workflows/release.yml`: `on: push: tags` → `on: workflow_dispatch:`, mismo comentario (lanzarlo eligiendo el tag como ref).

Docs:
- `COMMANDS.md`: sección de `torii pipeline` con `run`, `check`, `runs`, el requisito de Docker o Podman (torii no los instala), el formato TOML (los campos de Task 1), `TORII_TRUST_PIPELINES`, `TORII_CONTAINER_ENGINE` y las variables `TORII_*`.
- `ROADMAP.md`: las dos líneas que cita el spec ("Gate … Not a runner" y "Runners observability … Out of scope") con una nota de que el runner local existe y no depende de Gate ni registra runners.
- `CHANGELOG.md`: entrada en la sección sin publicar (o crear `## [Unreleased]` si no hay) con el comando nuevo y el cambio de disparo del CI.

- [ ] **Step 1:** Escribir `pipelines/release.toml`.
- [ ] **Step 2:** `cargo run -p gitorii -- pipeline check release` → `✅ release: 6 jobs (...)`, con los tres builds antes que los tres de publicación.
- [ ] **Step 3:** `TORII_TRUST_PIPELINES=1 cargo run --release -p gitorii -- pipeline run release --job build-linux-x86_64` → exit 0 y `.torii/pipelines/<id>/artifacts/build-linux-x86_64/torii-linux-x86_64` existe y `--version` funciona. Solo build: **no** lanzar los jobs de publicación.
- [ ] **Step 4:** Lo mismo con `--job build-windows-x86_64` y `--job build-linux-aarch64`; `file` sobre cada binario dice PE32+ x86-64 y ELF aarch64.
- [ ] **Step 5:** Editar `.gitlab-ci.yml`, `.github/workflows/release.yml`, `COMMANDS.md`, `ROADMAP.md`, `CHANGELOG.md`. Comprobar YAML: `python3 -c 'import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]' .gitlab-ci.yml .github/workflows/release.yml` → sin salida.
- [ ] **Step 6:** `torii save pipelines .gitlab-ci.yml .github/workflows/release.yml COMMANDS.md ROADMAP.md CHANGELOG.md -m "ci: move the release to torii pipeline run release; platform CI becomes manual fallback"`

La prueba de aceptación completa (`torii pipeline run release --ref vX.Y.Z`
con los tres tokens) publica de verdad: la lanza el usuario en el próximo
release, no el implementador.
