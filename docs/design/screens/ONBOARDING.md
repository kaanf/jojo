# Jojo screens: Onboarding

Built in step 7 (2026-11-18) from the canvas board `OnboardingMac.dc.html` ("Jojo macOS", Onboarding page, version `1794922102-b518`). Code: `macos/JojoApp/Sources/JojoApp/Onboarding.swift`; setup kept by `JojoCore/Setup.swift` (`JojoSetup`). Renders: `S17SetupTests` (`s17-onboarding-…`).

## 1. The board

A window without the shell: a glass side panel (app tile, "Set up Jojo", three steps, three promises) and a main column with three steps and a footer ("Step n of 3 · setup only reads your machine", Back, Continue / Finish Machine Setup, Skip for Now / Continue to Project Setup).

1. **Agents and tools.** "We found 2 coding agents on this Mac", one row per agent (tile, name, status pill, note, path, Default radio, Allow switch; Install guide when missing), Rescan, and five tool cards (git, GitHub CLI, Xcode command line tools, JDK and Gradle wrapper, Docker).
2. **Autonomy.** Three presets (Careful, Balanced recommended, Autonomous), the rule table of the chosen preset and a red note that pushes always ask.
3. **First project.** "Your Mac is set up" summary, a drop zone (folder, links, files), four cards of what the human has (An idea, Designs, Documents, Code) and "Jojo's starting plan" with the pipeline, what Jojo does first and what the human decides.

## 2. When it shows

The app shows Onboarding instead of the shell when it was started without helper options, without `--ui-script` and with no saved setup (`LaunchOptions.needsSetup`). The helper is not started until step 1's workspace folder is confirmed ("Use This Folder"); then it is started with `JojoSetup.helperOptions`, the same options `scripts/live_m3_plan.py` passes: the folder as `--disposable-root`, each allowed agent on its native subscription (`--claude-home` HOME, `--claude-user` the account, `--codex-home` `~/.codex`) and the login shell. The helper creates the folder 0700 when it does not exist and its parent does (`prepare_disposable_root`); an existing folder is never changed. Later launches start the helper from the saved setup (`JojoSetup`, key `JojoSetup` in the app's settings). Settings › Storage › "Run Setup Again" shows Onboarding over a running app; a changed folder or agent takes effect when Jojo starts next.

## 3. What the app shows

| Board | App |
|---|---|
| Agent rows | Claude Code and Codex from `Executables` (path, version, Ready/Checking/Not found/Turned off). The note says the agent's sign-in is checked when a turn runs: the core does not report a provider's sign-in or usage limit. Gemini CLI is a row with "Not supported" |
| Allow switch, Default | App settings (`JojoSetup.claudeCode`, `codex`, `defaultProvider`). An agent that is off is not offered for new projects and its options are not passed at the next launch. The default is the provider New Project gives a project |
| Install guide | Not built; the note says to install it and start Jojo again (discovery looks again once, then only a helper restart finds it, STATUS.md "Known gaps") |
| Tool cards | git always (the core runs `/usr/bin/git`), with whether a credential helper is set; GitHub CLI signed in or not (`ExternalActions`); Xcode, JDK, Docker "Not reported by this core" |
| Workspace folder (not on the board) | A card above the agents: the folder, Choose…, Use This Folder; once the helper runs, "In use" or the core's reason it cannot be used (`Workspace.problem`) |
| Presets | The core enforces one policy, drawn as Balanced (recommended, selected). Careful and Autonomous are drawn but cannot be chosen: "Not built: the core runs one policy" |
| Rule table | What the core enforces: read files Allow; edit inside the task scope Allow (own worktree); run builds and tests "Jojo runs it" (the approved check after each step; Claude Code gets file tools only); write outside the scope "Fails the step" (no scope expansion request); dependencies or network "Not allowed" (no network tool); push or pull request "Always ask" |
| Drop zone | "Bring a project folder" opens New Project; Paste Link is disabled (not available yet) |
| Starting plan | Derived from the cards: code alone is "Learn the codebase"; code with anything else "New feature on existing code"; no code "Start from scratch". Designs and documents add the line that Jojo can't read them yet |
| Theme button | Toggles Settings › Appearance between Light and Dark |

## 4. Known gaps

No in-app restart of the helper: a different workspace folder or agent set needs Jojo to start again. A folder that is not private (0700) or less than three levels deep is refused by the core; the app does not change permissions. Provider sign-in, usage limits, Xcode, JDK, Docker and Gemini are not reported. Autonomy presets other than the core's one policy are not built.
