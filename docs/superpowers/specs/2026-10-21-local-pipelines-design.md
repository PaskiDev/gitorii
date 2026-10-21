# Pipelines locales en torii — diseño

Fecha: 2026-10-21 · Estado: borrador para aprobación

## Intención y porqué

Las plataformas (GitLab, GitHub) cobran la compilación y el despliegue en
minutos de CI. Cuando se acaban, el proyecto se queda sin poder publicar.
Otras plataformas, como Radicle, directamente no tienen CI: hoy
`RadiclePipelineClient` devuelve `Unsupported` y sugiere "run CI locally /
on your own runner" (`crates/torii-core/src/platforms/radicle/pipeline.rs`).

La idea es **reclamar en local la parte que hoy ponen las plataformas**:
torii ejecuta pipelines en contenedores Docker en la máquina del usuario, sin
depender de ningún servidor, ni de la plataforma ni de gitorii.com. Es una
función de torii para cualquier repo, no un script para gitorii.

Esto cambia dos líneas del `ROADMAP.md`:

- "Gate — CI/CD transpiler … Not a runner": el runner local no usa Gate y no
  depende del transpilador. El transpilador queda aparcado (decisión del
  usuario: "olvida el transpilador").
- "Runners observability … Out of scope: registering runners": sigue siendo
  cierto para los runners de las plataformas. Esto no registra nada en
  ninguna plataforma; ejecuta en local.

## Criterio de éxito

El release de gitorii que hoy hace `.gitlab-ci.yml` se puede hacer entero con
`torii pipeline run release` desde la máquina del usuario, **sin que GitLab ni
GitHub ejecuten ningún job**:

1. Compilar `torii` para `x86_64-unknown-linux-gnu`,
   `aarch64-unknown-linux-gnu` y `x86_64-pc-windows-gnu` en contenedores
   Linux (los dos últimos por compilación cruzada).
2. Subir los tres binarios al Generic Package Registry de GitLab
   (`packages/generic/gitorii/<tag>/<archivo>`).
3. Crear la Release en GitLab con las notas de `CHANGELOG.md` y enlaces a los
   binarios.
4. Crear la Release en GitHub con las mismas notas (hoy lo hace
   `.github/workflows/release.yml`), marcada como pre-release si el tag es
   `-rc`, `-beta` o `-alpha`.
5. Publicar los crates en crates.io con `cargo publish`.

**CI de las plataformas (decisión del usuario: todo local).** Se quita el
disparo por tag `v*` de `.gitlab-ci.yml` y de `.github/workflows/release.yml`;
los dos ficheros se conservan como plan B, ejecutables a mano. Coste
aceptado: el OIDC de crates.io (Trusted Publisher) solo existe dentro de
GitHub Actions, así que en local hace falta un `CARGO_REGISTRY_TOKEN` en
`secrets`, igual que los tokens de GitLab y GitHub.

El fichero `pipelines/release.toml` de este repo es la prueba de aceptación.

## Objetivos

- Definir pipelines en TOML dentro del repo y ejecutarlas en local con
  Docker, sin interfaz gráfica.
- Jobs con dependencias (`needs`), artefactos que pasan de un job a otro,
  caché entre ejecuciones, y límites de memoria/CPU por job.
- Secretos que vienen del entorno del host, nunca escritos en el TOML.
- Logs en vivo en terminal y guardados en disco.
- Que funcione igual en cualquier repo y para cualquier plataforma (o sin
  plataforma).

## No objetivos (v1)

