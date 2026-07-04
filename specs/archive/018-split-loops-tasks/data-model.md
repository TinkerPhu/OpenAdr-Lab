# data-model.md

Entities

- TaskFile
  - name: string (e.g., "poll_events")
  - path: string (VEN/src/tasks/<name>.rs or VEN/src/tasks/<name>/mod.rs)
  - spawn_functions: [string] (names of spawn_* functions defined in the file)
  - helpers: [string] (names of helper functions defined or imported)
  - has_tests: bool (true if #[cfg(test)] module present)
  - production_line_count: integer (lines of production code, excludes test module)

- SpawnFunction
  - name: string (must start with "spawn_" and be unique)
  - signature: string (function parameters and return type)
  - responsibilities: string (short description)
  - belongs_to: TaskFile.name

- Helper
  - name: string
  - description: string
  - visibility: enum {private, pub(crate), pub}
  - used_by: [SpawnFunction.name]

- TestFunction
  - name: string
  - original_location: string (file::module where it was before migration)
  - target_file: TaskFile.path

- MigrationStep
  - id: string (e.g., "move-poll_events")
  - file_from: string
  - file_to: string
  - tests_run: [string] (unit test modules and BDD subset steps run)
  - status: enum {pending, in_progress, verified, done}

Validation rules

- Each SpawnFunction must be present in exactly one TaskFile.
- tasks/mod.rs must re-export every spawn_* public name so that main.rs only needs a module path change.
- Production code in a TaskFile must not exceed 200 lines (#[cfg(test)] modules excluded).
- All #[cfg(test)] functions originally in loops.rs must be migrated to the owning TaskFile's test module without deletion or renaming.
- After migration of each file, `cargo test` must report no loss in passing tests for migrated units; the global passing test count is verified at the end of the full migration.

State transitions

- MigrationStep: pending -> in_progress -> verified -> done

Examples

- TaskFile example: { name: "poll_events", path: "VEN/src/tasks/poll_events.rs", spawn_functions: ["spawn_event_poll"], helpers: ["detect_event_changes"], has_tests: true }
