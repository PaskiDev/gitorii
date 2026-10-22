# Elegir lado en conflictos — diseño

Fecha: 2026-10-22 · Estado: aprobado

## Intención y porqué

Hoy, cuando un merge, rebase o cherry-pick deja conflictos, torii escribe
los marcadores `<<<<<<<` y termina con un mensaje del tipo "Resolve
conflicts and run: …". Hay que abrir cada fichero y resolverlo a mano,
aunque casi siempre la decisión es la misma: "aquí gana lo mío" o "aquí
gana lo que llega".

El usuario quiere que torii pregunte en ese momento y le deja elegir entre:

- **Todo el merge**: gana lo local o gana lo remoto en todos los ficheros
  en conflicto.
- **Fichero a fichero**: una lista interactiva donde marca cada fichero
  como local o remoto.

Ninguna elección se guarda como configuración: vale para ese merge.
**Nunca se elige un lado por defecto** (usuario, y regla de memoria
`merge-conflict-ask-side`).

## Qué significa "gana un lado" (usuario)

Igual que `git merge -X ours` / `-X theirs`: en cada fichero se conservan
los cambios de los dos lados que no chocan, y **solo los trozos en
conflicto** se resuelven a favor del lado elegido. No se toma el fichero
entero de un lado.

Conflictos sin trozos (binarios, borrado en un lado y modificado en el
otro, añadido en los dos con contenido distinto, etc.) → gana el fichero
entero del lado elegido; si en ese lado el fichero está borrado, se
borra. **(Claude)**

## Qué es "local" y qué es "remoto" (Claude)

"Local" siempre es el trabajo del usuario y "remoto" lo que llega, sea
cual sea la operación. git invierte ours/theirs en el rebase; torii lo
traduce para que el usuario no tenga que saberlo.

| operación | local | remoto |
|---|---|---|
| `torii sync <rama> --merge` | HEAD | `<rama>` |
| `torii sync <rama> --rebase` / `torii history rebase <base>` | el commit propio que se está reaplicando | `<base>` (lo que ya está en upstream) |
| `torii cherry-pick <commit>` | HEAD | `<commit>` |

Las etiquetas en pantalla dicen `local` y `remote` (la CLI está en
inglés), con una línea que explica qué es cada uno en esa operación
(p. ej. "remote = origin/main").

## Alcance

Operaciones cubiertas en v1 (todas las que hoy acaban en "Resolve
conflicts and run"):

- Merge: `merge_branch` (`crates/torii-core/src/vcs/core_extensions.rs:1393`).
- Rebase con git2: el bucle de `crates/torii-core/src/vcs/core_extensions.rs:~1480`.
  Un rebase puede parar en varios commits; se pregunta en cada parada.
- Cherry-pick: `crates/torii-core/src/vcs/core_tag.rs:76`.

Fuera de v1:

- El rebase interactivo (`history rebase -i`) va por el binario `git` y
  sus paradas las gestiona git. **(Claude)**
- `torii sync --pull` no hace merges (solo fast-forward), así que no
  aplica.
- Resolver a nivel de trozo (elegir lado trozo a trozo). El usuario quiere
  fichero a fichero.

## Flujo

Cuando una operación deja conflictos:

1. **Con terminal interactiva** (stdin y stdout son tty) → torii pregunta:
   ```
   ⚠️  3 files have conflicts (merge origin/main into main)
     local  = main (your work)
     remote = origin/main (incoming)
   Resolve: [l] local wins everywhere  [r] remote wins everywhere
            [f] choose file by file    [m] leave markers and resolve manually
   ```
   `m` (o Esc) deja todo como hoy. No hay opción por defecto: Enter solo
   no hace nada. **(Claude: texto y teclas)**
2. **`f`: selector fichero a fichero** (ver abajo).
3. Al confirmar, torii aplica las elecciones: los ficheros resueltos
   quedan escritos en el árbol de trabajo y en el índice, sin marcadores.
   Los que siguen en conflicto quedan con marcadores, como hoy.
4. Si ya no queda ningún conflicto, la operación sigue sola: el merge crea
   su commit, el rebase continúa al siguiente commit, el cherry-pick crea
   su commit. Si queda alguno, termina con el mensaje de hoy ("Resolve
   … and run …"). **(Claude: continuar solo)**

**Sin terminal interactiva** (script, CI, stdin redirigido) → se comporta
como hoy, salvo que se pase un flag:

- `--local` / `--remote` en `torii sync <rama> --merge|--rebase`,
  `torii history rebase <base>` y `torii cherry-pick <commit>` (usuario:
  "sí dale"). Eligen lado para todo el merge sin preguntar, también con
  terminal. Son excluyentes entre sí.

## Selector fichero a fichero (usuario, salvo lo marcado)

Lo mismo en la CLI y en la TUI:

- Lista de los ficheros en conflicto, uno por línea.
- `espacio`: conflicto → remoto; remoto → local; local → remoto (alterna
  entre los dos lados; la tercera pulsación vuelve a remoto). **(Claude,
  recomendado y aceptado por el usuario)**
- `backspace`: borra la preferencia; el fichero vuelve a conflicto.
- Cada estado (conflicto, remoto, local) tiene un color, y una leyenda en
  pantalla dice qué significa cada uno.
- **(Claude)** Navegación con ↑/↓ y j/k; `Enter` confirma y aplica; `Esc`
  o `q` cancela sin tocar nada (equivale a `m`). La leyenda incluye estas
  teclas.

**CLI:** selector en línea en la propia terminal, con `crossterm` (ya es
dependencia de `crates/torii`). Sin dependencias nuevas. **(Claude)**

**TUI:** cuando una operación lanzada desde la TUI deja conflictos, la TUI
muestra la misma pregunta y el mismo selector en su propio estilo
(`ratatui`). **(Claude: punto de enganche exacto lo decide el builder)**

## Restricciones

- Sin dependencias nuevas.
- Control de versiones del trabajo solo con `torii`, nunca `git`; sin
  trailer `Co-Authored-By`.
- Compilar y testear con `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=33554432`.
- torii-core se compila también para `x86_64-pc-windows-gnu`.
- Mensajes y ayuda en inglés, como el resto de la CLI.
- La lógica de resolver (dado un lado por fichero, escribir resultado y
  actualizar el índice) va en `torii-core` y se prueba sin terminal; la
  CLI y la TUI solo piden la elección.

## Antiobjetivos — fallaría aunque funcione si…

- Elige un lado sin que el usuario lo diga (ni por defecto, ni por
  configuración guardada).
- "Gana local" pierde cambios remotos que no chocaban (o al revés).
- En un rebase, "local" acaba significando upstream.
- Deja el repo en un estado a medias si el usuario cancela: cancelar debe
  dejarlo exactamente como hoy (marcadores y mensaje).
- Rompe scripts: sin tty y sin flag, el comportamiento es el de hoy.

## Lo que se deja al builder

- Cómo se calcula el resultado con git2 (p. ej. `merge_file_from_index`
  con `MergeFileOptions::favor`), siempre que cumpla "solo los trozos en
  conflicto".
- Estructura de módulos y nombres de tipos.
- Colores concretos.
- El punto exacto de la TUI donde se engancha el selector.