- **macOS.** No hay contenedores macOS y el SDK de Apple solo se puede usar
  en hardware Apple. Fuera hasta nueva decisión (usuario: "olvida de
  momento mac").
- **Windows nativo.** Windows se cubre compilando para Windows desde
  contenedores Linux (mingw). No hay contenedores Windows ni uso de la VM.
- **Tests en el sistema destino.** Los tests corren en Linux; los binarios de
  otras plataformas no se ejecutan (usuario: le parece bien).
- **Transpilar a YAML de plataformas** (Gate). Proyecto aparte, aparcado.
- **Leer `.gitlab-ci.yml` o workflows de GitHub.** Ya lo hacen `gitlab-ci-local`
  y `act`; torii tiene su propio formato.
- **Ejecutar en otras máquinas** (la MacBook, la VM). Ver "Futuro".
- **Disparo automático** (al crear un tag, al hacer push). v1 es manual.
- **Vista en la TUI.** v1 es solo CLI.
- **Subcomandos `torii package upload` / `torii release create`.** Ver
  "Despliegue".

## Antiobjetivos — fallaría aunque funcione si…

- Depende de un servidor (gitorii.com, la plataforma) para ejecutar. Todo el
  sentido es no depender de nadie.
- Ejecuta comandos de un repo recién clonado sin que el usuario lo haya
  aceptado. Un `pipelines/*.toml` es código arbitrario igual que los hooks.
- Compila lo que hay sin commitear y lo publica como si fuera el tag. Lo que
  se publica tiene que ser exactamente el commit.
- Se convierte en un aprovisionador de runners (Ansible/Terraform). El
  ROADMAP ya lo descarta.

## Restricciones conocidas

- La máquina de compilación tiene memoria justa: el historial de este repo
  incluye OOM de `cargo test` (exit 137) y SIGSEGV de rustc arreglado con
  `RUST_MIN_STACK=33554432` y `CARGO_BUILD_JOBS=2`. Por defecto los jobs van
  **de uno en uno** y los límites por job son de primera clase.
- Docker tiene que estar instalado y accesible para el usuario. torii no lo
  instala.
- `.torii/` ya está en `.toriignore` (línea 22); ahí van los datos de
  ejecución.

## Decisiones tomadas

Las marcadas **(usuario)** las decidió el usuario. Las marcadas **(Claude)**
son mías: revisarlas.

### Formato: TOML en `pipelines/<nombre>.toml` (usuario: TOML; Claude: ruta)

Sigue el precedente de `policies/commits.toml`, que migró del DSL de Gate a
TOML para quitar la dependencia de `gate-lang`. Un fichero por pipeline; el
nombre del fichero es el nombre de la pipeline.

Boceto del esquema (el builder puede ajustar nombres de campos, no la
semántica):

```toml
# pipelines/release.toml

[defaults]                      # opcional, se aplica a todos los jobs
image  = "rust:1-bookworm"
memory = "6g"                   # docker --memory
cpus   = 2                      # docker --cpus
env    = { RUST_MIN_STACK = "33554432", CARGO_BUILD_JOBS = "2" }
cache  = ["/usr/local/cargo/registry", "/workspace/target"]

[[job]]
name      = "build-windows-x86_64"
env       = { TARGET = "x86_64-pc-windows-gnu" }
script    = """
apt-get update -qq && apt-get install -y -qq mingw-w64 nasm
rustup target add $TARGET
cargo build --release --target $TARGET
cp target/$TARGET/release/torii.exe torii-windows-x86_64.exe
"""
artifacts = ["torii-windows-x86_64.exe"]

[[job]]
name    = "release"
image   = "alpine:3.20"
needs   = ["build-linux-x86_64", "build-linux-aarch64", "build-windows-x86_64"]
secrets = ["GITLAB_TOKEN"]      # nombres; el valor sale del entorno del host
script  = """
apk add --no-cache curl jq
# los artefactos de los jobs de `needs` están en /artifacts/<job>/
...
"""
```

Campos por job: `name` (obligatorio, único), `image`, `script` (obligatorio,
se ejecuta con `sh -e`), `needs`, `env`, `secrets`, `artifacts`, `cache`,
`memory`, `cpus`. Todo lo de `[defaults]` se puede sobrescribir por job;
`env` se fusiona (gana el job).

### Comandos (Claude)

- `torii pipeline run <nombre>` — ejecuta la pipeline.
  - `--ref <tag|commit>` — qué commit construir (por defecto `HEAD`).
  - `--job <nombre>` — ejecuta solo ese job y lo que necesita.
  - `--parallel <N>` — jobs a la vez (por defecto 1).
- `torii pipeline check <nombre>` — valida el TOML sin ejecutar: campos,
  `needs` a jobs que existen, sin ciclos.
- `torii pipeline runs` — lista las ejecuciones locales guardadas.

Van bajo `torii pipeline`, que hoy gestiona las pipelines de las plataformas
(`list`, `cancel`, `retry`, `delete`). `run`, `check` y `runs` son
explícitamente locales y la ayuda lo dice. Alternativa descartada: un
namespace nuevo (`torii ci`), porque partiría "pipelines" en dos sitios.

### Qué se construye: el commit, no el árbol de trabajo (Claude)

torii exporta el árbol del commit de `--ref` (con git2, sin red) a un
directorio de trabajo de la ejecución y lo monta en el contenedor en
`/workspace`. Cambios sin commitear no entran. Si el árbol de trabajo está
sucio, torii avisa pero sigue.

### Motor de contenedores: CLI de `docker` (Claude)

torii llama a `docker` con `std::process::Command`, igual que
`util/hooks.rs` ejecuta comandos. Sin dependencia nueva (nada de
`bollard`). `TORII_CONTAINER_ENGINE=podman` cambia el binario; v1 solo se
prueba con Docker. Si `docker` no está o no responde, error claro antes de
empezar.

### Ejecución (Claude)

- Orden topológico de `needs`. Un job falla → los que dependen de él no se
  ejecutan y se marcan "skipped"; los independientes sí terminan. Código de
  salida de torii distinto de cero si algún job falló.
- Cada job es un `docker run --rm` con: `/workspace` (copia propia del
  árbol exportado para que los jobs no se pisen), `/artifacts/<job>/` de cada
  job de `needs` (solo lectura), los volúmenes de caché, `--memory`/`--cpus`
  si están, y las variables.
- Variables inyectadas en todos los jobs: `TORII_PIPELINE`, `TORII_JOB`,
  `TORII_RUN_ID`, `TORII_COMMIT`, `TORII_REF`, `TORII_TAG` (vacía si la ref
  no es un tag), `TORII_BRANCH`.
- Ctrl+C detiene los contenedores en marcha y marca la ejecución como
  cancelada.

### Artefactos, caché y logs (Claude)

- Todo en `.torii/pipelines/<run-id>/`: `<job>.log`, `artifacts/<job>/`,
  y un `run.toml` con estado, tiempos y commit. `run-id` = fecha-hora +
  pipeline.
- Al acabar el job, torii copia las rutas de `artifacts` (relativas a
  `/workspace`) a `artifacts/<job>/`.
- Caché: cada ruta de `cache` es un volumen Docker con nombre estable
  `torii-<hash del repo>-<hash de la ruta>`, compartido entre ejecuciones.
- Logs en vivo con el nombre del job delante de cada línea.

### Secretos (Claude)

`secrets` lista nombres de variables. torii lee el valor del entorno del host
y lo pasa al contenedor; si falta alguno, error antes de empezar. Los
valores nunca se escriben en `run.toml` ni en los logs (torii sustituye el
valor por `***` en la salida que guarda y muestra).

### Confianza (Claude, obligatorio)

Mismo modelo que `util/hooks.rs`: antes de ejecutar una pipeline, el usuario
la acepta para este repo; se guarda en `~/.config/torii/pipeline-trust.toml`
(fichero propio, separado de `hook-trust.toml`) por ruta del repo + sha256
del fichero. `sha2` ya es dependencia de `torii-core` (lo usa
`util/oauth.rs`); `hooks.rs` usa FNV-1a, que no sirve aquí. Si el fichero
cambia, se vuelve a preguntar. `TORII_TRUST_PIPELINES=1` acepta sin
preguntar (uso en scripts).

### Despliegue (Claude)

En v1 el despliegue es un job más: su `script` usa `curl` contra la API de la
plataforma con un token de `secrets`, como hace hoy el job `release` de
`.gitlab-ci.yml`. Eso basta para el criterio de éxito.

**Sin variables de plataforma (Claude).** torii no inyecta nada equivalente a
`CI_API_V4_URL`, `CI_PROJECT_ID` o `CI_JOB_TOKEN`: `run` no detecta el
remoto, para que funcione sin plataforma. Los datos del proyecto (URL de la
API, id del proyecto) van en el `env` del TOML, y el token en `secrets`.
Ojo: hoy `cli/pipeline.rs::run` llama a `detect_platform_full` antes de
mirar el subcomando; `run`, `check` y `runs` deben despacharse antes de esa
detección.

**El tag lo gestiona el script, no torii (Claude).** torii no comprueba que
`--ref` sea un tag ni que exista en el remoto; eso es política de cada
pipeline. El flujo de aceptación es: crear y subir el tag, luego
`torii pipeline run release --ref vX.Y.Z`. El job `release` falla si
`TORII_TAG` está vacío y crea la Release con `tag_name=$TORII_TAG`, que
GitLab exige que exista. Como torii construye el commit de ese mismo tag,
lo publicado es exactamente lo etiquetado.

Las piezas nativas (`torii package upload`, `torii release create`) no
existen: `PackageClient` y `ReleaseClient` en `torii-core/src/platforms/`
tienen listar/borrar/editar pero no subir/crear. Añadirlas haría el
despliegue igual en todas las plataformas, pero es un proyecto aparte que
viene después.

## Lo que se deja al builder

- Nombres exactos de campos TOML y de flags, mientras respeten la semántica
  de arriba.
- Estructura de módulos (sugerencia: el modelo y el ejecutor en
  `torii-core`, el comando en `crates/torii/src/cli/pipeline.rs`).
- Formato de `run.toml` y de la salida en terminal.
- Cómo se hace la copia de `/workspace` por job (copia, overlay, etc.).
- Cómo se borran ejecuciones viejas de `.torii/pipelines/` (puede no haber
  limpieza en v1).

## Futuro (no v1)

- **Otras máquinas como runners.** Docker ya admite `DOCKER_HOST=ssh://…`
  y `docker context`; un campo `host` por job llevaría el job a la MacBook o
  a la VM sin que torii aprovisione nada. Camino natural para macOS.
- macOS nativo, contenedores Windows.
- Disparo al crear un tag (`torii tag create`) o al hacer push.
- Vista en la TUI.
- `torii package upload` / `torii release create` para todas las
  plataformas.
- Que Gate genere el TOML o que torii acepte `.gate`.
