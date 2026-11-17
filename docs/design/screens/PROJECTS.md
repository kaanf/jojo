# Jojo screens: Projects

Transcribed on 2026-11-17 from the canvas board "Projects" (`Projects.dc.html`, dark only), in short: the M3 minimum was decided by the human (a list and a form). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md).

## 1. The board

A projects column ("Projects · 2": "Crew", 2 repos · 3 features; "Jojo"; "Create Project…", "Import Local Project…") beside the chosen project: its name and path, "Open in Finder", "Project Settings", "New Feature"; tabs Overview, Repositories, Instructions, Configuration; operational pills (admission, running, blocked, awaiting approval, held); "Repositories" with "Attach Repository…" (per repository: path, branch and commit, worktrees, health); "Missing configuration · 2" with fixes; "Features" with their stage; "Test and CI commands" (a name and a command per line); "Project instructions" ("Given to every provider session for this project. Changes create a new instructions revision.").

## 2. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/Projects.swift`. The records are read with `Projects` while the screen is the shown one, and again when the snapshot's event number changes; the registered repositories come from `ChatIndex`.

| Part | What it shows |
|---|---|
| Projects | One list row per record: name, revision, repositories, default provider; "Shown" on the one in the detail. "Create Project…" opens an empty form with a new identity. A footnote states the core's capacity: "Parallel capacity of the core: n attempts in all, not split by provider. A lead turn takes one of them (D4)." |
| Detail | Repositories (paths), test command, CI command, task kinds ("backend → Claude Code"), default provider, parallel capacity; "Project instructions" as text; "Edit Project" |
| Form | Name, repositories (a chip per registered repository), test command and CI command (words split on spaces), task kinds ("backend=ClaudeCode, docs=Codex"), default provider (two chips), parallel capacity, instructions; "Save Project" sends the whole record as the next revision (`SaveProject`); "Discard". A save is disabled with a reason: no name, no repository, a kind pair that is not `kind=Provider`, a capacity outside 1 to 8, a command awaiting the core, no connection |
| Not reported | Repository health; features per project; missing configuration; "Open in Finder" and "Import Local Project" (not built; a repository is registered from Feature Chat) |

## 3. States

Reading (Loading form "Reading the project records…"); no projects (Empty form "No projects yet" with the sentence of what a project names); unreadable (warning banner "The project records could not be read", "Try Again"; "This core has no project records." for an older helper); disconnected (danger banner "Jojo core isn't responding", the records read before stay); editing.

## 4. Where the board and the decisions in force differ

| Board | In force |
|---|---|
| A project has a path and is imported or created from a folder | A project names registered repositories; registering stays in Feature Chat |
| Instructions revisions | Every save is a revision of the whole record; a plan keeps the revision it was drafted under, and the instructions reach attempts through the approved ADR |
| Test and CI commands are run | The test command is offered to the lead; host validation runs the approved validation plan's command. The CI command is stored and shown only |
| Capacity per project | One number for the whole core (D4); the last record saved sets it |
