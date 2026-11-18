# Jojo screens: Projects

Transcribed on 2026-11-17 from the canvas board "Projects" (`Projects.dc.html`, the M3 minimum); rebuilt on 2026-11-18 from `ProjectsMac.dc.html` (step 6). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md).

## 1. The board

`ProjectsMac.dc.html`: a projects column ("Projects · 2": "Crew", 2 repos · 3 features; "Create Project…", "Import Local Project…") beside the chosen project: name, path and operational pills (admission, running, blocked, awaiting approval, held); header "Open in Finder", "Project Settings", "New Feature"; tabs Overview, Repositories, Instructions, Configuration; "Repositories" with "Attach Repository…" (path, branch and commit, worktrees, health); "Missing configuration · 2" with fixes; "Test and CI commands"; "Features" with their stage; "Project instructions".

## 2. What the app shows today

Rebuilt on 2026-11-18 (step 6) in `macos/JojoApp/Sources/JojoApp/Projects.swift`. While the screen is shown it reads `Projects`, `ChatIndex` (registered repositories), `Features` (1.7), `ExternalActions` (gh's sign-in) and `ProjectDetail` (1.11) for the chosen project.

| Part | What it shows |
|---|---|
| Projects | A row per record: letter tile, name, "n repos · n features"; Create Project…; Import Local Project… disabled (a repository is registered from Feature Chat); the core's capacity |
| Header | Open in Finder (the project's repository paths, `NSWorkspace`), Project Settings (the form), New Feature (New Feature with the project chosen) |
| Head | Name, the first repository's path, pills from the features (running, blocked, awaiting approval), the admission and held attempts of the inspection |
| Repositories | Per repository: name, "main @ 9c1e4b2 · 2 worktrees (1 active)", path, issues and "Healthy" or "n issues" (`ProjectDetail`) |
| Missing configuration | The core's list (no test command, no CI command, no origin, an SSH origin) and gh not signed in; Edit Command for a command |
| Test and CI commands | The record's two commands |
| Features | The project's features with their stage line ("Execution · 2 running · 3 blocked") and stage pill; a row opens its chat |
| Project instructions | The text, "Edit Instructions" |
| Configuration (inspector) | Revision, default provider, task kinds, capacity, automatic repair, review, "Edit"; "Not built" |
| Form | Unchanged from M3 and M4: name, repositories, commands, task kinds, default provider, capacity, repair limit, review, instructions; Save Project sends the whole record |

The board's tabs are not built: everything is on one page. "Attach Repository…" is not built.

### 2.1 Known gaps

- Import Local Project and Attach Repository are not built; registering stays in Feature Chat.
- The project path is the first repository's; a project has no path of its own.
- Instructions revisions are revisions of the whole record.
- Worktrees counted are those the core owns for attempts; integration worktrees under `<data root>/integration/` are not counted.
- Health reads the repository with local Git when the screen is shown; nothing is read from the remote.

## 3. States

Reading (Loading form "Reading the project records…"); no projects (Empty form "No projects yet" with the sentence of what a project names); unreadable (warning banner "The project records could not be read", "Try Again"; "This core has no project records." for an older helper); disconnected (danger banner "Jojo core isn't responding", the records read before stay); editing.

## 4. Where the board and the decisions in force differ

| Board | In force |
|---|---|
| A project has a path and is imported or created from a folder | A project names registered repositories; registering stays in Feature Chat |
| Instructions revisions | Every save is a revision of the whole record; a plan keeps the revision it was drafted under, and the instructions reach attempts through the approved ADR |
| Test and CI commands are run | The test command is offered to the lead; host validation runs the approved validation plan's command. The CI command is stored and shown only |
| Capacity per project | One number for the whole core (D4); the last record saved sets it |

## 5. Repair and review (M4)

Not on the board. The record shows "Automatic repair" ("Off", "Once", "Up to 2 times") and "Cross-provider review" ("On: the other provider reviews each step", "Off"). The form sets them with three filter chips and a check box. Saving sends both with the whole record; a record saved before M4 reads as up to two repairs with the reviewer on. They apply from the next decision of every running plan of the project's repositories (the newest revision counts).
