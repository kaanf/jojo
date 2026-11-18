# Jojo screens: New Project

Built in step 7 (2026-11-18) from the canvas board `NewProjectMac.dc.html` ("Jojo macOS", Onboarding page, version `1794922102-b518`). Code: `macos/JojoApp/Sources/JojoApp/NewProject.swift`; requests in `JojoCore/Setup.swift`. Renders: `S17SetupTests` (`s17-new-project-…`).

## 1. The board

A window without the shell: a glass header (New Project, three numbered steps Sources, Reading, Review) and a footer (Cancel, a note, Back, Start Reading / Review Results / Open Discovery).

1. **Sources.** Project name; "Code · repositories on this Mac" with check rows (name, stack, path), Choose another folder…, Clone from GitHub…; Designs (Figma link, Screenshots, Design canvas); an aside "Jojo's starting plan" with the pipeline and what the next screen does read-only.
2. **Reading.** Progress, one row per task (done, running), "Continue in background".
3. **Review.** One card per proposal (Project brief; PRD draft from designs) with key, text and source per row, Edit and Approve, and a note that questions continue in Feature Chat.

## 2. What the app does

Opened from Onboarding's last step and from Projects' "Create Project…" (the project form stays for editing). It covers the window.

| Board | App |
|---|---|
| Repositories on this Mac | The Git repositories one or two levels inside the workspace folder (`Workspace`, protocol 1.12), with a stack read from their manifests (Cargo.toml, Package.swift, Gradle, Maven, package.json, tsconfig.json, go.mod, Python, Gemfile, composer.json, Makefile) and "Registered" when the core has them |
| Choose another folder… | A folder picker; the folder is registered (`RegisterRepository`). The core accepts only a repository inside the workspace folder and says so otherwise |
| Clone from GitHub…, Designs | Drawn disabled: not available yet |
| Start Reading | One chain, each command after the previous answer: register what is not registered, save the project (`SaveProject`, the setup's default provider, the helper's capacity), then one `BriefTurn` per repository, the next when the previous turn ended |
| Reading rows | One per repository from `ProjectDetail` at 1.12 (read every 2 s while shown): Waiting, Running, Done, Failed with the core's detail and "Read Again". Progress is counted in repositories; progress within a turn is not reported |
| Project brief card | The brief's rows (key, text, sources in mono, a warning row's sources in orange), revision, Proposed or Approved. "Approve Brief" reads the brief's review bundle and sends that decision (`ReviewBundle`, `HumanDecision`). A proposed test command is a check box; ticked, it is saved to the project (`SaveProject`) after the approval, and only when the project has none. "Edit" is "Read Again" (a new brief turn; the same content again is not a new revision) |
| PRD draft from designs | A card "Not available yet": the PRD is drafted in Feature Chat |
| Open Discovery, Answer in Feature Chat | Feature Chat's New Feature with this project chosen |
| Continue in background, Cancel | Back to the Dashboard; a running brief turn goes on and its brief waits in the Approval Inbox |

## 3. The brief in the core

A brief turn is a read-only lead turn of one registered repository (`crates/jojo-storage/src/briefs.rs`, prompt and reply in `crates/jojo-domain/src/brief.rs`, schema `jojo-project-brief-v1`): 1 to 8 rows of key, text, up to six repository-relative sources and a warning flag, and a proposed test command (absolute program path) or null. Its reply is the next revision of the repository's one brief (`project_briefs`, schema 17), authored by the model and approved by nobody until the human decides it. It takes the lead turn's place (no chat turn beside it) and one unit of capacity. An approved brief is appended to the project section of every later discovery and plan turn of that repository; an unapproved one is not. Briefs appear in `Documents` at 1.12 as kind `Brief` ("Project brief: <repository>") and are decided in the Approval Inbox like any document.

## 4. Known gaps

One brief per repository, read one at a time. No designs, links, files or cloning. Questions from the brief are not asked; gaps are "Watch out" rows. Progress within a turn and a notification when proposals are ready are not reported.
